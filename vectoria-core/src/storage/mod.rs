use crate::model::{Event, Product};
use anyhow::Result;
use async_trait::async_trait;

/// Abstraction over the metadata + KV storage layer.
/// Default implementation: EdgeStore. Fallback: SQLite.
#[async_trait]
pub trait StorageEngine: Send + Sync {
    async fn put_product(&self, product: &Product) -> Result<()>;
    async fn get_product(&self, id: &str) -> Result<Option<Product>>;
    async fn delete_product(&self, id: &str) -> Result<()>;
    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>>;
    async fn put_event(&self, event: &Event) -> Result<()>;
    /// Returns cached pre-aggregated signals, or computes from events if not cached.
    async fn get_product_signals(&self, product_id: &str) -> Result<ProductSignals>;
    /// Persist pre-computed signals (called by background aggregation job).
    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()>;
    async fn stats(&self) -> Result<StorageStats>;
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
pub struct ProductSignals {
    pub click_count: u64,
    pub purchase_count: u64,
    pub view_count: u64,
    pub cart_count: u64,
    /// Normalized 0.0–1.0 popularity score.
    pub popularity: f32,
    /// Normalized 0.0–1.0 conversion rate.
    pub conversion_rate: f32,
}

#[derive(Debug, Default)]
pub struct StorageStats {
    pub product_count: u64,
    pub event_count: u64,
    pub storage_bytes: u64,
}

pub mod edgestore;
pub mod memory;
pub mod sqlite;
