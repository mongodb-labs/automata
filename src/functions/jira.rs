use crate::context::ExecutionContext;
use crate::expr::interpolate;
use crate::jira::JiraClient;
use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn create_issue(
    client: &JiraClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let project = inputs["project"].as_str().context("project required")?;
    let issue_type = inputs.get("issue_type").and_then(|v| v.as_str()).unwrap_or("Story");
    let component = inputs["component"].as_str().context("component required")?;
    let summary_tpl = inputs["summary"].as_str().context("summary required")?;
    let summary = interpolate(summary_tpl, ctx)?;

    let mut custom_fields = HashMap::new();
    if let Some(cf) = inputs.get("custom_fields").and_then(|v| v.as_mapping()) {
        for (k, v) in cf {
            if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                custom_fields.insert(k.to_string(), v.to_string());
            }
        }
    }

    let (key, url) = client.create_issue(project, issue_type, component, &summary, &custom_fields).await?;
    Ok(json!({"key": key, "url": url}))
}

pub async fn transition(
    client: &JiraClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let key_tpl = inputs["key"].as_str().context("key required")?;
    let key = interpolate(key_tpl, ctx)?;
    let transition_id = inputs["transition_id"].as_str().context("transition_id required")?;
    client.transition(&key, transition_id).await?;
    Ok(json!({}))
}

pub async fn find_key(
    client: &JiraClient,
    _gh_client: &reqwest::Client,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let pattern = inputs["pattern"].as_str().context("pattern required")?;

    // Try branch first, then comments_url
    if let Some(branch_tpl) = inputs.get("branch").and_then(|v| v.as_str()) {
        let branch = interpolate(branch_tpl, ctx)?;
        if let Ok(key) = crate::jira::JiraClient::find_key_in_branch(&branch, pattern) {
            return Ok(json!({"key": key}));
        }
    }

    if let Some(url_tpl) = inputs.get("comments_url").and_then(|v| v.as_str()) {
        let url = interpolate(url_tpl, ctx)?;
        let key = client.find_key_in_comments(&url, pattern).await?;
        return Ok(json!({"key": key}));
    }

    anyhow::bail!("find_key requires branch or comments_url")
}
