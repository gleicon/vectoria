use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use vectoria_core::model::{Product, SimilarRequest};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct IndexProductRequest {
    pub id: String,
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    pub metadata: serde_json::Value,
}

fn product_from_request(id: String, req: IndexProductRequest) -> Product {
    let mut p = Product::new(id, req.metadata);
    p.text = req.text;
    p.vector = req.vector;
    p
}

pub async fn index_product(
    State(state): State<AppState>,
    Json(req): Json<IndexProductRequest>,
) -> impl IntoResponse {
    let product = product_from_request(req.id.clone(), req);
    match state.engine.index(product).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({"status": "indexed"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn update_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<IndexProductRequest>,
) -> impl IntoResponse {
    let product = product_from_request(id, req);
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
        Ok(_) => Json(serde_json::json!({"status": "deleted"})).into_response(),
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
