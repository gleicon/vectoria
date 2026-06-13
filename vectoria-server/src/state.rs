use std::sync::Arc;
use vectoria_core::SearchEngine;
use crate::index_registry::IndexRegistry;

/// Shared application state injected into every route handler.
#[derive(Clone)]
pub struct AppState {
    /// Default search engine (for /search, /products, /events endpoints).
    pub engine: Arc<SearchEngine>,
    /// Per-index engine registry (for /1/indexes/{indexName}/* multi-index endpoints).
    pub index_registry: Arc<IndexRegistry>,
    pub api_key: String,
}
