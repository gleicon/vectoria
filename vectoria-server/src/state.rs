use std::collections::HashMap;
use std::sync::Arc;
use crate::index_registry::IndexRegistry;
use crate::rate_limit::SharedRateLimiter;
use crate::tenants::TenantStore;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<IndexRegistry>,
    /// Global admin API key.
    pub api_key: String,
    /// Static per-tenant keys from config file: api_key → tenant_name.
    pub tenant_keys: Arc<HashMap<String, String>>,
    /// Dynamically-managed tenants (created via API). Checked after static keys.
    pub tenant_store: Arc<TenantStore>,
    pub limiter: Option<SharedRateLimiter>,
}
