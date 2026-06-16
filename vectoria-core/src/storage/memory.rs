use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, Product};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Default)]
pub struct MemoryStorage {
    products: RwLock<HashMap<String, Product>>,
    events: RwLock<Vec<Event>>,
    signals_cache: RwLock<HashMap<String, ProductSignals>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
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
        if let Some(s) = self.signals_cache.read().unwrap().get(product_id).cloned() {
            return Ok(s);
        }
        self.recompute_product_signals(product_id).await
    }

    async fn recompute_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        let events = self.events.read().unwrap();
        Ok(super::compute_signals_from_events(
            events.iter().filter(|e| e.product_id == product_id),
        ))
    }

    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()> {
        self.signals_cache
            .write()
            .unwrap()
            .insert(product_id.to_string(), signals.clone());
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
