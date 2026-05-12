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
    let owner = interpolate(inputs["owner"].as_str().context("owner must be a string")?, ctx)?;
    let repo  = interpolate(inputs["repo"].as_str().context("repo must be a string")?, ctx)?;
    let number: u64 = interpolate(inputs["number"].as_str().context("number must be a string")?, ctx)?.parse()?;
    let body  = interpolate(inputs["body"].as_str().context("body must be a string")?, ctx)?;
    let comment_id = client.post_comment(&owner, &repo, number, &body).await?;
    Ok(json!({"comment_id": comment_id}))
}

pub async fn add_label(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(inputs["owner"].as_str().context("owner must be a string")?, ctx)?;
    let repo  = interpolate(inputs["repo"].as_str().context("repo must be a string")?, ctx)?;
    let number: u64 = interpolate(inputs["number"].as_str().context("number must be a string")?, ctx)?.parse()?;
    let label = interpolate(inputs["label"].as_str().context("label must be a string")?, ctx)?;
    client.add_label(&owner, &repo, number, &label).await?;
    Ok(json!({}))
}

pub async fn approve_pr(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner  = interpolate(inputs["owner"].as_str().context("owner must be a string")?, ctx)?;
    let repo   = interpolate(inputs["repo"].as_str().context("repo must be a string")?, ctx)?;
    let number: u64 = interpolate(inputs["number"].as_str().context("number must be a string")?, ctx)?.parse()?;
    let review_id = client.approve_pr(&owner, &repo, number).await?;
    Ok(json!({"review_id": review_id}))
}

pub async fn list_pr_comments(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner  = interpolate(inputs["owner"].as_str().context("owner must be a string")?, ctx)?;
    let repo   = interpolate(inputs["repo"].as_str().context("repo must be a string")?, ctx)?;
    let number: u64 = interpolate(inputs["number"].as_str().context("number must be a string")?, ctx)?.parse()?;
    let comments = client.list_comments(&owner, &repo, number).await?;
    Ok(json!({ "comments": comments }))
}

pub async fn enable_auto_merge(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner  = interpolate(inputs["owner"].as_str().context("owner must be a string")?, ctx)?;
    let repo   = interpolate(inputs["repo"].as_str().context("repo must be a string")?, ctx)?;
    let number: u64 = interpolate(inputs["number"].as_str().context("number must be a string")?, ctx)?.parse()?;
    let strategy = inputs
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("squash");
    let strategy = interpolate(strategy, ctx)?;
    client.enable_auto_merge(&owner, &repo, number, &strategy).await?;
    Ok(json!({}))
}
