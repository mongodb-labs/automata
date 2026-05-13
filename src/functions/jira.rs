use crate::context::ExecutionContext;
use crate::expr::{interpolate, interpolate_value};
use crate::jira::{CreateIssueParams, JiraClient};
use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn create_issue(
    client: &JiraClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let project = interpolate(inputs["project"].as_str().context("project required")?, ctx)?;
    let issue_type = interpolate(
        inputs
            .get("issue_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Story"),
        ctx,
    )?;
    let component = interpolate(
        inputs["component"].as_str().context("component required")?,
        ctx,
    )?;
    let summary_tpl = inputs["summary"].as_str().context("summary required")?;
    let summary = interpolate(summary_tpl, ctx)?;
    let description = inputs
        .get("description")
        .and_then(|v| v.as_str())
        .map(|tpl| interpolate(tpl, ctx))
        .transpose()?;

    let assignee_raw = inputs
        .get("assignee")
        .and_then(|v| v.as_str())
        .map(|tpl| interpolate(tpl, ctx))
        .transpose()?;
    let assignee = assignee_raw
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "null");

    let mut custom_fields = HashMap::new();
    if let Some(cf) = inputs.get("custom_fields").and_then(|v| v.as_mapping()) {
        for (k, v) in cf {
            if let Some(k) = k.as_str() {
                let json_val: serde_json::Value =
                    serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
                custom_fields.insert(k.to_string(), interpolate_value(&json_val, ctx)?);
            }
        }
    }

    let (key, url) = client
        .create_issue(CreateIssueParams {
            project: &project,
            issue_type: &issue_type,
            component: &component,
            summary: &summary,
            description: description.as_deref(),
            assignee,
            custom_fields: &custom_fields,
        })
        .await?;
    Ok(json!({"key": key, "url": url}))
}

pub async fn transition(
    client: &JiraClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let key_tpl = inputs["key"].as_str().context("key required")?;
    let key = interpolate(key_tpl, ctx)?;
    let transition_id_tpl = inputs["transition_id"]
        .as_str()
        .context("transition_id required")?;
    let transition_id = interpolate(transition_id_tpl, ctx)?;
    let fields: Option<serde_json::Value> = inputs
        .get("fields")
        .map(serde_json::to_value)
        .transpose()?
        .map(|v| interpolate_value(&v, ctx))
        .transpose()?;
    client
        .transition(&key, &transition_id, fields.as_ref())
        .await?;
    Ok(json!({}))
}
