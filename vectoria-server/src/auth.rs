use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use crate::state::AppState;

/// The resolved caller identity, inserted into request extensions by `require_api_key`.
/// Downstream handlers and middleware read this to enforce access control.
#[derive(Clone, Debug)]
pub enum Principal {
    /// Holder of the global admin API key — unrestricted access.
    Admin,
    /// Holder of a per-tenant API key. May only access `/indexes/{name}/*`
    /// where `name` equals the tenant name stored here.
    Tenant(String),
}

/// Auth middleware: validates the API key and inserts a `Principal` extension.
/// Does NOT enforce which routes are accessible — that is the job of
/// `require_admin` (for admin-only routes) and the named-index handlers.
pub async fn require_api_key(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let index_key = request
        .headers()
        .get("X-Search-API-Key")
        .and_then(|v| v.to_str().ok());

    let key = auth.or(index_key);

    let principal = if key.as_deref() == Some(state.api_key.as_str()) {
        Principal::Admin
    } else if let Some(name) = key
        .as_deref()
        .and_then(|k| state.tenant_keys.get(k))
        .cloned()
    {
        Principal::Tenant(name)
    } else if let Some(name) = key
        .as_deref()
        .and_then(|k| state.tenant_store.lookup_key(k))
    {
        Principal::Tenant(name)
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

/// Middleware that rejects tenant principals with 403 Forbidden.
/// Layer this over every route that must not be reachable by tenant API keys.
pub async fn require_admin(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match request.extensions().get::<Principal>() {
        Some(Principal::Admin) => Ok(next.run(request).await),
        Some(Principal::Tenant(_)) => Err(StatusCode::FORBIDDEN),
        None => Err(StatusCode::UNAUTHORIZED),
    }
}
