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
    match state.engine.search(req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
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
    let req = SearchRequest {
        q: params.q.clone(),
        limit: params.limit,
        offset: 0,
        mode: vectoria_core::model::SearchMode::Bm25,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    };
    match state.engine.search(req).await {
        Ok(resp) => Json(serde_json::json!({
            "query": params.q,
            "hits": resp.hits,
            "processing_time_ms": resp.processing_time_ms,
        })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}
