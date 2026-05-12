use axum::{extract::State, response::IntoResponse, Json};
use serde_json::{json, Value};
use tracing::info;

use crate::app_state::AppState;
use crate::github::{app_jwt, installation_token};

pub async fn handle(State(state): State<AppState>) -> impl IntoResponse {
    let jwt = match app_jwt(state.config.github_app_id, &state.config.github_app_private_key) {
        Ok(j) => j,
        Err(e) => return Json(json!({"error": format!("jwt error: {e}")})),
    };

    let repos = collect_repos(&state.automations);
    info!(count = repos.len(), "checking repos");

    let mut statuses: Vec<Value> = Vec::new();

    for repo in &repos {
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }
        let (owner, name) = (parts[0], parts[1]);

        let token = installation_token(&state.http, &jwt, owner, name).await;

        let (github_access, webhook) = match token {
            Err(_) => (false, false),
            Ok(t) => {
                let has_hook = check_webhook(&state.http, &t, owner, name).await;
                (true, has_hook)
            }
        };

        statuses.push(json!({
            "repo": repo,
            "github_access": github_access,
            "webhook": webhook,
        }));
    }

    Json(json!({ "repos": statuses }))
}

async fn check_webhook(client: &reqwest::Client, token: &str, owner: &str, repo: &str) -> bool {
    let resp = client
        .get(format!("https://api.github.com/repos/{owner}/{repo}/hooks"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => r
            .json::<Vec<Value>>()
            .await
            .ok()
            .map(|hooks| hooks.iter().any(|h| h["active"].as_bool().unwrap_or(false)))
            .unwrap_or(false),
        _ => false,
    }
}

/// Collect unique repos from all automation files (deduped, sorted).
pub fn collect_repos(automations: &[crate::types::Automation]) -> Vec<String> {
    let mut repos: Vec<String> = automations
        .iter()
        .flat_map(|a| a.given.repos.iter().cloned())
        .collect();
    repos.sort();
    repos.dedup();
    repos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_repos_dedupes_and_sorts() {
        let autos: Vec<crate::types::Automation> = vec![
            serde_yaml::from_str(
                "name: a\ngiven:\n  trigger: github\n  repos:\n    - mongodb/b\n    - mongodb/a\nwhen: []\nthen: []\n",
            )
            .unwrap(),
            serde_yaml::from_str(
                "name: b\ngiven:\n  trigger: github\n  repos:\n    - mongodb/a\n    - mongodb/c\nwhen: []\nthen: []\n",
            )
            .unwrap(),
        ];
        let repos = collect_repos(&autos);
        assert_eq!(repos, vec!["mongodb/a", "mongodb/b", "mongodb/c"]);
    }

    #[test]
    fn collect_repos_empty_automations() {
        let repos = collect_repos(&[]);
        assert!(repos.is_empty());
    }
}
