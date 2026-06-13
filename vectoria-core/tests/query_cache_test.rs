/// Tests for the TTL-bounded head query result cache.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Product, ProductStatus, RankingWeights, SearchMode, SearchRequest},
    search::SearchEngine,
    storage::memory::MemoryStorage,
    vector::memory::MemoryVectorIndex,
};

struct CountingEmbedding(Arc<AtomicUsize>);

#[async_trait]
impl EmbeddingProvider for CountingEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let b = text.as_bytes();
        Ok((0..32usize).map(|i| b.get(i % b.len().max(1)).copied().unwrap_or(0) as f32).collect())
    }
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::new();
        for t in texts { out.push(self.embed(t).await?); }
        Ok(out)
    }
    fn model_id(&self) -> &str { "counter-32" }
    fn dims(&self) -> usize { 32 }
}

fn make_engine_with_cache(embed_calls: Arc<AtomicUsize>) -> SearchEngine {
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("counter-32".into()), Some(32)));
    let embedding = Arc::new(CountingEmbedding(embed_calls));
    SearchEngine::new(storage, vidx, embedding, RankingWeights::default())
        .with_query_cache(60, 100)
}

fn make_product(id: &str, title: &str) -> Product {
    let now = Utc::now();
    Product {
        id: id.to_string(),
        text: Some(title.to_string()),
        vector: None,
        metadata: serde_json::json!({"title": title}),
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn test_query_cache_second_call_skips_embedding() {
    let embed_calls = Arc::new(AtomicUsize::new(0));
    let engine = make_engine_with_cache(Arc::clone(&embed_calls));

    engine.index(make_product("qc1", "Running Shoes")).await.unwrap();
    let calls_after_index = embed_calls.load(Ordering::SeqCst);

    let req = || SearchRequest {
        q: "running".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    };

    engine.search(req()).await.unwrap();
    let calls_after_first = embed_calls.load(Ordering::SeqCst);
    assert_eq!(calls_after_first, calls_after_index + 1, "first search must embed query");

    // Second search — identical request, should hit cache.
    engine.search(req()).await.unwrap();
    let calls_after_second = embed_calls.load(Ordering::SeqCst);
    assert_eq!(calls_after_second, calls_after_first, "cache hit must not call embed again");
}

#[tokio::test]
async fn test_query_cache_different_queries_not_shared() {
    let embed_calls = Arc::new(AtomicUsize::new(0));
    let engine = make_engine_with_cache(Arc::clone(&embed_calls));
    engine.index(make_product("qc2", "Yoga Mat")).await.unwrap();
    let after_index = embed_calls.load(Ordering::SeqCst);

    let search = |q: &str| SearchRequest {
        q: q.to_string(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    };

    engine.search(search("yoga")).await.unwrap();
    engine.search(search("mat")).await.unwrap();
    // Two distinct queries → two embed calls.
    assert_eq!(embed_calls.load(Ordering::SeqCst), after_index + 2);
}

#[tokio::test]
async fn test_explain_not_cached() {
    let embed_calls = Arc::new(AtomicUsize::new(0));
    let engine = make_engine_with_cache(Arc::clone(&embed_calls));
    engine.index(make_product("qc3", "Coffee Mug")).await.unwrap();
    let after_index = embed_calls.load(Ordering::SeqCst);

    let req = || SearchRequest {
        q: "coffee".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: true,  // explain=true must bypass cache
        rerank: false,
    };

    engine.search(req()).await.unwrap();
    engine.search(req()).await.unwrap();
    // Both calls should embed — explain requests are not cached.
    assert_eq!(embed_calls.load(Ordering::SeqCst), after_index + 2, "explain must not be cached");
}
