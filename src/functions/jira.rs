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
            if let Some(k) = k.as_str() {
                let json_val: serde_json::Value = serde_json::to_value(v)
                    .unwrap_or(serde_json::Value::Null);
                custom_fields.insert(k.to_string(), json_val);
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

