use std::collections::HashMap;
use std::sync::Arc;
use crate::index_registry::IndexRegistry;
use crate::rate_limit::SharedRateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<IndexRegistry>,
    /// Global admin API key.
    pub api_key: String,
    /// Per-tenant keys: api_key → tenant_name (namespace).
    pub tenant_keys: Arc<HashMap<String, String>>,
    pub limiter: Option<SharedRateLimiter>,
}
