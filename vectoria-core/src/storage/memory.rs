use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, Product};
use crate::search::bm25_index::Bm25Index;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;


#[derive(Default)]
pub struct MemoryStorage {
    products: RwLock<HashMap<String, Product>>,
    events: RwLock<Vec<Event>>,
    signals_cache: RwLock<HashMap<String, ProductSignals>>,
    bm25: Bm25Index,
    user_vectors: RwLock<HashMap<String, Vec<f32>>>,
    // user_id → ordered list of product_ids (click/purchase)
    user_product_history: RwLock<HashMap<String, Vec<String>>>,
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
        use crate::model::EventType;
        if let (Some(uid), EventType::Click | EventType::Purchase) =
            (&event.user_id, &event.event_type)
        {
            self.user_product_history
                .write()
                .unwrap()
                .entry(uid.clone())
                .or_default()
                .push(event.product_id.clone());
        }
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

    async fn get_query_ctrs(&self, query: &str) -> Result<HashMap<String, f32>> {
        use crate::model::EventType;
        let events = self.events.read().unwrap();
        let mut counts: HashMap<String, u32> = HashMap::new();
        for event in events.iter() {
            if event.query.as_deref() == Some(query)
                && matches!(event.event_type, EventType::Click | EventType::Purchase)
            {
                *counts.entry(event.product_id.clone()).or_insert(0) += 1;
            }
        }
        let max = counts.values().copied().fold(0u32, u32::max) as f32;
        if max == 0.0 { return Ok(HashMap::new()); }
        Ok(counts.into_iter().map(|(id, c)| (id, c as f32 / max)).collect())
    }

    async fn stats(&self) -> Result<StorageStats> {
        Ok(StorageStats {
            product_count: self.products.read().unwrap().len() as u64,
            event_count: self.events.read().unwrap().len() as u64,
            storage_bytes: 0,
            text_document_count: self.bm25.len() as u64,
        })
    }

    async fn index_text(&self, id: &str, text: &str, _metadata: &serde_json::Value) -> Result<()> {
        self.bm25.upsert(id, text);
        Ok(())
    }

    async fn search_text(
        &self,
        query: &str,
        limit: usize,
        _filters: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<(String, f32)>> {
        Ok(self.bm25.search(query, limit))
    }

    async fn delete_text(&self, id: &str) -> Result<()> {
        self.bm25.remove(id);
        Ok(())
    }

    fn suggest_text(&self, prefix: &str, limit: usize) -> Vec<String> {
        self.bm25.suggest(prefix, limit)
    }

    async fn put_user_vector(&self, user_id: &str, vector: &[f32]) -> Result<()> {
        self.user_vectors
            .write()
            .unwrap()
            .insert(user_id.to_string(), vector.to_vec());
        Ok(())
    }

    async fn get_user_vector(&self, user_id: &str) -> Result<Option<Vec<f32>>> {
        Ok(self.user_vectors.read().unwrap().get(user_id).cloned())
    }

    async fn get_user_recent_products(&self, user_id: &str, limit: usize) -> Result<Vec<String>> {
        let history = self.user_product_history.read().unwrap();
        let products = history.get(user_id).map(|v| v.as_slice()).unwrap_or(&[]);
        // Deduplicated most-recent `limit` products.
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<String> = products
            .iter()
            .rev()
            .filter(|id| seen.insert((*id).clone()))
            .take(limit)
            .cloned()
            .collect();
        Ok(deduped)
    }

    async fn list_user_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .user_product_history
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect())
    }
}
