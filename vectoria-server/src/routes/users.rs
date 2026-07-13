use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::{auth::Principal, state::AppState};

#[derive(Deserialize)]
pub struct RecommendParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 10 }

pub async fn get_recommendations(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(user_id): Path<String>,
    Query(params): Query<RecommendParams>,
) -> impl IntoResponse {
    // Admin uses the default engine; tenants are scoped to their named index.
    let engine = match &principal {
        Principal::Admin => state.registry.default_engine(),
        Principal::Tenant(name) => match state.registry.get(name) {
            Some(e) => e,
            None => return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant index not found"})),
            ).into_response(),
        },
    };
    match engine.recommend(&user_id, params.limit).await {
        Ok(hits) => Json(serde_json::json!({ "hits": hits, "user_id": user_id })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}
