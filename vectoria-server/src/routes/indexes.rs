use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use vectoria_core::{model::{Product, SearchRequest, SimilarRequest}, SearchEngine};
use crate::{index_registry::CreateIndexError, routes::products::IndexProductRequest, state::AppState};

#[derive(Deserialize)]
pub struct CreateIndexRequest {
    pub name: String,
}

pub async fn list_indexes(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({"indexes": state.registry.list()}))
}

fn validate_index_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("index name must be 1–64 characters");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("index name may only contain letters, digits, hyphens, and underscores");
    }
    Ok(())
}

pub async fn create_index(
    State(state): State<AppState>,
    Json(req): Json<CreateIndexRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_index_name(&req.name) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response();
    }
    match state.registry.create(&req.name).await {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({"name": req.name, "status": "created"}))).into_response(),
        Err(CreateIndexError::AlreadyExists) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": "index already exists"}))).into_response(),
        Err(CreateIndexError::LimitReached) => (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({"error": "index limit reached (max 100 named indexes)"}))).into_response(),
        Err(CreateIndexError::BuildFailed) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "failed to build index"}))).into_response(),
    }
}

pub async fn delete_index(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.registry.delete(&name) {
        Ok(true) => Json(serde_json::json!({"status": "deleted"})).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "index not found"}))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

fn resolve_index(state: &AppState, name: &str) -> Result<Arc<SearchEngine>, Response> {
    state.registry.get(name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "index not found"}))).into_response()
    })
}

pub async fn index_product(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<IndexProductRequest>,
) -> impl IntoResponse {
    let engine = match resolve_index(&state, &name) {
        Ok(e) => e,
        Err(r) => return r,
    };
    let mut p = Product::new(req.id, req.metadata);
    p.text = req.text;
    p.vector = req.vector;
    match engine.index(p).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({"status": "indexed"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn search(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let engine = match resolve_index(&state, &name) {
        Ok(e) => e,
        Err(r) => return r,
    };
    match engine.search(req).await {
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

pub async fn similar(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SimilarRequest>,
) -> impl IntoResponse {
    let engine = match resolve_index(&state, &name) {
        Ok(e) => e,
        Err(r) => return r,
    };
    match engine.similar(req).await {
        Ok(hits) => Json(serde_json::json!({"hits": hits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
