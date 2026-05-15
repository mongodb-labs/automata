use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::info;

use crate::app_state::AppState;
use crate::github::{app_jwt, installation_token, list_app_repos};

#[derive(serde::Deserialize)]
pub struct InstallWebhookForm {
    pub repo: String,
}

enum WebhookStatus {
    Ok,
    WrongUrl,
    Missing,
    NoPermission,
}

pub async fn handle(State(state): State<AppState>) -> impl IntoResponse {
    let jwt = match app_jwt(
        state.config.github_app_id,
        &state.config.github_app_private_key,
    ) {
        Ok(j) => j,
        Err(e) => return Html(error_page(&format!("JWT error: {e}"))),
    };

    let automation_repos: HashSet<String> = collect_repos(&state.automations).into_iter().collect();

    let app_repos = list_app_repos(&state.http, &jwt).await.unwrap_or_default();
    info!(count = app_repos.len(), "GitHub App repos");

    let app_repo_map: HashMap<String, crate::github::AppRepo> = app_repos
        .into_iter()
        .map(|r| (r.full_name.clone(), r))
        .collect();

    let mut all_repos: Vec<String> = automation_repos
        .iter()
        .cloned()
        .chain(app_repo_map.keys().cloned())
        .collect();
    all_repos.sort();
    all_repos.dedup();

    info!(count = all_repos.len(), "checking repos");

    struct Row {
        repo: String,
        app_installed: bool,
        webhook: WebhookStatus,
        has_automations: bool,
        permissions: Vec<(String, String)>,
        can_install: bool,
    }

    let mut rows: Vec<Row> = Vec::new();

    for repo in &all_repos {
        let has_automations = automation_repos.contains(repo);
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }
        let (owner, name) = (parts[0], parts[1]);

        let row = match app_repo_map.get(repo) {
            None => Row {
                repo: repo.clone(),
                app_installed: false,
                webhook: WebhookStatus::NoPermission,
                has_automations,
                permissions: vec![],
                can_install: false,
            },
            Some(app_repo) => {
                let can_check_hooks = app_repo
                    .permissions
                    .get("repository_hooks")
                    .and_then(|v| v.as_str())
                    .is_some();
                let webhook = if can_check_hooks {
                    check_webhook(
                        &state.http,
                        &app_repo.token,
                        owner,
                        name,
                        state.config.webhook_url.as_deref(),
                    )
                    .await
                } else {
                    WebhookStatus::NoPermission
                };
                let can_install = can_check_hooks
                    && state.config.webhook_url.is_some()
                    && matches!(webhook, WebhookStatus::Missing | WebhookStatus::WrongUrl);
                let permissions = app_repo
                    .permissions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect();
                Row {
                    repo: repo.clone(),
                    app_installed: true,
                    webhook,
                    has_automations,
                    permissions,
                    can_install,
                }
            }
        };

        rows.push(row);
    }

    let table_rows: String = rows
        .iter()
        .map(|r| {
            format!(
                "<tr><td><a href='https://github.com/{repo}'>{repo}</a></td>\
                 <td class='center'>{}</td>\
                 <td class='center'>{}</td>\
                 <td class='center'>{}</td>\
                 <td class='center'>{}</td></tr>",
                icon(r.app_installed),
                webhook_cell(&r.webhook, &r.repo, r.can_install),
                icon(r.has_automations),
                permissions_icon(&r.permissions),
                repo = r.repo,
            )
        })
        .collect();

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>automata — doctor</title>
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 1000px; margin: 40px auto; padding: 0 20px; color: #222; }}
  h1 {{ font-size: 1.4rem; margin-bottom: 1rem; }}
  table {{ width: 100%; border-collapse: collapse; }}
  th {{ text-align: left; border-bottom: 2px solid #ddd; padding: 8px 12px; font-size: .85rem; color: #666; text-transform: uppercase; letter-spacing: .05em; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #f0f0f0; font-size: .9rem; vertical-align: middle; }}
  td.center {{ text-align: center; font-size: 1.1rem; }}
  td.perms {{ font-size: .8rem; color: #555; }}
  .perm {{ display: inline-block; background: #f4f4f4; border-radius: 3px; padding: 1px 6px; margin: 2px; }}
  a {{ color: #0366d6; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  .empty {{ color: #999; font-style: italic; padding: 20px 0; }}
  .btn-install {{ font-size: .75rem; padding: 2px 8px; border: 1px solid #0366d6; background: none; color: #0366d6; border-radius: 3px; cursor: pointer; vertical-align: middle; margin-left: 6px; }}
  .btn-install:hover {{ background: #0366d6; color: #fff; }}
</style>
</head>
<body>
<h1>automata — doctor</h1>
<table>
  <thead>
    <tr>
      <th>Repo</th>
      <th>App Installed</th>
      <th>Webhook</th>
      <th>Automations</th>
      <th>Permissions</th>
    </tr>
  </thead>
  <tbody>
    {table_rows}
  </tbody>
</table>
{empty}
</body>
</html>"#,
        empty = if rows.is_empty() {
            "<p class='empty'>No repos found.</p>"
        } else {
            ""
        },
    ))
}

fn icon(ok: bool) -> &'static str {
    if ok {
        "✅"
    } else {
        "❌"
    }
}

fn error_page(msg: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><title>automata — error</title></head>
<body><h1>Error</h1><pre>{msg}</pre></body></html>"#
    )
}

async fn check_webhook(
    client: &reqwest::Client,
    token: &str,
    owner: &str,
    repo: &str,
    expected_url: Option<&str>,
) -> WebhookStatus {
    let resp = client
        .get(format!("https://api.github.com/repos/{owner}/{repo}/hooks"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await;
    let hooks = match resp {
        Ok(r) if r.status().is_success() => match r.json::<Vec<Value>>().await {
            Ok(h) => h,
            Err(_) => return WebhookStatus::Missing,
        },
        _ => return WebhookStatus::Missing,
    };
    let active: Vec<String> = hooks
        .iter()
        .filter(|h| h["active"].as_bool().unwrap_or(false))
        .filter_map(|h| h["config"]["url"].as_str().map(str::to_owned))
        .collect();
    if active.is_empty() {
        return WebhookStatus::Missing;
    }
    match expected_url {
        None => WebhookStatus::WrongUrl,
        Some(want) => {
            if active.iter().any(|u| u == want) {
                WebhookStatus::Ok
            } else {
                WebhookStatus::WrongUrl
            }
        }
    }
}

fn webhook_icon(status: &WebhookStatus) -> &'static str {
    match status {
        WebhookStatus::Ok => "<span title='Active webhook found'>✅</span>",
        WebhookStatus::Missing => "<span title='No active webhook'>❌</span>",
        WebhookStatus::NoPermission => {
            "<span title='Cannot check: repository_hooks permission not granted'>❓</span>"
        }
        WebhookStatus::WrongUrl => "<span title='No automata webhook found'>❌</span>",
    }
}

fn webhook_cell(status: &WebhookStatus, repo: &str, can_install: bool) -> String {
    let icon = webhook_icon(status);
    if can_install {
        format!(
            "{icon}<form method='post' action='/doctor/install-webhook' style='display:inline'>\
            <input type='hidden' name='repo' value='{repo}'>\
            <button type='submit' class='btn-install'>Install</button></form>"
        )
    } else {
        icon.to_string()
    }
}

pub async fn install_webhook(
    State(state): State<AppState>,
    axum::extract::Form(params): axum::extract::Form<InstallWebhookForm>,
) -> impl IntoResponse {
    match do_install_webhook(&state, &params.repo).await {
        Ok(()) => Redirect::to("/doctor").into_response(),
        Err(e) => Html(error_page(&format!(
            "Failed to install webhook for {}: {e}",
            params.repo
        )))
        .into_response(),
    }
}

async fn do_install_webhook(state: &AppState, repo: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    anyhow::ensure!(parts.len() == 2, "invalid repo: {repo}");
    let (owner, name) = (parts[0], parts[1]);

    let webhook_url = state
        .config
        .webhook_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("WEBHOOK_URL not configured"))?;

    let jwt = app_jwt(
        state.config.github_app_id,
        &state.config.github_app_private_key,
    )?;
    let token = installation_token(&state.http, &jwt, owner, name).await?;

    let config = serde_json::json!({
        "url": webhook_url,
        "content_type": "json",
        "secret": state.config.github_webhook_secret,
        "insecure_ssl": "0"
    });
    let hook_body = serde_json::json!({
        "name": "web",
        "active": true,
        "events": ["issues", "pull_request"],
        "config": config,
    });

    // Fetch existing hooks to find one that already points to our URL.
    // If found but inactive → activate it (PATCH).
    // If not found → create it (POST).
    // This avoids 422 "Hook already exists" on both verbs.
    let hooks: Vec<Value> = state
        .http
        .get(format!("https://api.github.com/repos/{owner}/{name}/hooks"))
        .bearer_auth(&token)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "automata/1.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let existing = hooks
        .iter()
        .find(|h| h["config"]["url"].as_str() == Some(webhook_url));

    let resp = match existing {
        Some(h) if h["active"].as_bool().unwrap_or(false) => {
            // Already active and correct — nothing to do.
            return Ok(());
        }
        Some(h) => {
            // Exists but inactive — activate it.
            let id = h["id"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("hook missing id"))?;
            state
                .http
                .patch(format!(
                    "https://api.github.com/repos/{owner}/{name}/hooks/{id}"
                ))
                .bearer_auth(&token)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "automata/1.0")
                .json(&serde_json::json!({"active": true, "config": config}))
                .send()
                .await?
        }
        None => {
            state
                .http
                .post(format!("https://api.github.com/repos/{owner}/{name}/hooks"))
                .bearer_auth(&token)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "automata/1.0")
                .json(&hook_body)
                .send()
                .await?
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub {status}: {body}");
    }

    Ok(())
}

fn permissions_icon(perms: &[(String, String)]) -> String {
    // (permission, required_value) — None means any non-empty value is acceptable
    const REQUIRED: &[(&str, Option<&str>)] = &[
        ("actions", Some("write")),
        ("contents", None),
        ("issues", Some("write")),
        ("metadata", Some("read")),
        ("pull_requests", Some("write")),
        ("repository_hooks", Some("write")),
    ];
    let perm_map: std::collections::HashMap<&str, &str> = perms
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let missing: Vec<String> = REQUIRED
        .iter()
        .filter_map(|(name, required_val)| match perm_map.get(name) {
            None => Some(format!("{name}: missing")),
            Some(actual) => match required_val {
                Some(want) if *actual != *want => {
                    Some(format!("{name}: got {actual}, want {want}"))
                }
                _ => None,
            },
        })
        .collect();
    if missing.is_empty() {
        "<span title='All required permissions granted'>✅</span>".into()
    } else {
        let list = missing.join(", ");
        format!("<span title='Missing permissions: {list}'>⚠️</span>")
    }
}

/// Collect unique repos from all automation files (deduped, sorted).
pub fn collect_repos(automations: &[crate::types::Automation]) -> Vec<String> {
    let mut repos: Vec<String> = automations
        .iter()
        .flat_map(|a| {
            a.pipeline
                .iter()
                .flat_map(|e| e.given.repos.iter().cloned())
        })
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
                "name: a\npipeline:\n  - given:\n      trigger: github\n      repos:\n        - mongodb/b\n        - mongodb/a\n    when: []\n    then: []\n",
            )
            .unwrap(),
            serde_yaml::from_str(
                "name: b\npipeline:\n  - given:\n      trigger: github\n      repos:\n        - mongodb/a\n        - mongodb/c\n    when: []\n    then: []\n",
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
