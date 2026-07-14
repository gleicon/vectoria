use axum::{
    extract::{Path, Query, State},
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
    let engine = state.registry.default_engine();
    match engine.index(product).await {
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
    let engine = state.registry.default_engine();
    match engine.index(product).await {
        Ok(_) => Json(serde_json::json!({"status": "updated"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let engine = state.registry.default_engine();
    match engine.delete(&id).await {
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
    let engine = state.registry.default_engine();
    match engine.similar(req).await {
        Ok(hits) => Json(serde_json::json!({"hits": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn similar_flexible(
    State(state): State<AppState>,
    Json(req): Json<SimilarRequest>,
) -> impl IntoResponse {
    let engine = state.registry.default_engine();
    match engine.similar(req).await {
        Ok(hits) => Json(serde_json::json!({"hits": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct RelatedParams {
    #[serde(rename = "type")]
    pub rel_type: Option<String>,
    #[serde(default = "default_related_limit")]
    pub limit: usize,
}

fn default_related_limit() -> usize { 10 }

pub async fn related_products(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<RelatedParams>,
) -> impl IntoResponse {
    let engine = state.registry.default_engine();
    let rel_type = params.rel_type.as_deref();
    match engine.related_products(&id, rel_type, params.limit).await {
        Ok(hits) => Json(serde_json::json!({"product_id": id, "related": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
