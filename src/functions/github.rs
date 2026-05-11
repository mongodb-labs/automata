use crate::context::ExecutionContext;
use crate::expr::interpolate;
use crate::github::api::GitHubClient;
use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn post_comment(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let body_tpl = inputs["body"].as_str().context("body must be a string")?;
    let body = interpolate(body_tpl, ctx)?;
    let issue_number = issue_number_from_ctx(ctx)?;
    let comment_id = client.post_comment(issue_number, &body).await?;
    Ok(json!({"comment_id": comment_id}))
}

pub async fn add_label(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let label_tpl = inputs["label"].as_str().context("label must be a string")?;
    let label = interpolate(label_tpl, ctx)?;
    let issue_number = issue_number_from_ctx(ctx)?;
    client.add_label(issue_number, &label).await?;
    Ok(json!({}))
}

pub async fn approve_pr(
    client: &GitHubClient,
    _inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let pr_number = pr_number_from_ctx(ctx)?;
    let review_id = client.approve_pr(pr_number).await?;
    Ok(json!({"review_id": review_id}))
}

pub async fn enable_auto_merge(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let strategy = inputs
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("squash");
    let strategy = interpolate(strategy, ctx)?;
    let pr_number = pr_number_from_ctx(ctx)?;
    client.enable_auto_merge(pr_number, &strategy).await?;
    Ok(json!({}))
}

fn issue_number_from_ctx(ctx: &ExecutionContext) -> anyhow::Result<u64> {
    ctx.payload
        .pointer("/pull_request/number")
        .or_else(|| ctx.payload.pointer("/issue/number"))
        .and_then(|v| v.as_u64())
        .context("no issue/PR number in payload")
}

fn pr_number_from_ctx(ctx: &ExecutionContext) -> anyhow::Result<u64> {
    ctx.payload
        .pointer("/pull_request/number")
        .and_then(|v| v.as_u64())
        .context("no PR number in payload")
}
