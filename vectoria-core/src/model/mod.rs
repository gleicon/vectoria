use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub fn build_product_text(
    metadata: &serde_json::Value,
    field_weights: Option<&HashMap<String, usize>>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    for field in &["title", "name", "brand", "category", "description"] {
        if let Some(v) = metadata.get(field).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                let repeat = field_weights
                    .and_then(|fw| fw.get(*field))
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                for _ in 0..repeat {
                    parts.push(v.to_string());
                }
            }
        }
    }

    if let Some(attrs) = metadata.get("attributes").and_then(|v| v.as_object()) {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                parts.push(format!("{}: {}", k, s));
            }
        }
    }

    parts.join(". ")
}

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
    /// BM25 match context windows. Present when `snippets: true` in the request
    /// and the text index was built under EdgeStore v3 format. Re-index required
    /// after upgrading from edgestore < 1.6 to populate position data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    /// Per-signal breakdown. Each factor's `contribution = score × weight`.
    pub factors: Vec<ScoreFactor>,
    /// How this product entered the candidate set: subset of `["bm25", "vector"]`.
    pub match_sources: Vec<String>,
    /// Query transformations applied before scoring.
    pub query_context: QueryContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreFactor {
    pub factor: String,
    /// Raw signal value (0.0–1.0).
    pub score: f32,
    /// Configured weight for this factor.
    pub weight: f32,
    /// Actual contribution to the total score: `score × weight`.
    pub contribution: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryContext {
    pub original_query: String,
    /// Query actually used for BM25 (may differ if spell-corrected, expanded, or LLM-rewritten).
    pub effective_query: String,
    pub spell_corrected: bool,
    pub query_expanded: bool,
    /// `true` if an LLM rewrote the query to improve low-recall results.
    #[serde(default)]
    pub llm_rewritten: bool,
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
    /// If `true`, group results into semantic clusters and return `clusters` in the response.
    #[serde(default)]
    pub cluster: bool,
    /// If `true`, each hit includes BM25 context snippets showing where query terms matched.
    /// Mutually exclusive with `scan_stats` — enabling snippets disables I/O accounting.
    /// Requires re-indexing after upgrading from edgestore < 1.6 for non-empty results.
    #[serde(default)]
    pub snippets: bool,
    /// Number of candidates retrieved from BM25 and ANN before hybrid scoring and truncation
    /// to `limit`. Controls the retrieve-wide-then-rerank window.
    ///
    /// Default: `(limit + offset) * 5` (e.g. 100 for limit=20).
    /// Set higher (e.g. 400) for large catalogs or when using `rerank: true` to give the
    /// scorer a wider pool — matching the pattern used by production search engines
    /// (Labrador retrieves 400 candidates before reranking to the final result set).
    /// Clamped to `[limit + offset, 1000]`.
    #[serde(default)]
    pub candidate_pool: Option<usize>,
}

pub const DEFAULT_LIMIT: usize = 20;
pub const MAX_CANDIDATE_POOL: usize = 1000;
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
            cluster: false,
            snippets: false,
            candidate_pool: None,
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

/// Storage-layer I/O accounting for a single BM25 query.
/// Only populated by the EdgeStore backend; `None` for memory-backed indexes or
/// when facet filters are active (which requires `search_text_with_options`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25ScanStats {
    /// 1 if the text index exists and was consulted; 0 if no index has been built yet.
    pub segments_scanned: u32,
    /// Serialized text-index size in bytes. Zero means no index → run reindex.
    pub bytes_scanned: u64,
    /// BM25 results returned (same as the number of keyword matches found).
    pub items_examined: u64,
    /// Total number of documents in the text index (denominator for coverage %).
    pub total_indexed: u64,
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
    /// Semantic clusters of the result set. Only present when `cluster: true` in the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clusters: Option<Vec<crate::search::clustering::Cluster>>,
    /// BM25 I/O stats from the EdgeStore text index. Absent for memory backends
    /// and when facet filters are active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_stats: Option<BM25ScanStats>,
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
    /// Weight for query-specific click-through rate signal.
    /// Products previously clicked after this exact query rank higher.
    #[serde(default = "w_query_ctr")]
    pub query_ctr: f32,
}

impl Default for RankingWeights {
    fn default() -> Self {
        Self {
            semantic: w_semantic(),
            bm25: w_bm25(),
            popularity: w_popularity(),
            availability: w_availability(),
            margin: w_margin(),
            query_ctr: w_query_ctr(),
        }
    }
}

fn w_semantic() -> f32 { 0.7 }
fn w_bm25() -> f32 { 0.3 }
fn w_popularity() -> f32 { 0.2 }
fn w_availability() -> f32 { 0.05 }
fn w_margin() -> f32 { 0.05 }
fn w_query_ctr() -> f32 { 0.15 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarRequest {
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    pub product_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Relationship type between two products.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// Products from the same brand (populated at aggregation time).
    Brand,
    /// Products frequently purchased or clicked together (co-occurrence signal).
    CoPurchased,
}

impl RelationType {
    /// Canonical string key used in storage and the REST API (`"brand"` or `"co_purchased"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationType::Brand => "brand",
            RelationType::CoPurchased => "co_purchased",
        }
    }

    /// Parse a relation type from its storage key. Returns `None` for unknown strings.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "brand" => Some(RelationType::Brand),
            "co_purchased" => Some(RelationType::CoPurchased),
            _ => None,
        }
    }
}

/// A related product hit returned by `GET /products/{id}/related`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedHit {
    pub id: String,
    pub relation_type: RelationType,
    /// Normalized relevance score for this relation (0.0–1.0).
    pub score: f32,
    pub metadata: serde_json::Value,
}

/// Hard pin: forces a specific product to a fixed position for a query.
/// Deterministic, instant — bypasses scoring entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub query: String,
    pub product_id: String,
    /// 1-indexed target position in the result list.
    pub position: usize,
    pub created_at: DateTime<Utc>,
}

impl Pin {
    pub fn new(query: impl Into<String>, product_id: impl Into<String>, position: usize) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            query: query.into(),
            product_id: product_id.into(),
            position,
            created_at: Utc::now(),
        }
    }
}

/// Sponsored slot: injects an advertiser product at a fixed position for a query pattern.
/// Injected before organic results; marked with `sponsored: true` in metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SponsoredSlot {
    pub id: String,
    /// Exact query or prefix pattern to match against.
    pub query_pattern: String,
    pub product_id: String,
    /// 1-indexed position in the final result list.
    pub position: usize,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    /// Display label shown in UI (e.g. "Sponsored", "Ad").
    pub label: String,
    pub created_at: DateTime<Utc>,
}

impl SponsoredSlot {
    pub fn new(
        query_pattern: impl Into<String>,
        product_id: impl Into<String>,
        position: usize,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            query_pattern: query_pattern.into(),
            product_id: product_id.into(),
            position,
            start_at: None,
            end_at: None,
            label: label.into(),
            created_at: Utc::now(),
        }
    }
}

/// Suppression: hides a product entirely for a specific query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suppression {
    pub id: String,
    pub query: String,
    pub product_id: String,
    pub created_at: DateTime<Utc>,
}

impl Suppression {
    pub fn new(query: impl Into<String>, product_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            query: query.into(),
            product_id: product_id.into(),
            created_at: Utc::now(),
        }
    }
}

/// Full export of all Phase 2 admin overrides. Used by GET/POST /admin/training-export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideExport {
    pub pins: Vec<Pin>,
    pub sponsored: Vec<SponsoredSlot>,
    pub suppressions: Vec<Suppression>,
    pub exported_at: DateTime<Utc>,
}
