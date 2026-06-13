use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use crate::state::AppState;

/// Middleware: require `Authorization: Bearer <key>` on every request.
pub async fn require_api_key(
    State(state): State<AppState>,
    request: Request,
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

    match key {
        Some(k) if k == state.api_key => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
