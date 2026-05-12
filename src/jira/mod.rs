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
