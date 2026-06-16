#[allow(dead_code)]
mod common;

use std::sync::Arc;
use tempfile::TempDir;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{RankingWeights, SearchMode, SearchRequest},
    search::SearchEngine,
    storage::edgestore::EdgeStoreStorage,
    vector::edgestore::EdgeStoreVectorIndex,
};

fn make_engine(dir: &TempDir) -> SearchEngine {
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    let vidx = Arc::new(
        EdgeStoreVectorIndex::open(
            dir.path().join("vectors.db"),
            Some("stub".into()),
            Some(384),
        ).unwrap(),
    );
    let embedding: Arc<dyn EmbeddingProvider> = Arc::new(common::StubEmbedding::new(384));
    SearchEngine::new(storage, vidx, embedding, RankingWeights::default())
}

#[tokio::test]
async fn test_edgestore_index_and_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir);

    engine.index(common::make_product("es1", "Nike Running Shoe")).await.unwrap();
    engine.index(common::make_product("es2", "Apple AirPods")).await.unwrap();
    engine.index(common::make_product("es3", "Adidas Running Shoe")).await.unwrap();

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

    engine.index(common::make_product("del1", "Temporary Product")).await.unwrap();
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

    engine.index(common::make_product("bm1", "Bluetooth Wireless Headphones")).await.unwrap();
    engine.index(common::make_product("bm2", "USB-C Charging Cable")).await.unwrap();

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

    engine.index(common::make_product("vs1", "Yoga Mat for Fitness")).await.unwrap();
    engine.index(common::make_product("vs2", "Coffee Mug Ceramic")).await.unwrap();
    engine.index(common::make_product("vs3", "Fitness Exercise Yoga Block")).await.unwrap();

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
