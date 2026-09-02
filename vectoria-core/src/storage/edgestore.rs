use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, EventType, Product};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{EdgestoreConfig, Engine, FacetFilter, FacetValue, SearchOptions, SnippetResult, TextEngine};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

/// Unified engine handle for standalone (RwLock, concurrent reads) and
/// replicated (Mutex, single-writer required) modes.
#[derive(Clone)]
enum EngineRef {
    Rw(Arc<RwLock<Engine>>),
    Ex(Arc<Mutex<Engine>>),
}

impl EngineRef {
    fn with_read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Engine) -> R,
    {
        match self {
            Self::Rw(rw) => f(&rw.read().unwrap()),
            Self::Ex(ex) => f(&ex.lock().unwrap()),
        }
    }

    fn with_write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Engine) -> R,
    {
        match self {
            Self::Rw(rw) => f(&mut rw.write().unwrap()),
            Self::Ex(ex) => f(&mut ex.lock().unwrap()),
        }
    }
}

const NS_PRODUCTS: &[u8] = b"products";
const NS_EVENTS: &[u8] = b"events";
const NS_SIGNALS: &[u8] = b"signals";
const NS_TEXT: &[u8] = b"text";
// Key: {query_bytes}\x00{product_id_bytes}, Value: u64 LE count.
// Null-byte separator is safe: JSON strings never contain \x00.
const NS_CTRS: &[u8] = b"ctrs";
const NS_USERS: &[u8] = b"users";
// Key: {user_id}\x00{event_id}, Value: product_id bytes.
// Enables O(user_events) scan without scanning all events.
const NS_USER_EVENTS: &[u8] = b"userevents";
// Key: {from_id}\x00{rel_type}\x00{to_id}, Value: u64 LE co-occurrence count.
const NS_RELATIONS: &[u8] = b"relations";
const NS_PINS: &[u8] = b"pins";
const NS_SPONSORED: &[u8] = b"sponsored";
const NS_SUPPRESSIONS: &[u8] = b"suppressions";

const MAX_QUERY_BYTES: usize = 512;
// user_id is caller-supplied; cap it to prevent storage key amplification.
const MAX_USER_ID_BYTES: usize = 256;

pub struct EdgeStoreStorage {
    engine: EngineRef,
    // Caches product count for `total_indexed` to avoid O(n) prefix scan on every search.
    // Invalidated after 5 seconds; accurate enough for coverage ratios in the quality panel.
    count_cache: Arc<std::sync::Mutex<Option<(u64, std::time::Instant)>>>,
}

impl EdgeStoreStorage {
    /// Create from a pre-opened shared engine.
    ///
    /// Both storage and vector index should share the same engine instance so they
    /// share one WAL, one lock file, and one replication target.
    pub fn from_engine(engine: Arc<RwLock<Engine>>) -> Self {
        Self { engine: EngineRef::Rw(engine), count_cache: Arc::new(std::sync::Mutex::new(None)) }
    }

    /// Replicated mode: engine is held by a `ReplicatedEngine` (single-writer, Mutex required).
    pub fn from_mutex_engine(engine: Arc<Mutex<Engine>>) -> Self {
        Self { engine: EngineRef::Ex(engine), count_cache: Arc::new(std::sync::Mutex::new(None)) }
    }

    /// Convenience: open a new engine at `path` and wrap it.
    ///
    /// Prefer `from_engine` when a vector index shares the same engine.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let config = EdgestoreConfig::new(path.as_ref());
        let engine = Engine::open(config).context("failed to open EdgeStore")?;
        Ok(Self {
            engine: EngineRef::Rw(Arc::new(RwLock::new(engine))),
            count_cache: Arc::new(std::sync::Mutex::new(None)),
        })
    }

    /// Returns cached product count. Recomputes at most once per 5 seconds
    /// to avoid an O(products) prefix scan on every search.
    async fn cached_product_count(&self) -> u64 {
        const TTL: std::time::Duration = std::time::Duration::from_secs(5);
        {
            let guard = self.count_cache.lock().unwrap();
            if let Some((count, at)) = *guard {
                if at.elapsed() < TTL {
                    return count;
                }
            }
        }
        let engine = self.engine.clone();
        let count = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_PRODUCTS, b"").map(|p| p.len() as u64).unwrap_or(0))
        })
        .await
        .unwrap_or(0);
        *self.count_cache.lock().unwrap() = Some((count, std::time::Instant::now()));
        count
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
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.put(NS_PRODUCTS, &key, &value))
        })
        .await?
        .context("put_product failed")?;
        Ok(())
    }

    async fn get_product(&self, id: &str) -> Result<Option<Product>> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.get(NS_PRODUCTS, &key))
        })
        .await??;
        match bytes {
            None => Ok(None),
            Some(b) => Ok(Some(decode(&b)?)),
        }
    }

    async fn delete_product(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.delete(NS_PRODUCTS, &key))
        })
        .await?
        .context("delete_product failed")?;
        Ok(())
    }

    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>> {
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_PRODUCTS, b""))
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
        // Dual-write: index event under user_id for fast per-user lookups.
        let user_event_key: Option<Vec<u8>> = match (&event.user_id, &event.event_type) {
            (Some(uid), EventType::Click | EventType::Purchase)
                if uid.len() <= MAX_USER_ID_BYTES =>
            {
                let mut k = uid.as_bytes().to_vec();
                k.push(0);
                k.extend_from_slice(event.id.as_bytes());
                Some(k)
            }
            _ => None,
        };
        let product_id_bytes = event.product_id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|eng| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                if let Some(ck) = ctr_key {
                    // Atomic: event record + CTR increment in one transaction.
                    let count = eng.get(NS_CTRS, &ck)?
                        .and_then(|b| <[u8; 8]>::try_from(b).ok())
                        .map(u64::from_le_bytes)
                        .unwrap_or(0);
                    let mut tx = eng.begin();
                    tx.put(NS_EVENTS, &key, &value, 0, now).context("tx event put failed")?;
                    tx.put(NS_CTRS, &ck, &(count + 1).to_le_bytes(), 0, now).context("tx ctr put failed")?;
                    if let Some(uek) = user_event_key {
                        tx.put(NS_USER_EVENTS, &uek, &product_id_bytes, 0, now).context("tx user-event put failed")?;
                    }
                    eng.commit_transaction(tx).context("put_event transaction failed")?;
                } else {
                    eng.put(NS_EVENTS, &key, &value).context("put_event failed")?;
                    if let Some(uek) = user_event_key {
                        eng.put(NS_USER_EVENTS, &uek, &product_id_bytes).context("user-event put failed")?;
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
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
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_CTRS, &prefix))
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
        let engine = self.engine.clone();
        let cached = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.get(NS_SIGNALS, &key))
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
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_EVENTS, &prefix))
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
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.put(NS_SIGNALS, &key, &value))
        })
        .await?
        .context("put_product_signals failed")?;
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats> {
        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<StorageStats> {
            engine.with_read(|eng| {
                let product_count = eng.prefix(NS_PRODUCTS, b"").context("stats: prefix products")?.len() as u64;
                let event_count   = eng.prefix(NS_EVENTS,   b"").context("stats: prefix events")?.len() as u64;
                Ok(StorageStats {
                    product_count,
                    event_count,
                    storage_bytes: crate::dir_bytes(eng.db_path()),
                    text_document_count: product_count,
                })
            })
        })
        .await??;
        Ok(result)
    }

    async fn index_text(&self, id: &str, text: &str, metadata: &serde_json::Value) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let text = text.to_string();
        let facets = extract_facets(metadata);
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.index_text(NS_TEXT, &key, &text, facets).context("index_text failed"))
        })
        .await??;
        Ok(())
    }

    async fn search_text(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<(String, f32)>> {
        let query = query.to_string();
        let facet_filters = filters.map(to_facet_filters).unwrap_or_default();
        let engine = self.engine.clone();
        let results = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| {
                e.search_text_with_options(
                    NS_TEXT,
                    &query,
                    &SearchOptions { k: limit, typo_tolerance: false, facet_filters },
                )
                .context("search_text failed")
            })
        })
        .await??;
        Ok(results
            .into_iter()
            .map(|r| (String::from_utf8_lossy(&r.doc_id).into_owned(), r.score))
            .collect())
    }

    async fn search_text_with_stats(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(Vec<(String, f32)>, Option<crate::model::BM25ScanStats>)> {
        let has_filters = filters.map_or(false, |f| !f.is_empty());
        if has_filters {
            // Facet filters require search_text_with_options; no stats available.
            let results = self.search_text(query, limit, filters).await?;
            return Ok((results, None));
        }
        let query = query.to_string();
        let engine = self.engine.clone();
        let (results, stats) = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| {
                e.search_text_with_stats(NS_TEXT, &query, limit)
                    .context("search_text_with_stats failed")
            })
        })
        .await??;
        let pairs = results
            .into_iter()
            .map(|r| (String::from_utf8_lossy(&r.doc_id).into_owned(), r.score))
            .collect();
        let total_indexed = self.cached_product_count().await;
        let scan = crate::model::BM25ScanStats {
            segments_scanned: stats.segments_scanned,
            bytes_scanned: stats.bytes_scanned,
            items_examined: stats.items_examined,
            total_indexed,
        };
        Ok((pairs, Some(scan)))
    }

    async fn search_text_with_snippets(
        &self,
        query: &str,
        limit: usize,
        context_chars: usize,
    ) -> Result<Vec<(String, f32, Vec<String>)>> {
        let query = query.to_string();
        let engine = self.engine.clone();
        let results: Vec<SnippetResult> = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| {
                e.search_text_with_snippets(NS_TEXT, &query, limit, context_chars)
                    .context("search_text_with_snippets failed")
            })
        })
        .await??;
        Ok(results
            .into_iter()
            .map(|r| {
                let id = String::from_utf8_lossy(&r.doc_id).into_owned();
                let snips: Vec<String> = r.snippets.into_iter().map(|s| s.text).collect();
                (id, r.score, snips)
            })
            .collect())
    }

    async fn delete_text(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.delete_text(NS_TEXT, &key).context("delete_text failed"))
        })
        .await??;
        Ok(())
    }

    async fn put_user_vector(&self, user_id: &str, vector: &[f32]) -> Result<()> {
        if user_id.len() > MAX_USER_ID_BYTES {
            anyhow::bail!("user_id exceeds maximum length of {} bytes", MAX_USER_ID_BYTES);
        }
        let key = user_id.as_bytes().to_vec();
        let owned: Vec<f32> = vector.to_vec();
        let value = encode(&owned)?;
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.put(NS_USERS, &key, &value))
        })
        .await?
        .context("put_user_vector failed")?;
        Ok(())
    }

    async fn get_user_vector(&self, user_id: &str) -> Result<Option<Vec<f32>>> {
        if user_id.len() > MAX_USER_ID_BYTES {
            return Ok(None);
        }
        let key = user_id.as_bytes().to_vec();
        let engine = self.engine.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.get(NS_USERS, &key))
        })
        .await??;
        match bytes {
            None => Ok(None),
            Some(b) => Ok(Some(decode(&b)?)),
        }
    }

    async fn get_user_recent_products(&self, user_id: &str, limit: usize) -> Result<Vec<String>> {
        if user_id.len() > MAX_USER_ID_BYTES {
            return Ok(vec![]);
        }
        let mut prefix = user_id.as_bytes().to_vec();
        prefix.push(0);
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_USER_EVENTS, &prefix))
        })
        .await??;
        // Deduplicate while preserving recency (pairs are insertion-ordered).
        let mut seen = std::collections::HashSet::new();
        let products: Vec<String> = pairs
            .into_iter()
            .rev()
            .filter_map(|(_, v)| String::from_utf8(v).ok())
            .filter(|id| seen.insert(id.clone()))
            .take(limit)
            .collect();
        Ok(products)
    }

    async fn list_user_ids(&self) -> Result<Vec<String>> {
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_USER_EVENTS, b""))
        })
        .await??;
        let mut seen = std::collections::HashSet::new();
        for (k, _) in pairs {
            // Key format: {user_id}\x00{event_id}
            if let Some(sep) = k.iter().position(|&b| b == 0) {
                if let Ok(uid) = String::from_utf8(k[..sep].to_vec()) {
                    seen.insert(uid);
                }
            }
        }
        Ok(seen.into_iter().collect())
    }

    async fn put_relation(&self, from: &str, to: &str, rel_type: &str, score: u64) -> Result<()> {
        // Key: {from_id}\x00{rel_type}\x00{to_id}
        let mut key = from.as_bytes().to_vec();
        key.push(0);
        key.extend_from_slice(rel_type.as_bytes());
        key.push(0);
        key.extend_from_slice(to.as_bytes());
        let existing_score = {
            let k2 = key.clone();
            let engine = self.engine.clone();
            tokio::task::spawn_blocking(move || engine.with_read(|e| e.get(NS_RELATIONS, &k2)))
                .await??
                .and_then(|b| <[u8; 8]>::try_from(b).ok())
                .map(u64::from_le_bytes)
                .unwrap_or(0)
        };
        let new_score = existing_score.saturating_add(score);
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.put(NS_RELATIONS, &key, &new_score.to_le_bytes()))
        })
        .await??;
        Ok(())
    }

    async fn get_related(
        &self,
        product_id: &str,
        rel_type_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, String, u64)>> {
        let mut prefix = product_id.as_bytes().to_vec();
        prefix.push(0);
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_RELATIONS, &prefix))
        })
        .await??;

        let prefix_len = product_id.len() + 1; // skip "{from}\x00"
        let mut results: Vec<(String, String, u64)> = pairs
            .into_iter()
            .filter_map(|(k, v)| {
                let rest = k.get(prefix_len..)?;
                // rest = {rel_type}\x00{to_id}
                let sep = rest.iter().position(|&b| b == 0)?;
                let rel_type = String::from_utf8(rest[..sep].to_vec()).ok()?;
                let to_id = String::from_utf8(rest[sep + 1..].to_vec()).ok()?;
                let count = <[u8; 8]>::try_from(v).ok().map(u64::from_le_bytes)?;
                Some((to_id, rel_type, count))
            })
            .filter(|(_, rt, _)| rel_type_filter.is_none_or(|f| f == rt))
            .collect();

        results.sort_by(|a, b| b.2.cmp(&a.2));
        results.truncate(limit);
        Ok(results)
    }

    async fn delete_product_relations(&self, product_id: &str) -> Result<()> {
        let mut prefix = product_id.as_bytes().to_vec();
        prefix.push(0);
        let engine = self.engine.clone();
        let keys: Vec<Vec<u8>> = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_RELATIONS, &prefix))
        })
        .await??
        .into_iter()
        .map(|(k, _)| k)
        .collect();

        if keys.is_empty() {
            return Ok(());
        }
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|eng| {
                for k in keys {
                    eng.delete(NS_RELATIONS, &k)?;
                }
                Ok::<_, anyhow::Error>(())
            })
        })
        .await??;
        Ok(())
    }

    // ── Pins ─────────────────────────────────────────────────────────────────

    async fn put_pin(&self, pin: &crate::model::Pin) -> Result<()> {
        let key = pin.id.as_bytes().to_vec();
        let value = encode(pin)?;
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.with_write(|e| e.put(NS_PINS, &key, &value)))
            .await?
            .context("put_pin failed")?;
        Ok(())
    }

    async fn delete_pin(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.with_write(|e| e.delete(NS_PINS, &key)))
            .await?
            .context("delete_pin failed")?;
        Ok(())
    }

    async fn list_pins(&self) -> Result<Vec<crate::model::Pin>> {
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || engine.with_read(|e| e.prefix(NS_PINS, b"")))
            .await??;
        pairs.into_iter().map(|(_, v)| decode(&v)).collect()
    }

    // ── Sponsored ─────────────────────────────────────────────────────────────

    async fn put_sponsored(&self, slot: &crate::model::SponsoredSlot) -> Result<()> {
        let key = slot.id.as_bytes().to_vec();
        let value = encode(slot)?;
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.with_write(|e| e.put(NS_SPONSORED, &key, &value)))
            .await?
            .context("put_sponsored failed")?;
        Ok(())
    }

    async fn delete_sponsored(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.with_write(|e| e.delete(NS_SPONSORED, &key)))
            .await?
            .context("delete_sponsored failed")?;
        Ok(())
    }

    async fn list_sponsored(&self) -> Result<Vec<crate::model::SponsoredSlot>> {
        let engine = self.engine.clone();
        let pairs =
            tokio::task::spawn_blocking(move || engine.with_read(|e| e.prefix(NS_SPONSORED, b"")))
                .await??;
        pairs.into_iter().map(|(_, v)| decode(&v)).collect()
    }

    // ── Suppressions ──────────────────────────────────────────────────────────

    async fn put_suppression(&self, sup: &crate::model::Suppression) -> Result<()> {
        let key = sup.id.as_bytes().to_vec();
        let value = encode(sup)?;
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            engine.with_write(|e| e.put(NS_SUPPRESSIONS, &key, &value))
        })
        .await?
        .context("put_suppression failed")?;
        Ok(())
    }

    async fn delete_suppression(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.with_write(|e| e.delete(NS_SUPPRESSIONS, &key)))
            .await?
            .context("delete_suppression failed")?;
        Ok(())
    }

    async fn list_suppressions(&self) -> Result<Vec<crate::model::Suppression>> {
        let engine = self.engine.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            engine.with_read(|e| e.prefix(NS_SUPPRESSIONS, b""))
        })
        .await??;
        pairs.into_iter().map(|(_, v)| decode(&v)).collect()
    }
}

/// Extract simple scalar metadata fields as EdgeStore facets.
/// Skips nested objects, arrays, and null — those can't be faceted.
fn extract_facets(metadata: &serde_json::Value) -> HashMap<String, FacetValue> {
    let Some(obj) = metadata.as_object() else { return HashMap::new() };
    obj.iter()
        .filter_map(|(k, v)| {
            let fv = match v {
                serde_json::Value::String(s) => FacetValue::String(s.clone()),
                serde_json::Value::Bool(b) => FacetValue::Bool(*b),
                serde_json::Value::Number(n) => FacetValue::Number(n.as_i64()?),
                _ => return None,
            };
            Some((k.clone(), fv))
        })
        .collect()
}

/// Convert search-request filters to EdgeStore FacetFilters.
/// Skips range-style keys (`price_min`, `price_max`) — those are handled
/// post-search by `matches_filters()`. Skips non-scalar filter values.
fn to_facet_filters(filters: &HashMap<String, serde_json::Value>) -> Vec<FacetFilter> {
    filters
        .iter()
        .filter(|(k, _)| *k != "price_min" && *k != "price_max")
        .filter_map(|(k, v)| {
            let fv = match v {
                serde_json::Value::String(s) => FacetValue::String(s.clone()),
                serde_json::Value::Bool(b) => FacetValue::Bool(*b),
                serde_json::Value::Number(n) => FacetValue::Number(n.as_i64()?),
                _ => return None,
            };
            Some(FacetFilter { field: k.clone(), value: fv })
        })
        .collect()
}
