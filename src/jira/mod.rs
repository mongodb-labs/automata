use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct CreateIssueParams<'a> {
    pub project: &'a str,
    pub issue_type: &'a str,
    pub component: &'a str,
    pub summary: &'a str,
    pub description: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub custom_fields: &'a HashMap<String, Value>,
}

pub struct JiraClient {
    client: reqwest::Client,
    base_url: String,
    api_token: String,
}

impl JiraClient {
    pub fn new(base_url: &str, api_token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_token: api_token.to_string(),
        }
    }

    fn auth(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.api_token).parse().unwrap(),
        );
        h.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        h.insert("User-Agent", "automata/1.0".parse().unwrap());
        h
    }

    pub async fn create_issue(&self, p: CreateIssueParams<'_>) -> anyhow::Result<(String, String)> {
        let mut fields = json!({
            "project": {"key": p.project},
            "issuetype": {"name": p.issue_type},
            "summary": p.summary,
            "components": [{"name": p.component}],
        });
        if let Some(desc) = p.description {
            fields["description"] = json!(desc);
        }
        if let Some(a) = p.assignee {
            fields["assignee"] = json!({"name": a});
        }
        for (field_id, value) in p.custom_fields {
            fields[field_id] = value.clone();
        }
        let body = json!({"fields": fields});
        tracing::debug!(body = %body, "jira create_issue request");
        let response = self
            .client
            .post(format!("{}/rest/api/2/issue", self.base_url))
            .headers(self.auth())
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Jira create_issue {status}: {body}");
        }
        let resp: Value = response.json().await?;
        let key = resp["key"].as_str().context("missing key")?.to_string();
        let url = format!("{}/browse/{}", self.base_url, key);
        Ok((key, url))
    }

    pub async fn transition(
        &self,
        key: &str,
        transition_id: &str,
        fields: Option<&Value>,
    ) -> anyhow::Result<()> {
        let mut body = json!({"transition": {"id": transition_id}});
        if let Some(f) = fields {
            body["fields"] = f.clone();
        }
        tracing::debug!(body = %body, "jira transition request");
        let response = self
            .client
            .post(format!(
                "{}/rest/api/2/issue/{}/transitions",
                self.base_url, key
            ))
            .headers(self.auth())
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Jira transition {status}: {body}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> JiraClient {
        JiraClient::new(&server.uri(), "token")
    }

    fn minimal_params<'a>(custom_fields: &'a HashMap<String, Value>) -> CreateIssueParams<'a> {
        CreateIssueParams {
            project: "CLOUDP",
            issue_type: "Story",
            component: "AtlasCLI",
            summary: "Test issue",
            description: None,
            assignee: None,
            custom_fields,
        }
    }

    #[tokio::test]
    async fn create_issue_returns_key_and_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-123"})))
            .mount(&server)
            .await;
        let cf = HashMap::new();
        let (key, url) = client(&server)
            .create_issue(minimal_params(&cf))
            .await
            .unwrap();
        assert_eq!(key, "CLOUDP-123");
        assert!(url.contains("CLOUDP-123"), "url should contain key: {url}");
    }

    #[tokio::test]
    async fn create_issue_with_description_and_assignee() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"key": "CLOUDP-456"})))
            .mount(&server)
            .await;
        let cf = HashMap::new();
        let (key, _) = client(&server)
            .create_issue(CreateIssueParams {
                description: Some("A description"),
                assignee: Some("cloud-atlascli-escalation"),
                ..minimal_params(&cf)
            })
            .await
            .unwrap();
        assert_eq!(key, "CLOUDP-456");
    }

    #[tokio::test]
    async fn create_issue_propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;
        let cf = HashMap::new();
        let result = client(&server).create_issue(minimal_params(&cf)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("400"));
    }

    #[tokio::test]
    async fn transition_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue/CLOUDP-123/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        client(&server)
            .transition("CLOUDP-123", "1381", None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn transition_with_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue/CLOUDP-999/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let fields = json!({"resolution": {"name": "Fixed"}});
        client(&server)
            .transition("CLOUDP-999", "1381", Some(&fields))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn transition_propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/2/issue/CLOUDP-1/transitions"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;
        let result = client(&server).transition("CLOUDP-1", "999", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("404"));
    }
}
