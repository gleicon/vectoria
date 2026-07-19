use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreatePinRequest {
    pub query: String,
    pub product_id: String,
    pub position: usize,
}

pub async fn list_pins(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().list_pins().await {
        Ok(pins) => Json(serde_json::json!({"pins": pins})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_pin(
    State(state): State<AppState>,
    Json(req): Json<CreatePinRequest>,
) -> impl IntoResponse {
    if req.query.is_empty() || req.product_id.is_empty() || req.position == 0 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query, product_id, and position (≥1) are required"}))).into_response();
    }
    match state.registry.default_engine().create_pin(req.query, req.product_id, req.position).await {
        Ok(pin) => (StatusCode::CREATED, Json(pin)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_pin(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.registry.default_engine().delete_pin(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
