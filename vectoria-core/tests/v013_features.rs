/// Integration tests for v0.1.13 features:
///   - Product relationship graph (brand + co-purchased relations)
///   - Two-tower retrieval (separate query embedder)
#[allow(dead_code)]
mod common;

use std::sync::Arc;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Event, EventType, RelationType, SearchMode, SearchRequest},
    storage::{memory::MemoryStorage, StorageEngine},
    SearchEngineBuilder,
};

// Helper: build engine + expose shared storage for aggregation tests.
async fn make_engine_with_storage(
    dims: usize,
) -> (vectoria_core::SearchEngine, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let embed: Arc<dyn EmbeddingProvider> = Arc::new(common::StubEmbedding::new(dims));
    let engine = SearchEngineBuilder::new()
        .storage(Arc::clone(&storage) as Arc<dyn StorageEngine>)
        .embedding(Arc::clone(&embed))
        .build()
        .await
        .unwrap();
    (engine, storage)
}

// ── Product Relationship Graph ────────────────────────────────────────────────

#[tokio::test]
async fn test_related_products_empty_for_unknown_product() {
    let (engine, _) = make_engine_with_storage(16).await;
    let related = engine.related_products("ghost_product", None, 10).await.unwrap();
    assert!(related.is_empty());
}

#[tokio::test]
async fn test_brand_relations_populated_by_aggregation() {
    let (engine, storage) = make_engine_with_storage(16).await;

    let mut p1 = common::make_product("shoe1", "Nike Air Max");
    p1.metadata = serde_json::json!({"brand": "Nike", "title": "Nike Air Max"});
    let mut p2 = common::make_product("shoe2", "Nike Free Run");
    p2.metadata = serde_json::json!({"brand": "Nike", "title": "Nike Free Run"});
    let mut p3 = common::make_product("chair1", "Office Chair");
    p3.metadata = serde_json::json!({"brand": "Acme", "title": "Office Chair"});

    engine.index(p1).await.unwrap();
    engine.index(p2).await.unwrap();
    engine.index(p3).await.unwrap();

    vectoria_core::aggregation::aggregate_once_for_test(
        Arc::clone(&storage) as Arc<dyn StorageEngine>
    ).await.unwrap();

    let related = engine.related_products("shoe1", Some("brand"), 10).await.unwrap();
    let ids: Vec<&str> = related.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"shoe2"), "shoe1 should relate to shoe2 via brand; got: {:?}", ids);
    assert!(!ids.contains(&"chair1"), "shoe1 should not relate to different-brand chair1");

    for r in &related {
        assert_eq!(r.relation_type, RelationType::Brand);
        assert!(r.score > 0.0 && r.score <= 1.0);
    }
}

#[tokio::test]
async fn test_co_purchase_relations_populated_by_aggregation() {
    let (engine, storage) = make_engine_with_storage(16).await;

    engine.index(common::make_product("p1", "product one")).await.unwrap();
    engine.index(common::make_product("p2", "product two")).await.unwrap();
    engine.index(common::make_product("p3", "product three")).await.unwrap();

    let mut ev1 = Event::new(EventType::Click, "p1");
    ev1.user_id = Some("u1".into());
    engine.record_event(ev1).await.unwrap();

    let mut ev2 = Event::new(EventType::Click, "p2");
    ev2.user_id = Some("u1".into());
    engine.record_event(ev2).await.unwrap();

    let mut ev3 = Event::new(EventType::Click, "p1");
    ev3.user_id = Some("u2".into());
    engine.record_event(ev3).await.unwrap();

    let mut ev4 = Event::new(EventType::Click, "p3");
    ev4.user_id = Some("u2".into());
    engine.record_event(ev4).await.unwrap();

    vectoria_core::aggregation::aggregate_once_for_test(
        Arc::clone(&storage) as Arc<dyn StorageEngine>
    ).await.unwrap();

    let related = engine.related_products("p1", Some("co_purchased"), 10).await.unwrap();
    let ids: Vec<&str> = related.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"p2"), "p1 should relate to p2 via co_purchased; got: {:?}", ids);
    assert!(ids.contains(&"p3"), "p1 should relate to p3 via co_purchased; got: {:?}", ids);

    for r in &related {
        assert_eq!(r.relation_type, RelationType::CoPurchased);
    }
}

#[tokio::test]
async fn test_related_type_filter_isolates_results() {
    let (engine, storage) = make_engine_with_storage(16).await;

    let mut p1 = common::make_product("a1", "product a1");
    p1.metadata = serde_json::json!({"brand": "BrandX", "title": "product a1"});
    let mut p2 = common::make_product("a2", "product a2");
    p2.metadata = serde_json::json!({"brand": "BrandX", "title": "product a2"});
    engine.index(p1).await.unwrap();
    engine.index(p2).await.unwrap();

    vectoria_core::aggregation::aggregate_once_for_test(
        Arc::clone(&storage) as Arc<dyn StorageEngine>
    ).await.unwrap();

    let brand = engine.related_products("a1", Some("brand"), 10).await.unwrap();
    let co = engine.related_products("a1", Some("co_purchased"), 10).await.unwrap();
    let all = engine.related_products("a1", None, 10).await.unwrap();

    assert!(!brand.is_empty(), "brand filter should return brand relations");
    assert!(co.is_empty(), "no events → no co_purchased relations");
    assert!(all.len() >= brand.len());
}

// ── Two-Tower Retrieval ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_two_tower_query_embedder_is_used() {
    struct ZeroEmbedder;
    #[async_trait::async_trait]
    impl EmbeddingProvider for ZeroEmbedder {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            Ok(vec![0.0f32; 16])
        }
        async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.0f32; 16]).collect())
        }
        fn model_id(&self) -> &str { "zero-embedder" }
        fn dims(&self) -> usize { 16 }
    }

    let storage = Arc::new(MemoryStorage::new());
    let product_embed: Arc<dyn EmbeddingProvider> = Arc::new(common::StubEmbedding::new(16));
    let query_embed: Arc<dyn EmbeddingProvider> = Arc::new(ZeroEmbedder);

    let engine = SearchEngineBuilder::new()
        .storage(Arc::clone(&storage) as Arc<dyn StorageEngine>)
        .embedding(Arc::clone(&product_embed))
        .with_query_embedder(query_embed)
        .build()
        .await
        .unwrap();

    engine.index(common::make_product("p1", "running shoe")).await.unwrap();
    engine.index(common::make_product("p2", "hiking boot")).await.unwrap();

    // Zero-vector query has no similarity to any product — search must not error.
    let resp = engine.search(SearchRequest {
        q: "shoe".into(),
        limit: 10,
        mode: SearchMode::Semantic,
        ..Default::default()
    }).await;
    assert!(resp.is_ok(), "two-tower search should not error; got: {:?}", resp);
}

#[tokio::test]
async fn test_two_tower_fallback_without_query_embedder() {
    // Without a query embedder set, falls back to the product embedder — should work normally.
    let (engine, _embed) = common::make_engine(16).await;
    engine.index(common::make_product("p1", "running shoe")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "shoe".into(),
        limit: 10,
        ..Default::default()
    }).await;
    assert!(resp.is_ok());
    assert!(!resp.unwrap().hits.is_empty());
}
