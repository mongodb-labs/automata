use anyhow::Context as _;
use regex::Regex;
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

    /// Fetch comments from a GitHub comments_url and extract the first Jira key matching `pattern`.
    pub async fn find_key_in_comments(&self, comments_url: &str, pattern: &str) -> anyhow::Result<String> {
        let comments: Vec<Value> = reqwest::Client::new()
            .get(comments_url)
            .header("User-Agent", "automata/1.0")
            .send().await?
            .error_for_status()?
            .json().await?;
        let re = Regex::new(pattern).context("invalid jira key pattern")?;
        for comment in &comments {
            if let Some(body) = comment["body"].as_str() {
                if let Some(m) = re.find(body) {
                    return Ok(m.as_str().to_string());
                }
            }
        }
        anyhow::bail!("no Jira key matching {pattern} found in comments")
    }

    /// Extract a Jira key from a branch name like "CLOUDP-1234-some-description".
    pub fn find_key_in_branch(branch: &str, pattern: &str) -> anyhow::Result<String> {
        let re = Regex::new(pattern)?;
        re.find(branch)
            .map(|m| m.as_str().to_string())
            .context(format!("no Jira key matching {pattern} in branch {branch}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_key_in_branch_success() {
        let key = JiraClient::find_key_in_branch("CLOUDP-1234-fix-bug", r"CLOUDP-\d+").unwrap();
        assert_eq!(key, "CLOUDP-1234");
    }

    #[test]
    fn find_key_in_branch_no_match() {
        let result = JiraClient::find_key_in_branch("feat/no-ticket", r"CLOUDP-\d+");
        assert!(result.is_err());
    }

    #[test]
    fn find_key_in_branch_with_prefix() {
        let key = JiraClient::find_key_in_branch("user/CLOUDP-9999-feature", r"CLOUDP-\d+").unwrap();
        assert_eq!(key, "CLOUDP-9999");
    }
}
