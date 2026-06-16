use std::sync::Arc;
use vectoria_core::SearchEngine;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<SearchEngine>,
    pub api_key: String,
}
