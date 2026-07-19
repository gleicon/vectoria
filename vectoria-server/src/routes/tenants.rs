use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
}

fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("name must be 1–64 characters");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("name may only contain letters, digits, hyphens, and underscores");
    }
    Ok(())
}

/// GET /admin/tenants — list all tenants with keys (admin-only endpoint).
pub async fn list_tenants(State(state): State<AppState>) -> impl IntoResponse {
    let tenants: Vec<_> = state.tenant_store.list().into_iter()
        .map(|t| serde_json::json!({
            "name": t.name,
            "api_key": t.api_key,
            "created_at": t.created_at,
        }))
        .collect();
    Json(serde_json::json!({"tenants": tenants}))
}

/// POST /admin/tenants — create a tenant and return the API key.
/// No index is auto-created; the tenant creates indexes via POST /indexes/{name}.
pub async fn create_tenant(
    State(state): State<AppState>,
    Json(req): Json<CreateTenantRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_name(&req.name) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response();
    }
    if state.tenant_store.exists(&req.name) {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "tenant already exists"}))).into_response();
    }
    match state.tenant_store.create(&req.name) {
        Ok(tenant) => (StatusCode::CREATED, Json(serde_json::json!({
            "name": tenant.name,
            "api_key": tenant.api_key,
            "created_at": tenant.created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// DELETE /admin/tenants/{name} — delete tenant and cascade-delete all their indexes.
pub async fn delete_tenant(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.tenant_store.delete(&name) {
        Ok(true) => {
            state.registry.delete_by_prefix(&name);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "tenant not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// GET /admin/tenants/{name}/indexes — list index names owned by a tenant.
pub async fn list_tenant_indexes(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if !state.tenant_store.exists(&name) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "tenant not found"}))).into_response();
    }
    let indexes = state.registry.list_for_tenant(&name);
    Json(serde_json::json!({"tenant": name, "indexes": indexes})).into_response()
}

/// POST /admin/tenants/{name}/rotate-key — issue a new API key for a tenant.
/// The old key is invalidated immediately.
pub async fn rotate_key(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.tenant_store.rotate_key(&name) {
        Ok(Some(tenant)) => Json(serde_json::json!({
            "name": tenant.name,
            "api_key": tenant.api_key,
        })).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "tenant not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
