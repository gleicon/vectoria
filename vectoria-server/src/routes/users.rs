use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct RecommendParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 10 }

pub async fn get_recommendations(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<RecommendParams>,
) -> impl IntoResponse {
    let engine = state.registry.default_engine();
    match engine.recommend(&user_id, params.limit).await {
        Ok(hits) => Json(serde_json::json!({ "hits": hits, "user_id": user_id })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
