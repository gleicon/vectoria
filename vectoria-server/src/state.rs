use std::sync::Arc;
use crate::index_registry::IndexRegistry;
use crate::rate_limit::SharedRateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<IndexRegistry>,
    pub api_key: String,
    pub limiter: Option<SharedRateLimiter>,
}
