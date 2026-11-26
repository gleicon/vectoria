use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;
use vectoria_core::model::SearchRequest;
use crate::state::AppState;

pub async fn search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    match state.registry.default_engine().search(req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("rerank requested but not enabled") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(serde_json::json!({"error": msg}))).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct AutocompleteQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 10 }

/// Retrieve a stored query trace by its UUID.
/// Only available when `enable_trace = true` in `[index]` config.
/// Traces are retained for the last 1000 queries that had `explain: true`.
pub async fn explain_trace(
    State(state): State<AppState>,
    Path(trace_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(ts) = &state.trace_store else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "trace store not enabled; set enable_trace = true in [index]"})),
        ).into_response();
    };
    match ts.get(&trace_id) {
        Some(trace) => Json(trace).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "trace not found", "trace_id": trace_id.to_string()})),
        ).into_response(),
    }
}

pub async fn autocomplete(
    State(state): State<AppState>,
    Query(params): Query<AutocompleteQuery>,
) -> impl IntoResponse {
    let suggestions = state.registry.default_engine().autocomplete(&params.q, params.limit);
    Json(serde_json::json!({
        "query": params.q,
        "suggestions": suggestions,
    }))
}
