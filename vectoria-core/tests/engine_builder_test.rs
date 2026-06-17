#[allow(dead_code)]
mod common;

use common::{make_product, StubEmbedding};
use std::sync::Arc;
use vectoria_core::{
    SearchEngineBuilder, SearchEngineSync,
    model::{RankingWeights, SearchMode, SearchRequest, SimilarRequest},
    storage::memory::MemoryStorage,
    vector::memory::MemoryVectorIndex,
};

// ── SearchEngineBuilder ───────────────────────────────────────────────────────

#[tokio::test]
async fn builder_defaults_build_successfully() {
    let embed = Arc::new(StubEmbedding::new(32));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(32)));

    let engine = SearchEngineBuilder::new()
        .embedding(Arc::clone(&embed) as Arc<dyn vectoria_core::embedding::EmbeddingProvider>)
        .storage(Arc::clone(&storage) as Arc<dyn vectoria_core::storage::StorageEngine>)
        .vector_index(Arc::clone(&vidx) as Arc<dyn vectoria_core::vector::VectorIndex>)
        .build()
        .await
        .expect("builder with explicit deps must succeed");

    let stats = engine.stats().await.unwrap();
    assert_eq!(stats.product_count, 0);
}

#[tokio::test]
async fn builder_with_query_cache_indexes_and_searches() {
    let embed = Arc::new(StubEmbedding::new(32));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(32)));

    let engine = SearchEngineBuilder::new()
        .embedding(Arc::clone(&embed) as Arc<dyn vectoria_core::embedding::EmbeddingProvider>)
        .storage(Arc::clone(&storage) as Arc<dyn vectoria_core::storage::StorageEngine>)
        .vector_index(Arc::clone(&vidx) as Arc<dyn vectoria_core::vector::VectorIndex>)
        .query_cache(60, 100)
        .build()
        .await
        .unwrap();

    engine.index(make_product("b1", "Wireless Keyboard")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "keyboard".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "b1"));
}

#[tokio::test]
async fn builder_custom_weights_applied() {
    let embed = Arc::new(StubEmbedding::new(32));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(32)));
    let weights = RankingWeights { semantic: 1.0, ..RankingWeights::default() };

    let engine = SearchEngineBuilder::new()
        .embedding(Arc::clone(&embed) as Arc<dyn vectoria_core::embedding::EmbeddingProvider>)
        .storage(Arc::clone(&storage) as Arc<dyn vectoria_core::storage::StorageEngine>)
        .vector_index(Arc::clone(&vidx) as Arc<dyn vectoria_core::vector::VectorIndex>)
        .weights(weights)
        .build()
        .await
        .unwrap();

    engine.index(make_product("w1", "Yoga Mat")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "yoga".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Semantic,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "w1"));
}

// ── SearchEngineSync ──────────────────────────────────────────────────────────

fn make_sync_engine() -> SearchEngineSync {
    let embed = Arc::new(StubEmbedding::new(32));
    let storage = Arc::new(MemoryStorage::new());
    let vidx = Arc::new(MemoryVectorIndex::new(Some("stub".into()), Some(32)));

    SearchEngineSync::from_builder(
        SearchEngineBuilder::new()
            .embedding(Arc::clone(&embed) as Arc<dyn vectoria_core::embedding::EmbeddingProvider>)
            .storage(Arc::clone(&storage) as Arc<dyn vectoria_core::storage::StorageEngine>)
            .vector_index(Arc::clone(&vidx) as Arc<dyn vectoria_core::vector::VectorIndex>),
    ).expect("sync engine must build")
}

#[test]
fn sync_index_and_search() {
    let engine = make_sync_engine();
    engine.index(make_product("s1", "Running Shoes")).unwrap();

    let resp = engine.search(SearchRequest {
        q: "running".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "s1"));
}

#[test]
fn sync_delete() {
    let engine = make_sync_engine();
    engine.index(make_product("d1", "Temporary Widget")).unwrap();
    engine.delete("d1").unwrap();

    let resp = engine.search(SearchRequest {
        q: "Temporary Widget".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Bm25,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).unwrap();

    assert!(!resp.hits.iter().any(|h| h.id == "d1"), "deleted product must not appear");
}

#[test]
fn sync_similar() {
    let engine = make_sync_engine();
    engine.index(make_product("sim1", "Trail Running Shoe")).unwrap();
    engine.index(make_product("sim2", "Road Running Shoe")).unwrap();

    let hits = engine.similar(SimilarRequest {
        product_id: Some("sim1".into()),
        text: None,
        vector: None,
        limit: 5,
        filters: None,
    }).unwrap();

    assert!(!hits.is_empty(), "similar must return results");
}

#[test]
fn sync_stats() {
    let engine = make_sync_engine();
    engine.index(make_product("st1", "Product A")).unwrap();
    engine.index(make_product("st2", "Product B")).unwrap();

    let stats = engine.stats().unwrap();
    assert_eq!(stats.product_count, 2);
}

#[test]
fn sync_reindex() {
    let engine = make_sync_engine();
    engine.index(make_product("ri1", "Reindex Me")).unwrap();

    let report = engine.reindex().unwrap();
    assert_eq!(report.reindexed, 1);
    assert_eq!(report.errors, 0);
}
