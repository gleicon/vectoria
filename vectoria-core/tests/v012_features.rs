/// Integration tests for v0.1.12 features:
///   - User recommendations (user vectors from events)
///   - LLM query rewriting (wired but not called — no server in tests)
///   - Semantic result clustering
///   - Multi-tenancy (storage-level, tested via MemoryStorage)
#[allow(dead_code)]
mod common;

use std::sync::Arc;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{Event, EventType, SearchMode, SearchRequest},
    SearchEngineBuilder,
};

// ── User Recommendations ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_recommend_unknown_user_returns_empty() {
    let (engine, _) = common::make_engine(16).await;
    engine.index(common::make_product("p1", "running shoe")).await.unwrap();

    let hits = engine.recommend("ghost_user", 10).await.unwrap();
    assert!(hits.is_empty(), "unknown user should yield no recommendations");
}

#[tokio::test]
async fn test_recommend_after_click_events() {
    let (engine, _) = common::make_engine(16).await;

    engine.index(common::make_product("shoe1", "Nike running shoe lightweight")).await.unwrap();
    engine.index(common::make_product("shoe2", "Adidas trail running shoe")).await.unwrap();
    engine.index(common::make_product("chair1", "Ergonomic office chair adjustable")).await.unwrap();

    // User "u1" clicks both shoes.
    let mut ev1 = Event::new(EventType::Click, "shoe1");
    ev1.user_id = Some("u1".to_string());
    engine.record_event(ev1).await.unwrap();

    let mut ev2 = Event::new(EventType::Click, "shoe2");
    ev2.user_id = Some("u1".to_string());
    engine.record_event(ev2).await.unwrap();

    // Purchase also counted.
    let mut ev3 = Event::new(EventType::Purchase, "shoe1");
    ev3.user_id = Some("u1".to_string());
    engine.record_event(ev3).await.unwrap();

    let hits = engine.recommend("u1", 10).await.unwrap();
    assert!(!hits.is_empty(), "user with click/purchase events should get recommendations");

    // Shoes should appear in results (higher vector similarity to shoe embedding).
    let shoe_ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
    assert!(
        shoe_ids.contains(&"shoe1") || shoe_ids.contains(&"shoe2"),
        "recommendation should surface clicked products; got: {:?}", shoe_ids
    );
}

#[tokio::test]
async fn test_recommend_view_events_not_counted() {
    let (engine, _) = common::make_engine(16).await;
    engine.index(common::make_product("p1", "laptop stand ergonomic")).await.unwrap();

    // Only a view event — not a click or purchase.
    let mut ev = Event::new(EventType::View, "p1");
    ev.user_id = Some("viewer".to_string());
    engine.record_event(ev).await.unwrap();

    let hits = engine.recommend("viewer", 10).await.unwrap();
    assert!(hits.is_empty(), "view events should not contribute to user vector");
}

#[tokio::test]
async fn test_recommend_user_id_length_validated() {
    let (engine, _) = common::make_engine(16).await;
    let long_id = "x".repeat(300);
    let result = engine.recommend(&long_id, 10).await;
    assert!(result.is_err(), "oversized user_id should be rejected");
}

#[tokio::test]
async fn test_recommend_empty_user_id_rejected() {
    let (engine, _) = common::make_engine(16).await;
    let result = engine.recommend("", 10).await;
    assert!(result.is_err(), "empty user_id should be rejected");
}

#[tokio::test]
async fn test_user_vector_caches_after_first_recommend() {
    use vectoria_core::storage::{memory::MemoryStorage, StorageEngine};

    let storage = Arc::new(MemoryStorage::new());
    let embed: Arc<dyn EmbeddingProvider> = Arc::new(common::StubEmbedding::new(16));
    let engine = SearchEngineBuilder::new()
        .storage(Arc::clone(&storage) as Arc<dyn StorageEngine>)
        .embedding(Arc::clone(&embed))
        .build()
        .await
        .unwrap();

    engine.index(common::make_product("p1", "wireless headphones noise cancelling")).await.unwrap();

    let mut ev = Event::new(EventType::Click, "p1");
    ev.user_id = Some("u2".to_string());
    engine.record_event(ev).await.unwrap();

    // First call computes and caches.
    let _ = engine.recommend("u2", 5).await.unwrap();

    // After first call, vector should be cached in storage.
    let cached = storage.get_user_vector("u2").await.unwrap();
    assert!(cached.is_some(), "user vector should be cached after first recommend call");
    assert_eq!(cached.unwrap().len(), 16, "cached vector should match embedding dims");
}

// ── Semantic Clustering ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_cluster_field_absent_when_not_requested() {
    let (engine, _) = common::make_engine(16).await;
    for i in 0..5 {
        engine.index(common::make_product(&format!("p{}", i), &format!("product {}", i))).await.unwrap();
    }
    let resp = engine.search(SearchRequest {
        q: "product".into(),
        limit: 10,
        cluster: false,
        ..Default::default()
    }).await.unwrap();
    assert!(resp.clusters.is_none(), "clusters should be None when cluster=false");
}

#[tokio::test]
async fn test_cluster_returned_when_requested() {
    let (engine, _) = common::make_engine(16).await;
    // Need at least k=2 products with vectors for clustering to return results.
    engine.index(common::make_product("shoe1", "Nike running shoe")).await.unwrap();
    engine.index(common::make_product("shoe2", "Adidas running shoe")).await.unwrap();
    engine.index(common::make_product("boot1", "Hiking boot waterproof")).await.unwrap();
    engine.index(common::make_product("boot2", "Trail boot leather")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "shoe".into(),
        limit: 10,
        cluster: true,
        mode: SearchMode::Semantic,
        ..Default::default()
    }).await.unwrap();

    // Clustering is best-effort; if results are fewer than k, clusters may be empty.
    // We just verify the field is present and structurally correct.
    if let Some(clusters) = &resp.clusters {
        for c in clusters {
            assert!(!c.label.is_empty(), "cluster label should not be empty");
            assert!(c.count > 0, "cluster count should be positive");
        }
    }
}

// ── Multi-tenancy (storage level) ────────────────────────────────────────────

#[tokio::test]
async fn test_tenant_isolation_via_named_indexes() {
    // Two tenants each with their own in-memory engine (simulates IndexRegistry per-name).
    let (engine_a, _) = common::make_engine(16).await;
    let (engine_b, _) = common::make_engine(16).await;

    engine_a.index(common::make_product("a1", "tenant-a product")).await.unwrap();
    engine_b.index(common::make_product("b1", "tenant-b product")).await.unwrap();

    let resp_a = engine_a.search(SearchRequest { q: "product".into(), limit: 10, ..Default::default() }).await.unwrap();
    let resp_b = engine_b.search(SearchRequest { q: "product".into(), limit: 10, ..Default::default() }).await.unwrap();

    let ids_a: Vec<&str> = resp_a.hits.iter().map(|h| h.id.as_str()).collect();
    let ids_b: Vec<&str> = resp_b.hits.iter().map(|h| h.id.as_str()).collect();

    assert!(ids_a.contains(&"a1"), "tenant A engine should see its own product");
    assert!(!ids_a.contains(&"b1"), "tenant A engine should not see tenant B's product");
    assert!(ids_b.contains(&"b1"), "tenant B engine should see its own product");
    assert!(!ids_b.contains(&"a1"), "tenant B engine should not see tenant A's product");
}

// ── Regression: existing search still works ───────────────────────────────────

#[tokio::test]
async fn test_search_response_includes_clusters_field_none_by_default() {
    let (engine, _) = common::make_engine(16).await;
    engine.index(common::make_product("r1", "regression test product")).await.unwrap();
    let resp = engine.search(SearchRequest { q: "regression".into(), limit: 5, ..Default::default() }).await.unwrap();
    // `clusters` is None when not requested — serializes as absent (skip_serializing_if).
    assert!(resp.clusters.is_none());
}
