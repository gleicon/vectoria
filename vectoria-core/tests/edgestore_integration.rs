#[allow(dead_code)]
mod common;

use std::sync::Arc;
use tempfile::TempDir;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Event, EventType, SearchMode, SearchRequest},
    search::SearchEngine,
    storage::edgestore::EdgeStoreStorage,
    vector::edgestore::EdgeStoreVectorIndex,
    SearchEngineBuilder,
};

async fn make_engine(dir: &TempDir) -> SearchEngine {
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
    SearchEngineBuilder::new()
        .storage(storage)
        .vector_index(vidx)
        .embedding(embedding)
        .build()
        .await
        .unwrap()
}

#[tokio::test]
async fn test_edgestore_index_and_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir).await;

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
        rerank: false, cluster: false,
    }).await.unwrap();

    assert!(resp.total > 0, "should find results for 'running shoe'");
    let ids: Vec<&str> = resp.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"es1") || ids.contains(&"es3"), "running shoe products must appear");
}

#[tokio::test]
async fn test_edgestore_delete_persists() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir).await;

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
        rerank: false, cluster: false,
    }).await.unwrap();

    assert!(!resp.hits.iter().any(|h| h.id == "del1"),
        "deleted product must not appear in BM25 results");
}

#[tokio::test]
async fn test_edgestore_bm25_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir).await;

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
        rerank: false, cluster: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "bm1"),
        "BM25 must match 'Bluetooth Headphones' on keyword 'Bluetooth'");
    assert!(!resp.hits.iter().any(|h| h.id == "bm2"),
        "USB cable must not match 'Bluetooth'");
}

#[tokio::test]
async fn test_edgestore_vector_search() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir).await;

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
        rerank: false, cluster: false,
    }).await.unwrap();

    assert!(resp.total > 0, "semantic search should return results");
}

#[tokio::test]
async fn test_edgestore_query_ctr_boosts_clicked_product() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(&dir).await;

    engine.index(common::make_product("ctr1", "Running Shoe Lightweight")).await.unwrap();
    engine.index(common::make_product("ctr2", "Running Shoe Waterproof")).await.unwrap();
    engine.index(common::make_product("ctr3", "Yoga Mat")).await.unwrap();

    for _ in 0..2 {
        let mut ev = Event::new(EventType::Click, "ctr1");
        ev.query = Some("running shoe".to_string());
        engine.record_event(ev).await.unwrap();
    }

    let resp = engine.search(SearchRequest {
        q: "running shoe".into(),
        limit: 10,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false, cluster: false,
    }).await.unwrap();

    let ids: Vec<&str> = resp.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"ctr1"), "clicked product must appear");
    let pos1 = ids.iter().position(|&id| id == "ctr1").unwrap();
    let pos2 = ids.iter().position(|&id| id == "ctr2").unwrap_or(usize::MAX);
    assert!(pos1 < pos2, "clicked product ctr1 must rank above ctr2");
}

#[tokio::test]
async fn test_edgestore_query_ctr_view_events_ignored() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    use vectoria_core::storage::StorageEngine;

    let mut ev = Event::new(EventType::View, "v1");
    ev.query = Some("trail running".to_string());
    storage.put_event(&ev).await.unwrap();

    let ctrs = storage.get_query_ctrs("trail running").await.unwrap();
    assert!(ctrs.is_empty(), "view events must not update CTR counters");
}

#[tokio::test]
async fn test_edgestore_purchase_events_count_as_ctr() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    use vectoria_core::storage::StorageEngine;

    let mut ev = Event::new(EventType::Purchase, "prod1");
    ev.query = Some("blue sneakers".to_string());
    storage.put_event(&ev).await.unwrap();

    let ctrs = storage.get_query_ctrs("blue sneakers").await.unwrap();
    assert_eq!(ctrs.get("prod1").copied(), Some(1.0), "purchase must count as CTR");
}

#[tokio::test]
async fn test_edgestore_click_without_query_ignored_by_ctr() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    use vectoria_core::storage::StorageEngine;

    // Click with no query — must not update any CTR counter.
    let ev = Event::new(EventType::Click, "p1");
    assert!(ev.query.is_none());
    storage.put_event(&ev).await.unwrap();

    // Any query lookup must return empty.
    let ctrs = storage.get_query_ctrs("running shoes").await.unwrap();
    assert!(ctrs.is_empty(), "click with no query must not populate any CTR entry");
}

#[tokio::test]
async fn test_edgestore_oversized_query_not_stored_in_ctr() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    use vectoria_core::storage::StorageEngine;

    let long_query = "a".repeat(513);
    let mut ev = Event::new(EventType::Click, "p1");
    ev.query = Some(long_query.clone());
    storage.put_event(&ev).await.unwrap();

    let ctrs = storage.get_query_ctrs(&long_query).await.unwrap();
    assert!(ctrs.is_empty(), "oversized query must not write or read CTR entries");
}

#[tokio::test]
async fn test_edgestore_query_ctr_no_cross_query_bleed() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(
        EdgeStoreStorage::open(dir.path().join("data.db")).unwrap(),
    );
    use vectoria_core::storage::StorageEngine;

    let mut ev = Event::new(EventType::Click, "p1");
    ev.query = Some("running shoes".to_string());
    storage.put_event(&ev).await.unwrap();

    let ctrs = storage.get_query_ctrs("yoga mat").await.unwrap();
    assert!(ctrs.is_empty(), "CTR for unrelated query must be empty");
}
