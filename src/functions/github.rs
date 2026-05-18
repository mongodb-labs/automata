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
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let body = interpolate(
        inputs["body"].as_str().context("body must be a string")?,
        ctx,
    )?;
    let comment_id = client.post_comment(&owner, &repo, number, &body).await?;
    Ok(json!({"output": {"comment_id": comment_id}}))
}

pub async fn add_label(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let label = interpolate(
        inputs["label"].as_str().context("label must be a string")?,
        ctx,
    )?;
    client.add_label(&owner, &repo, number, &label).await?;
    Ok(json!({"output": {}}))
}

pub async fn remove_label(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let label = interpolate(
        inputs["label"].as_str().context("label must be a string")?,
        ctx,
    )?;
    client.remove_label(&owner, &repo, number, &label).await?;
    Ok(json!({"output": {}}))
}

pub async fn approve_pr(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let review_id = client.approve_pr(&owner, &repo, number).await?;
    Ok(json!({"output": {"review_id": review_id}}))
}

pub async fn list_pr_comments(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let comments = client.list_comments(&owner, &repo, number).await?;
    Ok(json!({"output": {"comments": comments}}))
}

pub async fn get_commit(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let sha = interpolate(inputs["sha"].as_str().context("sha must be a string")?, ctx)?;
    let commit = client.get_commit(&owner, &repo, &sha).await?;
    Ok(json!({"output": commit}))
}

pub async fn enable_auto_merge(
    client: &GitHubClient,
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let owner = interpolate(
        inputs["owner"].as_str().context("owner must be a string")?,
        ctx,
    )?;
    let repo = interpolate(
        inputs["repo"].as_str().context("repo must be a string")?,
        ctx,
    )?;
    let number: u64 = interpolate(
        inputs["number"]
            .as_str()
            .context("number must be a string")?,
        ctx,
    )?
    .parse()?;
    let strategy = inputs
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("squash");
    let strategy = interpolate(strategy, ctx)?;
    client
        .enable_auto_merge(&owner, &repo, number, &strategy)
        .await?;
    Ok(json!({"output": {}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ExecutionContext;
    use crate::github::api::GitHubClient;
    use serde_json::json;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> ExecutionContext {
        ExecutionContext::new(json!({}))
    }

    fn sv(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.to_string())
    }

    async fn mock_client() -> (MockServer, GitHubClient) {
        let server = MockServer::start().await;
        let client = GitHubClient::new_with_base("token".to_string(), server.uri());
        (server, client)
    }

    #[tokio::test]
    async fn post_comment_wraps_comment_id() {
        let (server, client) = mock_client().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 7})))
            .mount(&server)
            .await;
        let mut inputs = HashMap::new();
        inputs.insert("owner".to_string(), sv("owner"));
        inputs.insert("repo".to_string(), sv("repo"));
        inputs.insert("number".to_string(), sv("42"));
        inputs.insert("body".to_string(), sv("comment text"));
        let result = post_comment(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["comment_id"], 7);
    }

    #[tokio::test]
    async fn add_label_returns_empty_output() {
        let (server, client) = mock_client().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/issues/1/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        let mut inputs = HashMap::new();
        inputs.insert("owner".to_string(), sv("owner"));
        inputs.insert("repo".to_string(), sv("repo"));
        inputs.insert("number".to_string(), sv("1"));
        inputs.insert("label".to_string(), sv("bug"));
        let result = add_label(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result, json!({"output": {}}));
    }

    #[tokio::test]
    async fn remove_label_returns_empty_output() {
        let (server, client) = mock_client().await;
        Mock::given(method("DELETE"))
            .and(path("/repos/owner/repo/issues/2/labels/auto_close"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        let mut inputs = HashMap::new();
        inputs.insert("owner".to_string(), sv("owner"));
        inputs.insert("repo".to_string(), sv("repo"));
        inputs.insert("number".to_string(), sv("2"));
        inputs.insert("label".to_string(), sv("auto_close"));
        let result = remove_label(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result, json!({"output": {}}));
    }

    #[tokio::test]
    async fn list_pr_comments_nests_under_output() {
        let (server, client) = mock_client().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/issues/3/comments"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{"id": 1, "body": "hi"}, {"id": 2, "body": "there"}])),
            )
            .mount(&server)
            .await;
        let mut inputs = HashMap::new();
        inputs.insert("owner".to_string(), sv("owner"));
        inputs.insert("repo".to_string(), sv("repo"));
        inputs.insert("number".to_string(), sv("3"));
        let result = list_pr_comments(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["comments"][0]["body"], "hi");
        assert_eq!(result["output"]["comments"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn get_commit_nests_under_output() {
        let (server, client) = mock_client().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/commits/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"sha": "abc", "commit": {"message": "msg"}})),
            )
            .mount(&server)
            .await;
        let mut inputs = HashMap::new();
        inputs.insert("owner".to_string(), sv("owner"));
        inputs.insert("repo".to_string(), sv("repo"));
        inputs.insert("sha".to_string(), sv("abc"));
        let result = get_commit(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["sha"], "abc");
        assert_eq!(result["output"]["commit"]["message"], "msg");
    }
}
