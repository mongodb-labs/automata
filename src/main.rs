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

use axum::{
    response::Redirect,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

use app_state::AppState;

fn init_tracer(endpoint: &str) -> anyhow::Result<opentelemetry_sdk::trace::TracerProvider> {
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_endpoint(endpoint)
        .build_span_exporter()?;

    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_simple_exporter(exporter)
        .with_config(opentelemetry_sdk::trace::Config::default().with_resource(
            opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                "service.name",
                "automata",
            )]),
        ))
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(provider)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let tracer_provider: Option<opentelemetry_sdk::trace::TracerProvider> =
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .and_then(|endpoint| {
                init_tracer(&endpoint)
                    .map_err(|e| eprintln!("failed to init OTel tracer: {e}"))
                    .ok()
            });

    let otel_layer = tracer_provider.as_ref().map(|p| {
        use opentelemetry::trace::TracerProvider as _;
        tracing_opentelemetry::layer().with_tracer(p.tracer("automata"))
    });

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .with(otel_layer)
        .init();

    let config = config::Config::from_env()?;
    info!("starting automata");

    let automations_dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let automations = engine::load_automations(&automations_dir)?;
    info!(count = automations.len(), "loaded automations");

    let state = AppState {
        config,
        automations: Arc::new(automations),
        http: reqwest::Client::new(),
    };

    let port = state.config.port;
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/doctor") }))
        .route("/health", get(|| async { "ok" }))
        .route("/webhook/github/argo", post(handlers::github::handle))
        .route("/webhook/github/raw", post(handlers::github_raw::handle))
        .route("/doctor", get(handlers::doctor::handle))
        .route(
            "/doctor/install-webhook",
            post(handlers::doctor::install_webhook),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr, "listening");
    axum::serve(listener, app).await?;

    if let Some(provider) = tracer_provider {
        if let Err(e) = provider.shutdown() {
            tracing::warn!(%e, "failed to flush OTel traces on shutdown");
        }
    }

    Ok(())
}
