use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateSuppressionRequest {
    pub query: String,
    pub product_id: String,
}

pub async fn list_suppressions(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().list_suppressions().await {
        Ok(sups) => Json(serde_json::json!({"suppressions": sups})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_suppression(
    State(state): State<AppState>,
    Json(req): Json<CreateSuppressionRequest>,
) -> impl IntoResponse {
    if req.query.is_empty() || req.product_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query and product_id are required"}))).into_response();
    }
    match state.registry.default_engine().create_suppression(req.query, req.product_id).await {
        Ok(sup) => (StatusCode::CREATED, Json(sup)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_suppression(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.registry.default_engine().delete_suppression(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
