use crate::model::{Event, Product};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

#[async_trait]
pub trait StorageEngine: Send + Sync {
    async fn put_product(&self, product: &Product) -> Result<()>;
    async fn get_product(&self, id: &str) -> Result<Option<Product>>;
    async fn delete_product(&self, id: &str) -> Result<()>;
    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>>;
    async fn put_event(&self, event: &Event) -> Result<()>;
    async fn get_product_signals(&self, product_id: &str) -> Result<ProductSignals>;
    async fn recompute_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        self.get_product_signals(product_id).await
    }
    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()>;
    async fn stats(&self) -> Result<StorageStats>;

    /// Returns normalized query-CTR scores (0.0–1.0) for the given product IDs,
    /// based on click and purchase events that carried the exact query string.
    /// Products with no matching events return 0.0 (absent from the map).
    async fn get_query_ctrs(&self, _query: &str) -> Result<HashMap<String, f32>> {
        Ok(HashMap::new())
    }
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

pub(super) fn compute_signals_from_events<'a>(
    events: impl Iterator<Item = &'a crate::model::Event>,
) -> ProductSignals {
    use crate::model::EventType;
    let mut signals = ProductSignals::default();
    for event in events {
        match &event.event_type {
            EventType::Click => signals.click_count += 1,
            EventType::Purchase => signals.purchase_count += 1,
            EventType::View => signals.view_count += 1,
            EventType::AddToCart => signals.cart_count += 1,
            EventType::Wishlist => {}
        }
    }
    let total = signals.view_count.max(1);
    signals.popularity = (signals.click_count as f32 / total as f32).min(1.0);
    signals.conversion_rate = (signals.purchase_count as f32 / total as f32).min(1.0);
    signals
}
