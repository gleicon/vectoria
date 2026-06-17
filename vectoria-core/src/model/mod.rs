use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    pub metadata: serde_json::Value,
    pub model_id: Option<String>,
    pub dims: Option<usize>,
    pub status: ProductStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Product {
    pub fn new(id: impl Into<String>, metadata: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            text: None,
            vector: None,
            metadata,
            model_id: None,
            dims: None,
            status: ProductStatus::PendingVector,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductStatus {
    PendingVector,
    Indexed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event_type: EventType,
    pub user_id: Option<String>,
    pub product_id: String,
    pub query: Option<String>,
    pub session_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl Event {
    pub fn new(event_type: EventType, product_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type,
            user_id: None,
            product_id: product_id.into(),
            query: None,
            session_id: None,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    View,
    Click,
    AddToCart,
    Wishlist,
    Purchase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub id: String,
    pub score: f32,
    pub metadata: serde_json::Value,
    pub explain: Option<ScoreBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub factors: Vec<ScoreFactor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreFactor {
    pub factor: String,
    pub score: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_mode")]
    pub mode: SearchMode,
    pub filters: Option<HashMap<String, serde_json::Value>>,
    pub ranking_weights: Option<RankingWeights>,
    pub aggregate: Option<Vec<String>>,
    #[serde(default)]
    pub explain: bool,
    #[serde(default)]
    pub rerank: bool,
}

pub const DEFAULT_LIMIT: usize = 20;
fn default_limit() -> usize { DEFAULT_LIMIT }
fn default_mode() -> SearchMode { SearchMode::Hybrid }

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            q: String::new(),
            limit: DEFAULT_LIMIT,
            offset: 0,
            mode: SearchMode::Hybrid,
            filters: None,
            ranking_weights: None,
            aggregate: None,
            explain: false,
            rerank: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Hybrid,
    Semantic,
    Bm25,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<Hit>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub processing_time_ms: u64,
    pub query: String,
    pub aggregations: Option<HashMap<String, HashMap<String, usize>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingWeights {
    #[serde(default = "w_semantic")]
    pub semantic: f32,
    #[serde(default = "w_bm25")]
    pub bm25: f32,
    #[serde(default = "w_popularity")]
    pub popularity: f32,
    #[serde(default = "w_availability")]
    pub availability: f32,
    #[serde(default = "w_margin")]
    pub margin: f32,
}

impl Default for RankingWeights {
    fn default() -> Self {
        Self {
            semantic: w_semantic(),
            bm25: w_bm25(),
            popularity: w_popularity(),
            availability: w_availability(),
            margin: w_margin(),
        }
    }
}

fn w_semantic() -> f32 { 0.7 }
fn w_bm25() -> f32 { 0.3 }
fn w_popularity() -> f32 { 0.2 }
fn w_availability() -> f32 { 0.05 }
fn w_margin() -> f32 { 0.05 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarRequest {
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    pub product_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub filters: Option<HashMap<String, serde_json::Value>>,
}
