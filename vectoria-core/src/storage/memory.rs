use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, EventType, Product};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

/// In-memory StorageEngine for tests and development.
pub struct MemoryStorage {
    products: RwLock<HashMap<String, Product>>,
    events: RwLock<Vec<Event>>,
    signals_cache: RwLock<HashMap<String, ProductSignals>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            products: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            signals_cache: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageEngine for MemoryStorage {
    async fn put_product(&self, product: &Product) -> Result<()> {
        self.products
            .write()
            .unwrap()
            .insert(product.id.clone(), product.clone());
        Ok(())
    }

    async fn get_product(&self, id: &str) -> Result<Option<Product>> {
        Ok(self.products.read().unwrap().get(id).cloned())
    }

    async fn delete_product(&self, id: &str) -> Result<()> {
        self.products.write().unwrap().remove(id);
        Ok(())
    }

    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>> {
        let products = self.products.read().unwrap();
        Ok(products.values().skip(offset).take(limit).cloned().collect())
    }

    async fn put_event(&self, event: &Event) -> Result<()> {
        self.events.write().unwrap().push(event.clone());
        Ok(())
    }

    async fn get_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        // Return cached signals if available.
        if let Some(s) = self.signals_cache.read().unwrap().get(product_id).cloned() {
            return Ok(s);
        }
        // Compute from raw events.
        let events = self.events.read().unwrap();
        let mut signals = ProductSignals::default();
        for event in events.iter().filter(|e| e.product_id == product_id) {
            match event.event_type {
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
        Ok(signals)
    }

    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()> {
        self.signals_cache
            .write()
            .unwrap()
            .insert(product_id.to_string(), ProductSignals {
                click_count: signals.click_count,
                purchase_count: signals.purchase_count,
                view_count: signals.view_count,
                cart_count: signals.cart_count,
                popularity: signals.popularity,
                conversion_rate: signals.conversion_rate,
            });
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats> {
        Ok(StorageStats {
            product_count: self.products.read().unwrap().len() as u64,
            event_count: self.events.read().unwrap().len() as u64,
            storage_bytes: 0,
        })
    }
}
