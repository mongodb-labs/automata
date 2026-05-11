mod config;
mod context;
mod engine;
mod expr;
mod functions;
mod github;
mod handlers;
mod jira;
mod types;

use axum::{routing::{get, post}, Router};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().json().init();

    let config = config::Config::from_env()?;
    info!("starting automata");

    let automations = engine::load_automations("automations/")?;
    info!(count = automations.len(), "loaded automations");

    let port = config.port;
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/webhook/github", post(handlers::github::handle))
        .route("/doctor", get(handlers::doctor::handle))
        .with_state(config);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}
