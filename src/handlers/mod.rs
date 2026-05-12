pub mod doctor;
pub mod github;
pub mod github_raw;

use axum::http::StatusCode;
use tracing::{error, info};

use crate::app_state::AppState;
use crate::functions::Clients;
use crate::github::api::GitHubClient;
use crate::github::installation_token;
use crate::jira::JiraClient;

/// Run all automations that match `event_type` + `payload` and return the HTTP status.
pub async fn dispatch(
    state: &AppState,
    event_type: &str,
    payload: serde_json::Value,
) -> StatusCode {
    let repo = payload
        .pointer("/repository/full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    info!(event_type, repo, "received github event");

    let matched: Vec<(&str, &crate::types::PipelineEntry)> = state
        .automations
        .iter()
        .flat_map(|a| a.pipeline.iter().map(move |e| (a.name.as_str(), e)))
        .filter(|(_, e)| crate::engine::matches_when(e, event_type, &repo, &payload))
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
        jira: JiraClient::new(&state.config.jira_base_url, &state.config.jira_api_token),
    };

    for (name, entry) in matched {
        info!(%name, "running automation");
        if let Err(e) = crate::engine::run_automation(entry, &payload, &clients).await {
            error!(%name, %e, "automation failed");
        }
    }

    StatusCode::OK
}
