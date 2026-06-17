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

fn init_tracer() -> anyhow::Result<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;

    // Let the exporter resolve OTEL_EXPORTER_OTLP_ENDPOINT itself so the spec
    // /v1/traces suffix (and other OTEL_* settings) are applied. As of 0.32 a
    // programmatic with_endpoint() takes precedence over the env var and is used
    // verbatim with no path appended, so passing it by hand would POST to the
    // bare host instead of the collector's /v1/traces route.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()?;

    // The default BatchSpanProcessor drives the exporter with block_on on a
    // dedicated thread, which can't run an async HTTP client like reqwest-client.
    // Use the tokio-runtime processor so spans export on our existing runtime.
    let processor =
        BatchSpanProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio).build();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(processor)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("automata")
                .build(),
        )
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(provider)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider> =
        if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
            init_tracer()
                .map_err(|e| eprintln!("failed to init OTel tracer: {e}"))
                .ok()
        } else {
            None
        };

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
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    github.event = tracing::field::Empty,
                    github.delivery_id = tracing::field::Empty,
                )
            }),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{Tracer, TracerProvider as _};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Emits a span and flushes it through the OTLP HTTP exporter to a mock
    // collector. Guards two regressions from the 0.32 bump:
    //   1. the async reqwest-client must be driven by the async-runtime
    //      BatchSpanProcessor (the default thread-based one cannot run it), and
    //   2. the base OTEL_EXPORTER_OTLP_ENDPOINT must get the spec /v1/traces
    //      suffix appended, which only happens when the crate resolves the env
    //      var itself rather than us passing it programmatically.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn init_tracer_exports_spans_to_otlp_traces_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/traces"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1..)
            .mount(&server)
            .await;

        // Single-threaded mutation before the exporter reads the var; no other
        // test touches OTEL_EXPORTER_OTLP_ENDPOINT.
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", server.uri());

        let provider = init_tracer().expect("tracer init should succeed");
        provider.tracer("test").in_span("unit-test-span", |_| {});

        // shutdown() blocks while flushing; run it off the runtime worker so the
        // async export task can make progress.
        let p = provider.clone();
        tokio::task::spawn_blocking(move || p.shutdown())
            .await
            .unwrap()
            .expect("shutdown should flush spans to the collector");

        let requests = server.received_requests().await.unwrap();
        assert!(
            requests.iter().any(|r| r.url.path() == "/v1/traces"),
            "expected POST to /v1/traces, got: {:?}",
            requests
                .iter()
                .map(|r| r.url.path().to_string())
                .collect::<Vec<_>>()
        );
    }
}
