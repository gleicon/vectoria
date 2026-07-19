/// Per-index admin routes: `/indexes/{name}/admin/*`
///
/// Admins can target any index. Tenants can only target their own index namespace.
use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::{collections::HashMap, sync::Arc};
use vectoria_core::{
    model::OverrideExport,
    search::SearchEngine,
};
use crate::{auth::Principal, state::AppState};
use super::pins::CreatePinRequest;
use super::sponsored::CreateSponsoredRequest;
use super::suppressions::CreateSuppressionRequest;

// ── Engine resolution ─────────────────────────────────────────────────────────

fn resolve(state: &AppState, principal: &Principal, name: &str) -> Result<Arc<SearchEngine>, Response> {
    let key = match principal {
        Principal::Admin => name.to_string(),
        Principal::Tenant(t) => format!("{t}/{name}"),
    };
    state.registry.get(&key).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "index not found"}))).into_response()
    })
}

// ── Pins ──────────────────────────────────────────────────────────────────────

pub async fn list_pins(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.list_pins().await {
        Ok(pins) => Json(serde_json::json!({"pins": pins})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_pin(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(req): Json<CreatePinRequest>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    if req.query.is_empty() || req.product_id.is_empty() || req.position == 0 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query, product_id and position (≥1) required"}))).into_response();
    }
    match engine.create_pin(req.query, req.product_id, req.position).await {
        Ok(pin) => (StatusCode::CREATED, Json(pin)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_pin(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((name, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.delete_pin(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// ── Sponsored ─────────────────────────────────────────────────────────────────

pub async fn list_sponsored(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.list_sponsored().await {
        Ok(s) => Json(serde_json::json!({"sponsored": s})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_sponsored(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(req): Json<CreateSponsoredRequest>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    if req.query_pattern.is_empty() || req.product_id.is_empty() || req.position == 0 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query_pattern, product_id and position (≥1) required"}))).into_response();
    }
    match engine.create_sponsored(req.query_pattern, req.product_id, req.position, req.label, req.start_at, req.end_at).await {
        Ok(slot) => (StatusCode::CREATED, Json(slot)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_sponsored(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((name, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.delete_sponsored(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// ── Suppressions ──────────────────────────────────────────────────────────────

pub async fn list_suppressions(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.list_suppressions().await {
        Ok(s) => Json(serde_json::json!({"suppressions": s})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn create_suppression(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(req): Json<CreateSuppressionRequest>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    if req.query.is_empty() || req.product_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query and product_id required"}))).into_response();
    }
    match engine.create_suppression(req.query, req.product_id).await {
        Ok(sup) => (StatusCode::CREATED, Json(sup)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_suppression(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((name, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.delete_suppression(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// ── Overrides summary ─────────────────────────────────────────────────────────

pub(super) async fn overrides_response(engine: Arc<SearchEngine>, query: Option<&str>) -> Response {
    let (pins, sponsored, suppressions) = tokio::join!(
        engine.list_pins(),
        engine.list_sponsored(),
        engine.list_suppressions(),
    );
    match (pins, sponsored, suppressions) {
        (Ok(pins), Ok(sponsored), Ok(suppressions)) => {
            let tainted = !pins.is_empty() || !sponsored.is_empty() || !suppressions.is_empty();
            let mut resp = serde_json::json!({
                "tainted": tainted,
                "pin_count": pins.len(),
                "sponsored_count": sponsored.len(),
                "suppression_count": suppressions.len(),
                "pins": pins,
                "sponsored": sponsored,
                "suppressions": suppressions,
            });
            if let Some(q) = query {
                if let Ok((ap, asp, asup)) = engine.active_overrides_for_query(q).await {
                    resp["active_pins"] = serde_json::json!(ap);
                    resp["active_sponsored"] = serde_json::json!(asp);
                    resp["active_suppressions"] = serde_json::json!(asup);
                }
            }
            Json(resp).into_response()
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "failed to load overrides"}))).into_response(),
    }
}

pub async fn list_overrides(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    overrides_response(engine, params.get("q").map(|s| s.as_str())).await
}

// ── Stats ─────────────────────────────────────────────────────────────────────

pub async fn stats(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.stats().await {
        Ok(s) => match serde_json::to_value(s) {
            Ok(v) => Json(v).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// ── Reindex ───────────────────────────────────────────────────────────────────

pub async fn reindex(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    tokio::spawn(async move {
        match engine.reindex_all().await {
            Ok(r) => tracing::info!(index = %name, reindexed = r.reindexed, errors = r.errors, "reindex complete"),
            Err(e) => tracing::error!(index = %name, error = %e, "reindex failed"),
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "reindex_started"}))).into_response()
}

// ── Aggregation ───────────────────────────────────────────────────────────────

pub async fn trigger_aggregation(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.trigger_aggregation().await {
        Ok(_) => Json(serde_json::json!({"status": "aggregation_complete"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// ── Export / Import ───────────────────────────────────────────────────────────

pub async fn export_overrides(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.export_overrides().await {
        Ok(export) => Json(export).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn import_overrides(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(data): Json<OverrideExport>,
) -> impl IntoResponse {
    let engine = match resolve(&state, &principal, &name) { Ok(e) => e, Err(r) => return r };
    match engine.import_overrides(data).await {
        Ok(report) => Json(serde_json::json!({"imported": report.imported})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
