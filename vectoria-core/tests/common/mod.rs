use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Product, ProductStatus, RankingWeights},
    search::SearchEngine,
    storage::memory::MemoryStorage,
    vector::memory::MemoryVectorIndex,
};

pub struct StubEmbedding {
    pub dims: usize,
    pub calls: Arc<AtomicUsize>,
}

impl StubEmbedding {
    pub fn new(dims: usize) -> Self {
        Self { dims, calls: Arc::new(AtomicUsize::new(0)) }
    }

    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn hash_vec(text: &str, dims: usize) -> Vec<f32> {
        let bytes = text.as_bytes();
        (0..dims)
            .map(|i| {
                let b1 = bytes.get(i % bytes.len().max(1)).copied().unwrap_or(0) as f32;
                let b2 = bytes.get((i * 7) % bytes.len().max(1)).copied().unwrap_or(0) as f32;
                (b1 * 3.14159 + b2 * 2.71828 + i as f32).sin()
            })
            .collect()
    }
}

#[async_trait]
impl EmbeddingProvider for StubEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Self::hash_vec(text, self.dims))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.calls.fetch_add(texts.len(), Ordering::SeqCst);
        Ok(texts.iter().map(|t| Self::hash_vec(t, self.dims)).collect())
    }

    fn model_id(&self) -> &str { "stub" }
    fn dims(&self) -> usize { self.dims }
}

pub fn make_engine(dims: usize) -> (SearchEngine, Arc<StubEmbedding>) {
    let embed = Arc::new(StubEmbedding::new(dims));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(dims)));
    let embed_dyn: Arc<dyn EmbeddingProvider> = Arc::clone(&embed) as Arc<dyn EmbeddingProvider>;
    let engine = SearchEngine::new(storage, vidx, embed_dyn, RankingWeights::default());
    (engine, embed)
}

pub fn make_engine_with_cache(dims: usize) -> (SearchEngine, Arc<StubEmbedding>) {
    let embed = Arc::new(StubEmbedding::new(dims));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(dims)));
    let embed_dyn: Arc<dyn EmbeddingProvider> = Arc::clone(&embed) as Arc<dyn EmbeddingProvider>;
    let engine = SearchEngine::new(storage, vidx, embed_dyn, RankingWeights::default())
        .with_query_cache(60, 1000);
    (engine, embed)
}

pub fn make_product(id: &str, title: &str) -> Product {
    let now = Utc::now();
    Product {
        id: id.to_string(),
        text: Some(title.to_string()),
        vector: None,
        metadata: serde_json::json!({"title": title, "in_stock": true, "price": 9.99}),
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    }
}
