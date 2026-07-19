use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateSponsoredRequest {
    pub query_pattern: String,
    pub product_id: String,
    pub position: usize,
    #[serde(default = "default_label")]
    pub label: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

fn default_label() -> String { "Sponsored".to_string() }

pub async fn list_sponsored(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.default_engine().list_sponsored().await {
        Ok(slots) => Json(serde_json::json!({"sponsored": slots})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_sponsored(
    State(state): State<AppState>,
    Json(req): Json<CreateSponsoredRequest>,
) -> impl IntoResponse {
    if req.query_pattern.is_empty() || req.product_id.is_empty() || req.position == 0 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query_pattern, product_id, and position (≥1) are required"}))).into_response();
    }
    match state.registry.default_engine()
        .create_sponsored(req.query_pattern, req.product_id, req.position, req.label, req.start_at, req.end_at)
        .await
    {
        Ok(slot) => (StatusCode::CREATED, Json(slot)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_sponsored(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.registry.default_engine().delete_sponsored(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
