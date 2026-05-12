use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct JiraClient {
    client: reqwest::Client,
    base_url: String,
    user: String,
    api_token: String,
}

impl JiraClient {
    pub fn new(base_url: &str, user: &str, api_token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            user: user.to_string(),
            api_token: api_token.to_string(),
        }
    }

    fn auth(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
        use base64::{engine::general_purpose::STANDARD, Engine};
        let creds = STANDARD.encode(format!("{}:{}", self.user, self.api_token));
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, format!("Basic {creds}").parse().unwrap());
        h.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        h.insert("User-Agent", "automata/1.0".parse().unwrap());
        h
    }

    pub async fn create_issue(
        &self,
        project: &str,
        issue_type: &str,
        component: &str,
        summary: &str,
        custom_fields: &HashMap<String, String>,
    ) -> anyhow::Result<(String, String)> {
        let mut fields = json!({
            "project": {"key": project},
            "issuetype": {"name": issue_type},
            "summary": summary,
            "components": [{"name": component}],
        });
        for (field_id, value) in custom_fields {
            fields[field_id] = json!({"id": value});
        }
        let resp: Value = self.client
            .post(format!("{}/rest/api/2/issue", self.base_url))
            .headers(self.auth())
            .json(&json!({"fields": fields}))
            .send().await?
            .error_for_status()?
            .json().await?;
        let key = resp["key"].as_str().context("missing key")?.to_string();
        let url = format!("{}/browse/{}", self.base_url, key);
        Ok((key, url))
    }

    pub async fn transition(&self, key: &str, transition_id: &str) -> anyhow::Result<()> {
        self.client
            .post(format!("{}/rest/api/2/issue/{}/transitions", self.base_url, key))
            .headers(self.auth())
            .json(&json!({"transition": {"id": transition_id}}))
            .send().await?
            .error_for_status()?;
        Ok(())
    }

}
