use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{error, warn};

use crate::app_state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let sig_header = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !verify_signature(&state.config.github_webhook_secret, &body, sig_header) {
        warn!("invalid GitHub HMAC signature");
        return StatusCode::UNAUTHORIZED;
    }

    let event_type = match headers.get("X-GitHub-Event").and_then(|v| v.to_str().ok()) {
        Some(e) => e.to_string(),
        None => {
            warn!("missing X-GitHub-Event header");
            return StatusCode::BAD_REQUEST;
        }
    };

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!(%e, "invalid JSON body");
            return StatusCode::BAD_REQUEST;
        }
    };

    super::dispatch(&state, &event_type, payload).await
}

fn verify_signature(secret: &str, body: &[u8], sig_header: &str) -> bool {
    let hex_sig = match sig_header.strip_prefix("sha256=") {
        Some(s) => s,
        None => return false,
    };
    let sig_bytes = match hex::decode(hex_sig) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    mac.verify_slice(&sig_bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::app_state::AppState;
    use crate::config::Config;

    fn test_state() -> AppState {
        AppState {
            config: Config {
                port: 8080,
                github_app_id: 1,
                github_app_private_key: "pem".into(),
                github_webhook_secret: "test-secret".into(),
                sensor_token: "test-sensor-token".into(),
                jira_base_url: "https://jira.example.com".into(),
                jira_api_token: "token".into(),
            },
            automations: Arc::new(vec![]),
            http: reqwest::Client::new(),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/webhook/github/raw", post(handle))
            .with_state(test_state())
    }

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[tokio::test]
    async fn missing_signature_returns_401() {
        let body = b"{}";
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github/raw")
                    .header("X-GitHub-Event", "ping")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.as_ref()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_signature_returns_401() {
        let body = b"{}";
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github/raw")
                    .header("X-GitHub-Event", "ping")
                    .header("X-Hub-Signature-256", "sha256=deadbeef")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.as_ref()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_event_header_returns_400() {
        let body = b"{}";
        let sig = sign("test-secret", body);
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github/raw")
                    .header("X-Hub-Signature-256", sig)
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.as_ref()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn valid_ping_returns_200() {
        let body = b"{\"zen\":\"keep it simple\"}";
        let sig = sign("test-secret", body);
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github/raw")
                    .header("X-GitHub-Event", "ping")
                    .header("X-Hub-Signature-256", sig)
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.as_ref()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn verify_signature_ok() {
        let body = b"hello world";
        let sig = sign("mysecret", body);
        assert!(verify_signature("mysecret", body, &sig));
    }

    #[test]
    fn verify_signature_wrong_secret() {
        let body = b"hello world";
        let sig = sign("mysecret", body);
        assert!(!verify_signature("wrong", body, &sig));
    }

    #[test]
    fn verify_signature_tampered_body() {
        let sig = sign("mysecret", b"hello world");
        assert!(!verify_signature("mysecret", b"hello world!", &sig));
    }
}
