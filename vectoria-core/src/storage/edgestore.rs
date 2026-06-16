use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, Product};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{EdgestoreConfig, Engine};
use std::path::Path;
use std::sync::{Arc, Mutex};

const NS_PRODUCTS: &[u8] = b"products";
const NS_EVENTS: &[u8] = b"events";
const NS_SIGNALS: &[u8] = b"signals";

pub struct EdgeStoreStorage {
    engine: Arc<Mutex<Engine>>,
}

impl EdgeStoreStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let config = EdgestoreConfig::new(path.as_ref());
        let engine = Engine::open(config).context("failed to open EdgeStore")?;
        Ok(Self { engine: Arc::new(Mutex::new(engine)) })
    }
}

fn encode<T: serde::Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).context("serialize failed")
}

fn decode<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T> {
    serde_json::from_slice(b).context("deserialize failed")
}

#[async_trait]
impl StorageEngine for EdgeStoreStorage {
    async fn put_product(&self, product: &Product) -> Result<()> {
        let key = product.id.as_bytes().to_vec();
        let value = encode(product)?;
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().put(NS_PRODUCTS, &key, &value)
        })
        .await?
        .context("put_product failed")?;
        Ok(())
    }

    async fn get_product(&self, id: &str) -> Result<Option<Product>> {
        let key = id.as_bytes().to_vec();
        let engine = Arc::clone(&self.engine);
        let bytes = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().get(NS_PRODUCTS, &key)
        })
        .await??;
        match bytes {
            None => Ok(None),
            Some(b) => Ok(Some(decode(&b)?)),
        }
    }

    async fn delete_product(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().delete(NS_PRODUCTS, &key)
        })
        .await?
        .context("delete_product failed")?;
        Ok(())
    }

    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>> {
        let engine = Arc::clone(&self.engine);
        let pairs = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().prefix(NS_PRODUCTS, b"")
        })
        .await??;

        pairs
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(_, v)| decode(&v))
            .collect()
    }

    async fn put_event(&self, event: &Event) -> Result<()> {
        // Key: "{product_id}/{event_id}" — prefix scan by product
        let key = format!("{}/{}", event.product_id, event.id).into_bytes();
        let value = encode(event)?;
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().put(NS_EVENTS, &key, &value)
        })
        .await?
        .context("put_event failed")?;
        Ok(())
    }

    async fn get_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        let key = product_id.as_bytes().to_vec();
        let engine = Arc::clone(&self.engine);
        let cached = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().get(NS_SIGNALS, &key)
        })
        .await??;
        if let Some(b) = cached {
            if let Ok(s) = decode::<ProductSignals>(&b) {
                return Ok(s);
            }
        }
        self.recompute_product_signals(product_id).await
    }

    async fn recompute_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        let prefix = format!("{}/", product_id).into_bytes();
        let engine = Arc::clone(&self.engine);
        let pairs = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().prefix(NS_EVENTS, &prefix)
        })
        .await??;
        let events: Vec<crate::model::Event> = pairs
            .iter()
            .filter_map(|(_, v)| serde_json::from_slice(v).ok())
            .collect();
        Ok(super::compute_signals_from_events(events.iter()))
    }

    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()> {
        let key = product_id.as_bytes().to_vec();
        let value = encode(signals)?;
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().put(NS_SIGNALS, &key, &value)
        })
        .await?
        .context("put_product_signals failed")?;
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats> {
        let engine = Arc::clone(&self.engine);
        let result = tokio::task::spawn_blocking(move || -> Result<StorageStats> {
            let eng = engine.lock().unwrap();
            let product_count = eng.prefix(NS_PRODUCTS, b"").context("stats: prefix products")?.len() as u64;
            let event_count   = eng.prefix(NS_EVENTS,   b"").context("stats: prefix events")?.len() as u64;
            Ok(StorageStats { product_count, event_count, storage_bytes: crate::dir_bytes(eng.db_path()) })
        })
        .await??;
        Ok(result)
    }
}
