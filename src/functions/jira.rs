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
                let json_val: serde_json::Value = serde_json::to_value(v)
                    .with_context(|| format!("failed to convert custom field {k} to JSON"))?;
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
    Ok(json!({"output": {"key": key, "url": url}}))
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
    Ok(json!({"output": {}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ExecutionContext;
    use crate::jira::JiraClient;
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

    fn base_inputs() -> HashMap<String, serde_yaml::Value> {
        let mut m = HashMap::new();
        m.insert("project".to_string(), sv("CLOUDP"));
        m.insert("issue_type".to_string(), sv("Story"));
        m.insert("component".to_string(), sv("AtlasCLI"));
        m.insert("summary".to_string(), sv("Test PR"));
        m
    }

    #[tokio::test]
    async fn create_issue_returns_key_and_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-1"})))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let result = create_issue(&client, &base_inputs(), &ctx()).await.unwrap();
        assert_eq!(result["output"]["key"], "CLOUDP-1");
        assert!(result["output"]["url"]
            .as_str()
            .unwrap()
            .contains("CLOUDP-1"));
    }

    #[tokio::test]
    async fn null_string_assignee_is_filtered_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-2"})))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let mut inputs = base_inputs();
        inputs.insert("assignee".to_string(), sv("null"));
        let result = create_issue(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["key"], "CLOUDP-2");
    }

    #[tokio::test]
    async fn empty_string_assignee_is_filtered_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-3"})))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let mut inputs = base_inputs();
        inputs.insert("assignee".to_string(), sv(""));
        let result = create_issue(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["key"], "CLOUDP-3");
    }

    #[tokio::test]
    async fn real_assignee_is_passed_through() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-4"})))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let mut inputs = base_inputs();
        inputs.insert("assignee".to_string(), sv("cloud-atlascli-escalation"));
        let result = create_issue(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result["output"]["key"], "CLOUDP-4");
    }

    #[tokio::test]
    async fn custom_fields_fix_versions_and_doc_changes_are_sent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-5"})))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let mut inputs = base_inputs();
        let custom_fields: serde_yaml::Value = serde_yaml::from_str(
            "fixVersions:\n  - name: \"Not Applicable\"\ncustomfield_10257:\n  value: \"Not Needed\"\n",
        )
        .unwrap();
        inputs.insert("custom_fields".to_string(), custom_fields);
        create_issue(&client, &inputs, &ctx()).await.unwrap();
        let req = server.received_requests().await.unwrap().remove(0);
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(
            body["fields"]["fixVersions"],
            json!([{"name": "Not Applicable"}])
        );
        assert_eq!(
            body["fields"]["customfield_10257"],
            json!({"value": "Not Needed"})
        );
    }

    #[tokio::test]
    async fn transition_returns_empty_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue/CLOUDP-10/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = JiraClient::new(&server.uri(), "token");
        let mut inputs = HashMap::new();
        inputs.insert("key".to_string(), sv("CLOUDP-10"));
        inputs.insert("transition_id".to_string(), sv("1381"));
        let result = transition(&client, &inputs, &ctx()).await.unwrap();
        assert_eq!(result, json!({"output": {}}));
    }
}
