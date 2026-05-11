use axum::{extract::State, http::StatusCode, response::IntoResponse};

use crate::config::Config;

pub async fn handle(State(_config): State<Config>) -> impl IntoResponse {
    StatusCode::OK
}
