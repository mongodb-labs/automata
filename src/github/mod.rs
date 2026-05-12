pub mod api;

use anyhow::Context as _;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct AppClaims {
    iat: u64,
    exp: u64,
    iss: String,
}

/// Generate a GitHub App JWT valid for 60 seconds.
pub fn app_jwt(app_id: u64, private_key_pem: &str) -> anyhow::Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let claims = AppClaims {
        iat: now - 60, // backdate 60s to account for clock skew
        exp: now + 60,
        iss: app_id.to_string(),
    };
    let key =
        EncodingKey::from_rsa_pem(private_key_pem.as_bytes()).context("invalid RSA private key")?;
    encode(&Header::new(Algorithm::RS256), &claims, &key).context("failed to encode JWT")
}

pub struct InstallationInfo {
    pub token: String,
    pub permissions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Installation {
    id: u64,
    #[serde(default)]
    permissions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct InstallationToken {
    pub token: String,
}

/// Exchange a GitHub App JWT for an installation access token for the given repo.
pub async fn installation_token(
    client: &reqwest::Client,
    jwt: &str,
    owner: &str,
    repo: &str,
) -> anyhow::Result<String> {
    // Find installation ID for the repo
    let install: Installation = client
        .get(format!(
            "https://api.github.com/repos/{owner}/{repo}/installation"
        ))
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Exchange for a token
    let token: InstallationToken = client
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            install.id
        ))
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(token.token)
}

/// Like `installation_token` but also returns the app's permissions on the repo.
pub async fn installation_info(
    client: &reqwest::Client,
    jwt: &str,
    owner: &str,
    repo: &str,
) -> anyhow::Result<InstallationInfo> {
    let install: Installation = client
        .get(format!(
            "https://api.github.com/repos/{owner}/{repo}/installation"
        ))
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let token_resp: InstallationToken = client
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            install.id
        ))
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(InstallationInfo {
        token: token_resp.token,
        permissions: install.permissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_jwt_rejects_invalid_key() {
        let result = app_jwt(12345, "not-a-valid-pem");
        assert!(result.is_err());
    }

    #[test]
    fn app_jwt_structure_with_valid_key() {
        use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
        let mut rng = rsa::rand_core::OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let token = app_jwt(12345, pem.as_str()).unwrap();
        assert_eq!(token.split('.').count(), 3);
    }
}
