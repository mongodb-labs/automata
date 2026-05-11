use axum::{extract::State, http::StatusCode, response::IntoResponse};

use crate::app_state::AppState;

pub async fn handle(State(_state): State<AppState>) -> impl IntoResponse {
    StatusCode::OK
}
