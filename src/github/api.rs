use anyhow::Context as _;
use serde_json::{json, Value};

pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
    base_url: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: "https://api.github.com".to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_base(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url,
        }
    }

    fn base(&self, owner: &str, repo: &str) -> String {
        format!("{}/repos/{owner}/{repo}", self.base_url)
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            "Authorization",
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        h.insert("Accept", "application/vnd.github+json".parse().unwrap());
        h.insert("X-GitHub-Api-Version", "2022-11-28".parse().unwrap());
        h.insert("User-Agent", "automata/1.0".parse().unwrap());
        h
    }

    pub async fn post_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        body: &str,
    ) -> anyhow::Result<u64> {
        let resp: Value = self
            .client
            .post(format!(
                "{}/issues/{}/comments",
                self.base(owner, repo),
                issue_number
            ))
            .headers(self.headers())
            .json(&json!({"body": body}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp["id"].as_u64().context("missing comment id")
    }

    pub async fn add_label(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        label: &str,
    ) -> anyhow::Result<()> {
        self.client
            .post(format!(
                "{}/issues/{}/labels",
                self.base(owner, repo),
                issue_number
            ))
            .headers(self.headers())
            .json(&json!({"labels": [label]}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn approve_pr(&self, owner: &str, repo: &str, pr_number: u64) -> anyhow::Result<u64> {
        let resp: Value = self
            .client
            .post(format!(
                "{}/pulls/{}/reviews",
                self.base(owner, repo),
                pr_number
            ))
            .headers(self.headers())
            .json(&json!({"event": "APPROVE"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp["id"].as_u64().context("missing review id")
    }

    pub async fn enable_auto_merge(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        strategy: &str,
    ) -> anyhow::Result<()> {
        let merge_method = match strategy {
            "squash" => "SQUASH",
            "merge" => "MERGE",
            "rebase" => "REBASE",
            _ => "SQUASH",
        };
        // Uses GraphQL since REST doesn't support auto-merge
        let query = format!(
            r#"mutation {{ enablePullRequestAutoMerge(input: {{ pullRequestId: "{}", mergeMethod: {} }}) {{ clientMutationId }} }}"#,
            self.pr_node_id(owner, repo, pr_number).await?,
            merge_method
        );
        self.client
            .post(format!("{}/graphql", self.base_url))
            .headers(self.headers())
            .json(&json!({"query": query}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn pr_node_id(&self, owner: &str, repo: &str, pr_number: u64) -> anyhow::Result<String> {
        let resp: Value = self
            .client
            .get(format!("{}/pulls/{}", self.base(owner, repo), pr_number))
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp["node_id"]
            .as_str()
            .map(|s| s.to_string())
            .context("missing node_id")
    }

    pub async fn remove_label(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        label: &str,
    ) -> anyhow::Result<()> {
        self.client
            .delete(format!(
                "{}/issues/{}/labels/{}",
                self.base(owner, repo),
                issue_number,
                label
            ))
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_commit(&self, owner: &str, repo: &str, sha: &str) -> anyhow::Result<Value> {
        let resp: Value = self
            .client
            .get(format!("{}/commits/{}", self.base(owner, repo), sha))
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }

    /// Fetch all comments on an issue/PR and return the full comment objects.
    pub async fn list_comments(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
    ) -> anyhow::Result<Vec<Value>> {
        let resp: Vec<Value> = self
            .client
            .get(format!(
                "{}/issues/{}/comments",
                self.base(owner, repo),
                issue_number
            ))
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client(server: &MockServer) -> GitHubClient {
        GitHubClient::new_with_base("token".to_string(), server.uri())
    }

    #[tokio::test]
    async fn post_comment_returns_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 99})))
            .mount(&server)
            .await;
        let id = client(&server)
            .await
            .post_comment("owner", "repo", 42, "hello")
            .await
            .unwrap();
        assert_eq!(id, 99);
    }

    #[tokio::test]
    async fn post_comment_propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/issues/1/comments"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let result = client(&server)
            .await
            .post_comment("owner", "repo", 1, "x")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_label_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/issues/1/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        client(&server)
            .await
            .add_label("owner", "repo", 1, "bug")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn remove_label_ok() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/repos/owner/repo/issues/5/labels/auto_close"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        client(&server)
            .await
            .remove_label("owner", "repo", 5, "auto_close")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn approve_pr_returns_review_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repo/pulls/7/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 42})))
            .mount(&server)
            .await;
        let id = client(&server)
            .await
            .approve_pr("owner", "repo", 7)
            .await
            .unwrap();
        assert_eq!(id, 42);
    }

    #[tokio::test]
    async fn get_commit_returns_json() {
        let server = MockServer::start().await;
        let commit = json!({"sha": "abc123", "commit": {"message": "fix bug"}});
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/commits/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(commit))
            .mount(&server)
            .await;
        let result = client(&server)
            .await
            .get_commit("owner", "repo", "abc123")
            .await
            .unwrap();
        assert_eq!(result["sha"], "abc123");
    }

    #[tokio::test]
    async fn list_comments_returns_vec() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/issues/3/comments"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{"id": 1, "body": "hello"}, {"id": 2, "body": "world"}])),
            )
            .mount(&server)
            .await;
        let comments = client(&server)
            .await
            .list_comments("owner", "repo", 3)
            .await
            .unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0]["body"], "hello");
    }
}
