/// Tests for the foyer-backed embedding cache layer.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use anyhow::Result;
use async_trait::async_trait;
use vectoria_core::embedding::{cache::CachedEmbedding, EmbeddingProvider};

struct CountingEmbedding {
    call_count: Arc<AtomicUsize>,
    dims: usize,
}

#[async_trait]
impl EmbeddingProvider for CountingEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let bytes = text.as_bytes();
        Ok((0..self.dims)
            .map(|i| bytes.get(i % bytes.len().max(1)).copied().unwrap_or(0) as f32)
            .collect())
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.call_count.fetch_add(texts.len(), Ordering::SeqCst);
        let mut out = Vec::new();
        for t in texts { out.push(self.embed(t).await?); }
        Ok(out)
    }

    fn model_id(&self) -> &str { "counting-stub" }
    fn dims(&self) -> usize { self.dims }
}

#[tokio::test]
async fn test_cache_hits_skip_inner_embed() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let inner = Arc::new(CountingEmbedding { call_count: Arc::clone(&call_count), dims: 32 });
    let cached = CachedEmbedding::new(inner, 1000);

    // First call: cache miss, inner called once.
    let v1 = cached.embed("running shoes").await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second call: same text, cache hit, inner NOT called again.
    let v2 = cached.embed("running shoes").await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1, "cache should short-circuit inner");

    // Results must be identical.
    assert_eq!(v1, v2);
}

#[tokio::test]
async fn test_cache_different_queries_both_embedded() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let inner = Arc::new(CountingEmbedding { call_count: Arc::clone(&call_count), dims: 32 });
    let cached = CachedEmbedding::new(inner, 1000);

    cached.embed("query one").await.unwrap();
    cached.embed("query two").await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2, "distinct queries must each call inner");

    // Repeat both — both should hit cache.
    cached.embed("query one").await.unwrap();
    cached.embed("query two").await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2, "repeated queries must be cached");
}

#[tokio::test]
async fn test_cache_model_id_and_dims_delegated() {
    let inner = Arc::new(CountingEmbedding { call_count: Arc::new(AtomicUsize::new(0)), dims: 128 });
    let cached = CachedEmbedding::new(inner, 100);
    assert_eq!(cached.model_id(), "counting-stub");
    assert_eq!(cached.dims(), 128);
}
