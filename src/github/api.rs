use anyhow::Context as _;
use serde_json::{json, Value};

pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
    owner: String,
    repo: String,
}

impl GitHubClient {
    pub fn new(token: String, owner: &str, repo: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            owner: owner.to_string(),
            repo: repo.to_string(),
        }
    }

    fn base(&self) -> String {
        format!("https://api.github.com/repos/{}/{}", self.owner, self.repo)
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("Authorization", format!("Bearer {}", self.token).parse().unwrap());
        h.insert("Accept", "application/vnd.github+json".parse().unwrap());
        h.insert("X-GitHub-Api-Version", "2022-11-28".parse().unwrap());
        h.insert("User-Agent", "automata/1.0".parse().unwrap());
        h
    }

    pub async fn post_comment(&self, issue_number: u64, body: &str) -> anyhow::Result<u64> {
        let resp: Value = self.client
            .post(format!("{}/issues/{}/comments", self.base(), issue_number))
            .headers(self.headers())
            .json(&json!({"body": body}))
            .send().await?
            .error_for_status()?
            .json().await?;
        resp["id"].as_u64().context("missing comment id")
    }

    pub async fn add_label(&self, issue_number: u64, label: &str) -> anyhow::Result<()> {
        self.client
            .post(format!("{}/issues/{}/labels", self.base(), issue_number))
            .headers(self.headers())
            .json(&json!({"labels": [label]}))
            .send().await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn approve_pr(&self, pr_number: u64) -> anyhow::Result<u64> {
        let resp: Value = self.client
            .post(format!("{}/pulls/{}/reviews", self.base(), pr_number))
            .headers(self.headers())
            .json(&json!({"event": "APPROVE"}))
            .send().await?
            .error_for_status()?
            .json().await?;
        resp["id"].as_u64().context("missing review id")
    }

    pub async fn enable_auto_merge(&self, pr_number: u64, strategy: &str) -> anyhow::Result<()> {
        let merge_method = match strategy {
            "squash" => "SQUASH",
            "merge" => "MERGE",
            "rebase" => "REBASE",
            _ => "SQUASH",
        };
        // Uses GraphQL since REST doesn't support auto-merge
        let query = format!(
            r#"mutation {{ enablePullRequestAutoMerge(input: {{ pullRequestId: "{}", mergeMethod: {} }}) {{ clientMutationId }} }}"#,
            self.pr_node_id(pr_number).await?,
            merge_method
        );
        self.client
            .post("https://api.github.com/graphql")
            .headers(self.headers())
            .json(&json!({"query": query}))
            .send().await?
            .error_for_status()?;
        Ok(())
    }

    async fn pr_node_id(&self, pr_number: u64) -> anyhow::Result<String> {
        let resp: Value = self.client
            .get(format!("{}/pulls/{}", self.base(), pr_number))
            .headers(self.headers())
            .send().await?
            .error_for_status()?
            .json().await?;
        resp["node_id"].as_str().map(|s| s.to_string()).context("missing node_id")
    }

    /// Fetch all comments on an issue/PR and return the body text of each.
    pub async fn list_comments(&self, issue_number: u64) -> anyhow::Result<Vec<String>> {
        let resp: Vec<Value> = self.client
            .get(format!("{}/issues/{}/comments", self.base(), issue_number))
            .headers(self.headers())
            .send().await?
            .error_for_status()?
            .json().await?;
        Ok(resp.iter()
            .filter_map(|c| c["body"].as_str().map(|s| s.to_string()))
            .collect())
    }
}
