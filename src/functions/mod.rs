pub mod builtin;
pub mod github;
pub mod jira;

use crate::context::ExecutionContext;
use crate::github::api::GitHubClient;
use crate::jira::JiraClient;
use crate::types::Step;
use serde_json::Value;
use tracing::info;

pub struct Clients {
    pub github: GitHubClient,
    pub jira: JiraClient,
    pub http: reqwest::Client,
}

pub fn execute_step<'a>(
    step: &'a Step,
    ctx: &'a mut ExecutionContext,
    clients: &'a Clients,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        info!(func = step.func, "executing step");
        let outputs = dispatch(&step.func, &step.inputs, ctx, clients).await?;

        if let Some(id) = &step.id {
            ctx.outputs.insert(id.clone(), outputs);
        }
        Ok(())
    })
}

async fn dispatch(
    func: &str,
    inputs: &std::collections::HashMap<String, serde_yaml::Value>,
    ctx: &mut ExecutionContext,
    clients: &Clients,
) -> anyhow::Result<Value> {
    match func {
        "builtin.jq"               => builtin::jq(inputs, ctx).await,
        "builtin.lookup"           => builtin::lookup(inputs, ctx).await,
        "jira.create_issue"        => jira::create_issue(&clients.jira, inputs, ctx).await,
        "jira.transition"          => jira::transition(&clients.jira, inputs, ctx).await,
        "github.post_comment"      => github::post_comment(&clients.github, inputs, ctx).await,
        "github.add_label"         => github::add_label(&clients.github, inputs, ctx).await,
        "github.remove_label"      => github::remove_label(&clients.github, inputs, ctx).await,
        "github.approve_pr"        => github::approve_pr(&clients.github, inputs, ctx).await,
        "github.enable_auto_merge" => github::enable_auto_merge(&clients.github, inputs, ctx).await,
        "github.list_pr_comments"  => github::list_pr_comments(&clients.github, inputs, ctx).await,
        "github.get_commit"        => github::get_commit(&clients.github, inputs, ctx).await,
        _ => anyhow::bail!("unknown function: {func}"),
    }
}
