use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use subtle::ConstantTimeEq;
use tracing::{error, info, warn};

use crate::app_state::AppState;
use crate::functions::Clients;
use crate::github::api::GitHubClient;
use crate::github::installation_token;
use crate::jira::JiraClient;

pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Validate sensor token using constant-time comparison to prevent timing attacks.
    // GitHub HMAC is validated upstream by the Argo Events EventSource.
    let sensor_token = headers
        .get("X-Automata-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if sensor_token
        .as_bytes()
        .ct_eq(state.config.sensor_token.as_bytes())
        .unwrap_u8()
        == 0
    {
        warn!("invalid sensor token");
        return StatusCode::UNAUTHORIZED;
    }

    // Parse the Sensor-wrapped envelope: {"github_event": "...", "body": {...}}
    let envelope: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!(%e, "invalid JSON envelope");
            return StatusCode::BAD_REQUEST;
        }
    };

    let event_type = match envelope.get("github_event").and_then(|v| v.as_str()) {
        Some(e) => e.to_string(),
        None => {
            warn!("missing github_event in envelope");
            return StatusCode::BAD_REQUEST;
        }
    };

    let payload = match envelope.get("body") {
        Some(p) => p.clone(),
        None => {
            warn!("missing body in envelope");
            return StatusCode::BAD_REQUEST;
        }
    };

    let repo = payload
        .pointer("/repository/full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    info!(event_type, repo, "received github event");

    let matched: Vec<_> = state
        .automations
        .iter()
        .filter(|a| crate::engine::matches_when(a, &event_type, &repo, &payload))
        .collect();

    if matched.is_empty() {
        info!("no matching automations");
        return StatusCode::OK;
    }

    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    let (owner, repo_name) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("", "")
    };

    let jwt = match crate::github::app_jwt(
        state.config.github_app_id,
        &state.config.github_app_private_key,
    ) {
        Ok(j) => j,
        Err(e) => {
            error!(%e, "failed to generate app JWT");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    let token = match installation_token(&state.http, &jwt, owner, repo_name).await {
        Ok(t) => t,
        Err(e) => {
            error!(%e, "failed to get installation token");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    let clients = Clients {
        github: GitHubClient::new(token),
        jira: JiraClient::new(
            &state.config.jira_base_url,
            &state.config.jira_user,
            &state.config.jira_api_token,
        ),
        http: state.http.clone(),
    };

    for automation in matched {
        info!(name = %automation.name, "running automation");
        if let Err(e) = crate::engine::run_automation(automation, &payload, &clients).await {
            error!(name = %automation.name, %e, "automation failed");
        }
    }

    StatusCode::OK
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
                github_webhook_secret: "secret".into(),
                sensor_token: "test-sensor-token".into(),
                jira_base_url: "https://jira.example.com".into(),
                jira_user: "user@example.com".into(),
                jira_api_token: "token".into(),
            },
            automations: Arc::new(vec![]),
            http: reqwest::Client::new(),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/webhook/github", post(handle))
            .with_state(test_state())
    }

    fn valid_envelope() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "github_event": "ping",
            "body": { "zen": "keep it simple" }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn missing_sensor_token_returns_401() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github")
                    .header("Content-Type", "application/json")
                    .body(Body::from(valid_envelope()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_sensor_token_returns_401() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github")
                    .header("X-Automata-Token", "wrong-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(valid_envelope()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_github_event_in_body_returns_400() {
        let body = serde_json::to_vec(&serde_json::json!({
            "body": { "zen": "keep it simple" }
        }))
        .unwrap();
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github")
                    .header("X-Automata-Token", "test-sensor-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn valid_ping_event_returns_200() {
        // Uses empty automations in test_state() so no matching automations path
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhook/github")
                    .header("X-Automata-Token", "test-sensor-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(valid_envelope()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
