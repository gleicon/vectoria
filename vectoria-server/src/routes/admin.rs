use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

pub async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().stats().await {
        Ok(stats) => match serde_json::to_value(stats) {
            Ok(v) => Json(v).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}

pub async fn reindex(State(state): State<AppState>) -> impl IntoResponse {
    let engine = state.registry.default_engine();
    tokio::spawn(async move {
        match engine.reindex_all().await {
            Ok(r) => tracing::info!(reindexed = r.reindexed, errors = r.errors, "reindex complete"),
            Err(e) => tracing::error!(error = %e, "reindex failed"),
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "reindex_started"})))
}
