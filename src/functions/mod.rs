pub mod github;
pub mod jira;
pub mod named;

use crate::context::ExecutionContext;
use crate::engine::eval_if;
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

/// Execute a single step, updating ctx.outputs if the step has an id.
pub fn execute_step<'a>(
    step: &'a Step,
    ctx: &'a mut ExecutionContext,
    clients: &'a Clients,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        // Evaluate if: condition
        if let Some(cond) = &step.if_cond {
            if !eval_if(cond, &ctx.payload) {
                info!(cond, "step skipped");
                return Ok(());
            }
        }

        let outputs = if let Some(func) = &step.func {
            info!(func, "executing step");
            dispatch(func, &step.inputs, ctx, clients).await?
        } else if let Some(uses) = &step.uses {
            info!(uses, "expanding named function");
            named::run(uses, &step.inputs, ctx, clients).await?
        } else {
            anyhow::bail!("step has neither func nor uses");
        };

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
        "jira.create_issue" => jira::create_issue(&clients.jira, inputs, ctx).await,
        "jira.transition"   => jira::transition(&clients.jira, inputs, ctx).await,
        "jira.find_key"     => jira::find_key(&clients.jira, &clients.http, inputs, ctx).await,
        "github.post_comment"      => github::post_comment(&clients.github, inputs, ctx).await,
        "github.add_label"         => github::add_label(&clients.github, inputs, ctx).await,
        "github.approve_pr"        => github::approve_pr(&clients.github, inputs, ctx).await,
        "github.enable_auto_merge" => github::enable_auto_merge(&clients.github, inputs, ctx).await,
        _ => anyhow::bail!("unknown function: {func}"),
    }
}
