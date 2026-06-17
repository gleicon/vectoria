use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, EventType, Product};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{EdgestoreConfig, Engine};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

const NS_PRODUCTS: &[u8] = b"products";
const NS_EVENTS: &[u8] = b"events";
const NS_SIGNALS: &[u8] = b"signals";
// Key: {query_bytes}\x00{product_id_bytes}, Value: u64 LE count.
// Null-byte separator is safe: JSON strings never contain \x00.
const NS_CTRS: &[u8] = b"ctrs";
// Queries longer than this are not written to NS_CTRS; reads return empty immediately.
// Prevents storage amplification from oversized client-supplied query strings.
const MAX_QUERY_BYTES: usize = 512;

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
        let key = format!("{}/{}", event.product_id, event.id).into_bytes();
        let value = encode(event)?;
        let ctr_key: Option<Vec<u8>> = match (&event.query, &event.event_type) {
            (Some(q), EventType::Click | EventType::Purchase) if q.len() <= MAX_QUERY_BYTES => {
                let mut k = q.as_bytes().to_vec();
                k.push(0);
                k.extend_from_slice(event.product_id.as_bytes());
                Some(k)
            }
            _ => None,
        };
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            let mut eng = engine.lock().unwrap();
            eng.put(NS_EVENTS, &key, &value).context("put_event failed")?;
            if let Some(ck) = ctr_key {
                let count = eng.get(NS_CTRS, &ck)?
                    .and_then(|b| <[u8; 8]>::try_from(b).ok())
                    .map(u64::from_le_bytes)
                    .unwrap_or(0);
                eng.put(NS_CTRS, &ck, &(count + 1).to_le_bytes()).context("put_ctr failed")?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        Ok(())
    }

    async fn get_query_ctrs(&self, query: &str) -> Result<HashMap<String, f32>> {
        if query.len() > MAX_QUERY_BYTES {
            return Ok(HashMap::new());
        }
        let mut prefix = query.as_bytes().to_vec();
        prefix.push(0);
        let prefix_len = prefix.len();
        let engine = Arc::clone(&self.engine);
        let pairs = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().prefix(NS_CTRS, &prefix)
        })
        .await??;
        let counts: HashMap<String, u64> = pairs
            .into_iter()
            .filter_map(|(k, v)| {
                let product_id = String::from_utf8(k[prefix_len..].to_vec()).ok()?;
                let count = <[u8; 8]>::try_from(v).ok().map(u64::from_le_bytes)?;
                Some((product_id, count))
            })
            .collect();
        let max = counts.values().copied().max().unwrap_or(0) as f32;
        if max == 0.0 {
            return Ok(HashMap::new());
        }
        Ok(counts.into_iter().map(|(id, c)| (id, c as f32 / max)).collect())
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
