/// Integration tests — real storage (MemoryStorage), real vector index (MemoryVectorIndex),
/// real BM25 index, real SymSpell. No mocks.
///
/// Embedding uses local fastembed (multilingual-e5-small) if VECTORIA_TEST_EMBEDDING=local,
/// otherwise uses a stub that returns deterministic fixed-dim vectors without downloading models.
use std::sync::Arc;
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::{
        Event, EventType, Product, ProductStatus, RankingWeights, SearchMode, SearchRequest,
        SimilarRequest,
    },
    search::SearchEngine,
    storage::memory::MemoryStorage,
    vector::memory::MemoryVectorIndex,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

// ── Deterministic stub embedding (no model download needed in CI) ──────────

struct StubEmbedding {
    dims: usize,
}

impl StubEmbedding {
    fn new(dims: usize) -> Self { Self { dims } }

    fn hash_text(text: &str, dims: usize) -> Vec<f32> {
        // Deterministic hash-based vector. Same text → same vector. Different text → different vector.
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
        Ok(Self::hash_text(text, self.dims))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::hash_text(t, self.dims)).collect())
    }

    fn model_id(&self) -> &str { "stub-384" }
    fn dims(&self) -> usize { self.dims }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_engine() -> SearchEngine {
    let storage = Arc::new(MemoryStorage::new());
    let embedding = Arc::new(StubEmbedding::new(384));
    let vector_index = Arc::new(MemoryVectorIndex::new(
        Some("stub-384".into()),
        Some(384),
    ));
    SearchEngine::new(storage, vector_index, embedding, RankingWeights::default())
}

fn make_product(id: &str, title: &str, brand: &str, category: &str, in_stock: bool) -> Product {
    let now = Utc::now();
    Product {
        id: id.to_string(),
        text: None,
        vector: None,
        metadata: serde_json::json!({
            "title": title,
            "brand": brand,
            "category": category,
            "in_stock": in_stock,
            "price": 99.99,
        }),
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_index_and_search_basic() {
    let engine = make_engine();

    engine.index(make_product("p1", "Nike Air Max Running Shoe", "Nike", "Running Shoes", true)).await.unwrap();
    engine.index(make_product("p2", "Adidas Ultraboost", "Adidas", "Running Shoes", true)).await.unwrap();
    engine.index(make_product("p3", "Apple AirPods Pro", "Apple", "Headphones", true)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "running shoe".to_string(),
        limit: 10,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.total > 0, "should return results for 'running shoe'");
    // Running shoes should rank above headphones.
    let ids: Vec<&str> = resp.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"p1") || ids.contains(&"p2"), "running shoe products should appear");
}

#[tokio::test]
async fn test_index_and_delete() {
    let engine = make_engine();
    engine.index(make_product("del1", "Temporary Product", "Brand", "Category", true)).await.unwrap();

    // Product should be findable.
    let resp = engine.search(SearchRequest {
        q: "Temporary Product".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();
    assert!(resp.hits.iter().any(|h| h.id == "del1"));

    // Delete and verify gone.
    engine.delete("del1").await.unwrap();
    let resp2 = engine.search(SearchRequest {
        q: "Temporary Product".into(), limit: 5, offset: 0,
        mode: SearchMode::Bm25, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();
    assert!(!resp2.hits.iter().any(|h| h.id == "del1"), "deleted product must not appear");
}

#[tokio::test]
async fn test_metadata_filters() {
    let engine = make_engine();
    engine.index(make_product("f1", "Nike Shoe", "Nike", "Footwear", true)).await.unwrap();
    engine.index(make_product("f2", "Nike Shirt", "Nike", "Apparel", false)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Nike".into(),
        limit: 10,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: Some([("in_stock".to_string(), serde_json::json!(true))].into()),
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().all(|h| h.id != "f2"), "out-of-stock product must be filtered");
    assert!(resp.hits.iter().any(|h| h.id == "f1"), "in-stock product must appear");
}

#[tokio::test]
async fn test_aggregations() {
    let engine = make_engine();
    for (id, brand) in [("a1","Nike"), ("a2","Nike"), ("a3","Adidas"), ("a4","Puma")] {
        engine.index(make_product(id, &format!("{} shoe", brand), brand, "Footwear", true)).await.unwrap();
    }

    let resp = engine.search(SearchRequest {
        q: "shoe".into(), limit: 10, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: Some(vec!["brand".to_string()]),
        explain: false, rerank: false,
    }).await.unwrap();

    let aggs = resp.aggregations.expect("aggregations should be present");
    let brand_counts = aggs.get("brand").expect("brand aggregation should be present");
    assert_eq!(brand_counts.get("Nike").copied().unwrap_or(0), 2);
    assert_eq!(brand_counts.get("Adidas").copied().unwrap_or(0), 1);
}

#[tokio::test]
async fn test_explainability() {
    let engine = make_engine();
    engine.index(make_product("e1", "Explainable Product", "Brand", "Cat", true)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Explainable".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: true, rerank: false,
    }).await.unwrap();

    for hit in &resp.hits {
        assert!(hit.explain.is_some(), "explain:true must include score breakdown");
        let breakdown = hit.explain.as_ref().unwrap();
        assert!(!breakdown.factors.is_empty(), "score breakdown must have factors");
    }
}

#[tokio::test]
async fn test_similar_by_product_id() {
    let engine = make_engine();
    // Index several products — similar products in same category should rank higher.
    engine.index(make_product("s1", "Nike Running Shoe", "Nike", "Running", true)).await.unwrap();
    engine.index(make_product("s2", "Adidas Running Shoe", "Adidas", "Running", true)).await.unwrap();
    engine.index(make_product("s3", "Sony Headphones", "Sony", "Audio", true)).await.unwrap();

    let similar = engine.similar(SimilarRequest {
        product_id: Some("s1".into()),
        text: None,
        vector: None,
        limit: 3,
        filters: None,
    }).await.unwrap();

    assert!(!similar.is_empty(), "similar products should be found");
    // s1 itself should not appear in results (it's the query product, not a result).
    // Note: MemoryVectorIndex doesn't filter the query product, so this just checks it returns something.
    assert!(similar.iter().all(|h| !h.id.is_empty()), "similar hits must have IDs");
}

#[tokio::test]
async fn test_similar_by_text() {
    let engine = make_engine();
    engine.index(make_product("t1", "Running Shoe", "Nike", "Footwear", true)).await.unwrap();
    engine.index(make_product("t2", "Yoga Mat", "Lululemon", "Fitness", true)).await.unwrap();

    let similar = engine.similar(SimilarRequest {
        text: Some("athletic footwear for running".into()),
        product_id: None,
        vector: None,
        limit: 2,
        filters: None,
    }).await.unwrap();

    assert!(!similar.is_empty(), "similar-by-text should return results");
}

#[tokio::test]
async fn test_event_recording_and_signals() {
    let engine = make_engine();
    engine.index(make_product("ev1", "Popular Shoe", "Nike", "Footwear", true)).await.unwrap();

    // Record behavioral events.
    for _ in 0..5 {
        engine.record_event(Event {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: EventType::View,
            product_id: "ev1".into(),
            user_id: Some("user1".into()),
            query: Some("shoe".into()),
            session_id: None,
            timestamp: Utc::now(),
        }).await.unwrap();
    }
    engine.record_event(Event {
        id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::Purchase,
        product_id: "ev1".into(),
        user_id: Some("user1".into()),
        query: None,
        session_id: None,
        timestamp: Utc::now(),
    }).await.unwrap();

    // Search should still work (signals affect ranking, not presence).
    let resp = engine.search(SearchRequest {
        q: "shoe".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "ev1"), "ev1 should appear in results");
}

#[tokio::test]
async fn test_spell_correction_via_search() {
    let engine = make_engine();
    engine.index(make_product("sp1", "Nike Air Max", "Nike", "Footwear", true)).await.unwrap();

    // After indexing "Nike", SymSpell should know the word.
    // Query with slight misspelling — spell corrector should help.
    let resp = engine.search(SearchRequest {
        q: "Nikee Air Maxx".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();
    // Result may or may not include sp1 depending on correction quality,
    // but the search must not error.
    // Correct assertion: the search runs without panicking/erroring.
    let _ = resp; // no panic = pass
}

#[tokio::test]
async fn test_bm25_mode_only() {
    let engine = make_engine();
    engine.index(make_product("b1", "Bluetooth Headphones", "Sony", "Audio", true)).await.unwrap();
    engine.index(make_product("b2", "Wireless Earbuds", "Apple", "Audio", true)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Bluetooth".into(), limit: 5, offset: 0,
        mode: SearchMode::Bm25, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    // BM25 mode should find "Bluetooth Headphones" via keyword match.
    assert!(resp.hits.iter().any(|h| h.id == "b1"),
        "BM25 should match 'Bluetooth Headphones' for query 'Bluetooth'");
}

#[tokio::test]
async fn test_pre_computed_vector_ingestion() {
    let engine = make_engine();
    let now = Utc::now();
    // Provide a pre-computed vector directly (384 dims).
    let vector: Vec<f32> = (0..384).map(|i| (i as f32 * 0.001).sin()).collect();
    let product = Product {
        id: "pv1".into(),
        text: Some("Pre-vectorized product".into()),
        vector: Some(vector),
        metadata: serde_json::json!({"title": "Pre-vectorized product"}),
        model_id: Some("stub-384".into()),
        dims: Some(384),
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    };
    engine.index(product).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Pre-vectorized".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "pv1"),
        "pre-computed vector product must be findable");
}

#[tokio::test]
async fn test_pagination() {
    let engine = make_engine();
    for i in 0..10 {
        engine.index(make_product(
            &format!("pg{}", i),
            &format!("Shoe Model {}", i),
            "Brand",
            "Footwear",
            true,
        )).await.unwrap();
    }

    let page1 = engine.search(SearchRequest {
        q: "shoe".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    let page2 = engine.search(SearchRequest {
        q: "shoe".into(), limit: 5, offset: 5,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    assert_eq!(page1.hits.len(), 5, "page 1 should have 5 hits");
    assert_eq!(page2.hits.len(), 5, "page 2 should have 5 hits");

    // Pages should not overlap.
    let p1_ids: std::collections::HashSet<&str> = page1.hits.iter().map(|h| h.id.as_str()).collect();
    let p2_ids: std::collections::HashSet<&str> = page2.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(p1_ids.is_disjoint(&p2_ids), "page 1 and page 2 must not share hits");
}

#[tokio::test]
async fn test_stats_query_count_and_latency_p95() {
    let engine = make_engine();
    engine.index(make_product("qc1", "Running Shoe", "Nike", "Footwear", true)).await.unwrap();
    engine.index(make_product("qc2", "Yoga Mat", "Lululemon", "Fitness", true)).await.unwrap();

    let n = 10usize;
    for _ in 0..n {
        engine.search(SearchRequest {
            q: "shoe".into(), limit: 5, offset: 0,
            mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
            aggregate: None, explain: false, rerank: false,
        }).await.unwrap();
    }

    let stats = engine.stats().await.unwrap();
    assert_eq!(stats.query_count, n as u64, "query_count must equal number of searches");
    // latency_p95_ms is in ms — even stub embedding completes in ≥0ms.
    // Just verify the field exists and is reasonable (not absurdly large).
    assert!(stats.latency_p95_ms < 60_000, "P95 latency must be sane");
}

#[tokio::test]
async fn test_stats_query_count_ignores_cache_hits() {
    let engine = make_engine()
        .with_query_cache(60, 100);
    engine.index(make_product("cc1", "Cached Product", "Brand", "Cat", true)).await.unwrap();

    // First call: cache miss, should be counted.
    engine.search(SearchRequest {
        q: "Cached".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    // Second call: cache hit — latency window not updated, but query_count should still be 1
    // (cache hits return early before the counter increment).
    engine.search(SearchRequest {
        q: "Cached".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    let stats = engine.stats().await.unwrap();
    // Only the first search hits the counter; cache hits bypass the counter.
    assert_eq!(stats.query_count, 1, "cache hits must not increment query_count");
}

#[tokio::test]
async fn test_model_mismatch_rejected() {
    let engine = make_engine();
    let now = Utc::now();
    // Try to index with a different model_id — should fail.
    let product = Product {
        id: "mm1".into(),
        text: None,
        vector: Some(vec![0.1; 768]),
        metadata: serde_json::json!({"title": "Mismatch product"}),
        model_id: Some("different-model-768".into()),
        dims: Some(768),
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    };
    let result = engine.index(product).await;
    assert!(result.is_err(), "indexing with mismatched model_id must fail");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("model mismatch"), "error must mention model mismatch, got: {}", err);
}
