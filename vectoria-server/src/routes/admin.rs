use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

pub async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    match state.engine.stats().await {
        Ok(stats) => Json(serde_json::to_value(stats).unwrap()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}

pub async fn reindex(State(state): State<AppState>) -> impl IntoResponse {
    tokio::spawn(async move {
        match state.engine.reindex_all().await {
            Ok(r) => tracing::info!(reindexed = r.reindexed, errors = r.errors, "reindex complete"),
            Err(e) => tracing::error!(error = %e, "reindex failed"),
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "reindex_started"})))
}

/// Multi-index endpoints — each indexName routes to an isolated SearchEngine
/// via IndexRegistry.
pub mod indexes {
    use axum::{
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    };
    use serde::Deserialize;
    use std::time::Instant;
    use vectoria_core::model::{Product, ProductStatus, SearchMode, SearchRequest};
    use chrono::Utc;
    use crate::state::AppState;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct IndexSearchRequest {
        pub query: Option<String>,
        pub hits_per_page: Option<usize>,
        pub page: Option<usize>,
        #[allow(dead_code)]
        pub filters: Option<String>,
    }

    pub async fn search(
        State(state): State<AppState>,
        Path(index_name): Path<String>,
        Json(req): Json<IndexSearchRequest>,
    ) -> impl IntoResponse {
        let engine = match state.index_registry.get_or_create(&index_name) {
            Ok(e) => e,
            Err(err) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": err.to_string()})),
            ).into_response(),
        };

        let q = req.query.unwrap_or_default();
        let limit = req.hits_per_page.unwrap_or(20);
        let page = req.page.unwrap_or(0);
        let offset = page * limit;
        let start = Instant::now();

        match engine.search(SearchRequest {
            q: q.clone(), limit, offset,
            mode: SearchMode::Hybrid,
            filters: None, ranking_weights: None, aggregate: None,
            explain: false, rerank: false,
        }).await {
            Ok(resp) => {
                let processing_time = start.elapsed().as_millis() as u64;
                let nb_pages = if limit > 0 { (resp.total + limit - 1) / limit } else { 1 };
                Json(serde_json::json!({
                    "hits": resp.hits,
                    "page": page,
                    "nbHits": resp.total,
                    "nbPages": nb_pages,
                    "hitsPerPage": limit,
                    "processingTimeMS": processing_time,
                    "query": q,
                    "params": "",
                })).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": e.to_string()})),
            ).into_response(),
        }
    }

    pub async fn index_object(
        State(state): State<AppState>,
        Path(index_name): Path<String>,
        Json(body): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let engine = match state.index_registry.get_or_create(&index_name) {
            Ok(e) => e,
            Err(err) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": err.to_string()})),
            ).into_response(),
        };

        let id = body.get("objectID").or_else(|| body.get("id"))
            .and_then(|v| v.as_str()).map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let now = Utc::now();
        let product = Product {
            id: id.clone(),
            text: body.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
            vector: None,
            metadata: body,
            model_id: None,
            dims: None,
            status: ProductStatus::PendingVector,
            created_at: now,
            updated_at: now,
        };
        match engine.index(product).await {
            Ok(_) => (
                StatusCode::CREATED,
                Json(serde_json::json!({"objectID": id, "taskID": uuid::Uuid::new_v4().to_string()})),
            ).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": e.to_string()})),
            ).into_response(),
        }
    }

    pub async fn update_object(
        State(state): State<AppState>,
        Path((index_name, object_id)): Path<(String, String)>,
        Json(mut body): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let engine = match state.index_registry.get_or_create(&index_name) {
            Ok(e) => e,
            Err(err) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": err.to_string()})),
            ).into_response(),
        };

        if let Some(obj) = body.as_object_mut() {
            obj.insert("objectID".to_string(), serde_json::Value::String(object_id.clone()));
        }
        let now = Utc::now();
        let product = Product {
            id: object_id.clone(),
            text: body.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
            vector: None,
            metadata: body,
            model_id: None,
            dims: None,
            status: ProductStatus::PendingVector,
            created_at: now,
            updated_at: now,
        };
        match engine.index(product).await {
            Ok(_) => Json(serde_json::json!({"objectID": object_id, "taskID": uuid::Uuid::new_v4().to_string()})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": e.to_string()})),
            ).into_response(),
        }
    }

    pub async fn delete_object(
        State(state): State<AppState>,
        Path((index_name, object_id)): Path<(String, String)>,
    ) -> impl IntoResponse {
        let engine = match state.index_registry.get_or_create(&index_name) {
            Ok(e) => e,
            Err(err) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": err.to_string()})),
            ).into_response(),
        };

        match engine.delete(&object_id).await {
            Ok(_) => Json(serde_json::json!({"objectID": object_id, "taskID": uuid::Uuid::new_v4().to_string()})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": e.to_string()})),
            ).into_response(),
        }
    }

    pub async fn batch(
        State(state): State<AppState>,
        Path(index_name): Path<String>,
        Json(payload): Json<BatchRequest>,
    ) -> impl IntoResponse {
        let engine = match state.index_registry.get_or_create(&index_name) {
            Ok(e) => e,
            Err(err) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": err.to_string()})),
            ).into_response(),
        };

        let mut indexed = 0usize;
        let mut deleted = 0usize;
        let mut errors = 0usize;

        for op in payload.requests {
            match op.action.as_str() {
                "addObject" | "updateObject" => {
                    let body = op.body;
                    let id = body.get("objectID").or_else(|| body.get("id"))
                        .and_then(|v| v.as_str()).map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    let now = Utc::now();
                    let product = Product {
                        id,
                        text: body.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        vector: None,
                        metadata: body,
                        model_id: None,
                        dims: None,
                        status: ProductStatus::PendingVector,
                        created_at: now,
                        updated_at: now,
                    };
                    match engine.index(product).await {
                        Ok(_) => indexed += 1,
                        Err(_) => errors += 1,
                    }
                }
                "deleteObject" => {
                    if let Some(id) = op.body.get("objectID").and_then(|v| v.as_str()) {
                        match engine.delete(id).await {
                            Ok(_) => deleted += 1,
                            Err(_) => errors += 1,
                        }
                    }
                }
                _ => errors += 1,
            }
        }

        Json(serde_json::json!({
            "taskID": uuid::Uuid::new_v4().to_string(),
            "indexed": indexed,
            "deleted": deleted,
            "errors": errors,
        })).into_response()
    }

    #[derive(Deserialize)]
    pub struct BatchRequest {
        pub requests: Vec<BatchOp>,
    }

    #[derive(Deserialize)]
    pub struct BatchOp {
        pub action: String,
        pub body: serde_json::Value,
    }
}
