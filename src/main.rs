mod app_state;
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
use std::sync::Arc;
use tracing::info;

use app_state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().json().init();

    let config = config::Config::from_env()?;
    info!("starting automata");

    let automations = engine::load_automations("automations/")?;
    info!(count = automations.len(), "loaded automations");

    let state = AppState {
        config,
        automations: Arc::new(automations),
        http: reqwest::Client::new(),
    };

    let port = state.config.port;
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/webhook/github", post(handlers::github::handle))
        .route("/doctor", get(handlers::doctor::handle))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}
