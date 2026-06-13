/// Integration tests using real EdgeStore storage and vector index.
/// Exercises the full persistence path on disk.
use std::sync::Arc;
use tempfile::TempDir;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Product, ProductStatus, RankingWeights, SearchMode, SearchRequest},
    search::SearchEngine,
    storage::edgestore::EdgeStoreStorage,
    vector::edgestore::EdgeStoreVectorIndex,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

// Deterministic stub embedding — no model download needed.
struct StubEmbedding384;

#[async_trait]
impl EmbeddingProvider for StubEmbedding384 {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let bytes = text.as_bytes();
        Ok((0..384usize)
            .map(|i| {
                let b = bytes.get(i % bytes.len().max(1)).copied().unwrap_or(0) as f32;
                (b * 3.14159 + i as f32).sin()
            })
            .collect())
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::new();
        for t in texts { out.push(self.embed(t).await?); }
        Ok(out)
    }

    fn model_id(&self) -> &str { "stub-384" }
    fn dims(&self) -> usize { 384 }
}

fn make_product(id: &str, title: &str) -> Product {
    let now = Utc::now();
    Product {
        id: id.to_string(),
        text: Some(title.to_string()),
        vector: None,
        metadata: serde_json::json!({ "title": title, "in_stock": true }),
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    }
}

fn make_engine(dir: &TempDir) -> SearchEngine {
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    let vidx = Arc::new(
        EdgeStoreVectorIndex::open(
            dir.path().join("vectors.db"),
            Some("stub-384".into()),
            Some(384),
        ).unwrap(),
    );
    let embedding = Arc::new(StubEmbedding384);
    SearchEngine::new(storage, vidx, embedding, RankingWeights::default())
}

#[tokio::test]
async fn test_edgestore_index_and_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir);

    engine.index(make_product("es1", "Nike Running Shoe")).await.unwrap();
    engine.index(make_product("es2", "Apple AirPods")).await.unwrap();
    engine.index(make_product("es3", "Adidas Running Shoe")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "running shoe".into(),
        limit: 10,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.total > 0, "should find results for 'running shoe'");
    let ids: Vec<&str> = resp.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"es1") || ids.contains(&"es3"), "running shoe products must appear");
}

#[tokio::test]
async fn test_edgestore_delete_persists() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir);

    engine.index(make_product("del1", "Temporary Product")).await.unwrap();
    engine.delete("del1").await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Temporary Product".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Bm25,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(!resp.hits.iter().any(|h| h.id == "del1"),
        "deleted product must not appear in BM25 results");
}

#[tokio::test]
async fn test_edgestore_bm25_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir);

    engine.index(make_product("bm1", "Bluetooth Wireless Headphones")).await.unwrap();
    engine.index(make_product("bm2", "USB-C Charging Cable")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Bluetooth".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Bm25,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "bm1"),
        "BM25 must match 'Bluetooth Headphones' on keyword 'Bluetooth'");
    assert!(!resp.hits.iter().any(|h| h.id == "bm2"),
        "USB cable must not match 'Bluetooth'");
}

#[tokio::test]
async fn test_edgestore_vector_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir);

    engine.index(make_product("vs1", "Yoga Mat for Fitness")).await.unwrap();
    engine.index(make_product("vs2", "Coffee Mug Ceramic")).await.unwrap();
    engine.index(make_product("vs3", "Fitness Exercise Yoga Block")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "yoga fitness".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Semantic,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.total > 0, "semantic search should return results");
}
