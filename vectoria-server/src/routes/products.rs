use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use vectoria_core::model::{Product, ProductStatus, SimilarRequest};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct IndexProductRequest {
    pub id: String,
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    pub metadata: serde_json::Value,
}


pub async fn index_product(
    State(state): State<AppState>,
    Json(req): Json<IndexProductRequest>,
) -> impl IntoResponse {
    let now = Utc::now();
    let product = Product {
        id: req.id,
        text: req.text,
        vector: req.vector,
        metadata: req.metadata,
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    };
    match state.engine.index(product).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({"status": "indexed"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn update_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut req): Json<IndexProductRequest>,
) -> impl IntoResponse {
    req.id = id;
    let now = Utc::now();
    let product = Product {
        id: req.id,
        text: req.text,
        vector: req.vector,
        metadata: req.metadata,
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    };
    match state.engine.index(product).await {
        Ok(_) => Json(serde_json::json!({"status": "updated"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.engine.delete(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn similar_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let req = SimilarRequest {
        product_id: Some(id),
        text: None,
        vector: None,
        limit: 10,
        filters: None,
    };
    match state.engine.similar(req).await {
        Ok(hits) => Json(serde_json::json!({"hits": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn similar_flexible(
    State(state): State<AppState>,
    Json(req): Json<SimilarRequest>,
) -> impl IntoResponse {
    match state.engine.similar(req).await {
        Ok(hits) => Json(serde_json::json!({"hits": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
