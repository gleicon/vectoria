use axum::{extract::{Query, State}, http::StatusCode, response::IntoResponse, Json};
use std::collections::HashMap;
use vectoria_core::model::OverrideExport;
use crate::state::AppState;
use super::index_admin::overrides_response;

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

/// Triggers aggregation immediately; normally fires on a ~5m timer after training events.
pub async fn trigger_aggregation(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().trigger_aggregation().await {
        Ok(_) => Json(serde_json::json!({"status": "aggregation_complete"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// GET /admin/overrides — summary of all active Phase 2 overrides.
/// Optional ?q=<query> returns `active_pins`, `active_sponsored`, `active_suppressions`
/// — the subset that currently applies to that query, computed server-side.
pub async fn list_overrides(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    overrides_response(state.registry.default_engine(), params.get("q").map(|s| s.as_str())).await
}

pub async fn export_overrides(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().export_overrides().await {
        Ok(export) => Json(export).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn import_overrides(
    State(state): State<AppState>,
    Json(data): Json<OverrideExport>,
) -> impl IntoResponse {
    match state.registry.default_engine().import_overrides(data).await {
        Ok(report) => Json(serde_json::json!({"imported": report.imported})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
