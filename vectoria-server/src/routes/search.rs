use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
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
