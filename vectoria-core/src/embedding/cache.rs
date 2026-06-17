use super::EmbeddingProvider;
use anyhow::Result;
use async_trait::async_trait;
use foyer::{Cache, CacheBuilder, LruConfig};
use std::sync::{Arc, Mutex};

pub struct CachedEmbedding {
    inner: Arc<dyn EmbeddingProvider>,
    cache: Mutex<Cache<String, Arc<Vec<f32>>>>,
}

impl CachedEmbedding {
    pub fn new(inner: Arc<dyn EmbeddingProvider>, capacity: usize) -> Self {
        let cache = CacheBuilder::new(capacity)
            .with_eviction_config(LruConfig {
                high_priority_pool_ratio: 0.1,
            })
            .build();
        Self { inner, cache: Mutex::new(cache) }
    }
}

#[async_trait]
impl EmbeddingProvider for CachedEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(text) {
                return Ok(entry.value().as_ref().clone());
            }
        }
        let vector = self.inner.embed(text).await?;
        {
            let cache = self.cache.lock().unwrap();
            // Re-check: a concurrent task may have computed and inserted the same text
            // while we were awaiting. Prefer the already-cached value to avoid a
            // redundant insert and keep cache entry lifetimes consistent.
            if let Some(entry) = cache.get(text) {
                return Ok(entry.value().as_ref().clone());
            }
            cache.insert(text.to_string(), Arc::new(vector.clone()));
        }
        Ok(vector)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        let mut miss_indices: Vec<usize> = Vec::new();
        let mut miss_texts: Vec<&str> = Vec::new();

        {
            let cache = self.cache.lock().unwrap();
            for (i, text) in texts.iter().enumerate() {
                if let Some(entry) = cache.get(*text) {
                    results.push((i, entry.value().as_ref().clone()));
                } else {
                    miss_indices.push(i);
                    miss_texts.push(text);
                    results.push((i, vec![]));
                }
            }
        }

        if !miss_texts.is_empty() {
            let embedded = self.inner.embed_batch(&miss_texts).await?;
            let cache = self.cache.lock().unwrap();
            for (j, (orig_idx, vec)) in miss_indices.iter().zip(embedded.into_iter()).enumerate() {
                cache.insert(miss_texts[j].to_string(), Arc::new(vec.clone()));
                results[*orig_idx] = (*orig_idx, vec);
            }
        }

        results.sort_by_key(|(i, _)| *i);
        Ok(results.into_iter().map(|(_, v)| v).collect())
    }

    fn model_id(&self) -> &str { self.inner.model_id() }
    fn dims(&self) -> usize { self.inner.dims() }
}
