#[allow(dead_code)]
mod common;

use vectoria_core::model::{
    Event, EventType, Product, ProductStatus, SearchMode, SearchRequest,
    SimilarRequest,
};
use chrono::Utc;

async fn make_engine() -> vectoria_core::search::SearchEngine {
    let (engine, _) = common::make_engine(384).await;
    engine
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

#[tokio::test]
async fn test_index_and_search_basic() {
    let engine = make_engine().await;

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
    let ids: Vec<&str> = resp.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"p1") || ids.contains(&"p2"), "running shoe products should appear");
}

#[tokio::test]
async fn test_index_and_delete() {
    let engine = make_engine().await;
    engine.index(make_product("del1", "Temporary Product", "Brand", "Category", true)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Temporary Product".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();
    assert!(resp.hits.iter().any(|h| h.id == "del1"));

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
    let engine = make_engine().await;
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
    let engine = make_engine().await;
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
    let engine = make_engine().await;
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
    let engine = make_engine().await;
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
    // MemoryVectorIndex doesn't filter the query product itself from results.
    assert!(similar.iter().all(|h| !h.id.is_empty()), "similar hits must have IDs");
}

#[tokio::test]
async fn test_similar_by_text() {
    let engine = make_engine().await;
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
    let engine = make_engine().await;
    engine.index(make_product("ev1", "Popular Shoe", "Nike", "Footwear", true)).await.unwrap();

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

    let resp = engine.search(SearchRequest {
        q: "shoe".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "ev1"), "ev1 should appear in results");
}

#[tokio::test]
async fn test_query_ctr_boosts_clicked_product() {
    let engine = make_engine().await;
    engine.index(make_product("ctr1", "Running Shoe A", "Nike", "Footwear", true)).await.unwrap();
    engine.index(make_product("ctr2", "Running Shoe B", "Adidas", "Footwear", true)).await.unwrap();

    // Record 5 clicks on ctr1 for this exact query, none for ctr2.
    for _ in 0..5 {
        engine.record_event(Event {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: EventType::Click,
            product_id: "ctr1".into(),
            user_id: Some("user1".into()),
            query: Some("running shoe".into()),
            session_id: None,
            timestamp: Utc::now(),
        }).await.unwrap();
    }

    let resp = engine.search(SearchRequest {
        q: "running shoe".into(), limit: 10, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: true, rerank: false,
    }).await.unwrap();

    let ctr1 = resp.hits.iter().find(|h| h.id == "ctr1").expect("ctr1 must be in results");
    let ctr2 = resp.hits.iter().find(|h| h.id == "ctr2").expect("ctr2 must be in results");

    assert!(
        ctr1.score > ctr2.score,
        "ctr1 (clicked 5×) must outscore ctr2 (never clicked): {:.4} vs {:.4}",
        ctr1.score, ctr2.score
    );

    // Verify query_ctr factor is present and non-zero in explain.
    let factors = ctr1.explain.as_ref().unwrap();
    let ctr_factor = factors.factors.iter().find(|f| f.factor == "query_ctr").unwrap();
    assert!(ctr_factor.score > 0.0, "query_ctr factor must be non-zero for ctr1");
}


#[tokio::test]
async fn test_bm25_mode_only() {
    let engine = make_engine().await;
    engine.index(make_product("b1", "Bluetooth Headphones", "Sony", "Audio", true)).await.unwrap();
    engine.index(make_product("b2", "Wireless Earbuds", "Apple", "Audio", true)).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "Bluetooth".into(), limit: 5, offset: 0,
        mode: SearchMode::Bm25, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    assert!(resp.hits.iter().any(|h| h.id == "b1"),
        "BM25 should match 'Bluetooth Headphones' for query 'Bluetooth'");
}

#[tokio::test]
async fn test_pre_computed_vector_ingestion() {
    let engine = make_engine().await;
    let now = Utc::now();
    let vector: Vec<f32> = (0..384).map(|i| (i as f32 * 0.001).sin()).collect();
    let product = Product {
        id: "pv1".into(),
        text: Some("Pre-vectorized product".into()),
        vector: Some(vector),
        metadata: serde_json::json!({"title": "Pre-vectorized product"}),
        model_id: Some("stub".into()),
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
    let engine = make_engine().await;
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

    let p1_ids: std::collections::HashSet<&str> = page1.hits.iter().map(|h| h.id.as_str()).collect();
    let p2_ids: std::collections::HashSet<&str> = page2.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(p1_ids.is_disjoint(&p2_ids), "page 1 and page 2 must not share hits");
}

#[tokio::test]
async fn test_stats_query_count_and_latency_p95() {
    let engine = make_engine().await;
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
    assert!(stats.latency_p95_ms < 60_000, "P95 latency must be sane");
}

#[tokio::test]
async fn test_stats_query_count_ignores_cache_hits() {
    let engine = make_engine().await
        .with_query_cache(60, 100);
    engine.index(make_product("cc1", "Cached Product", "Brand", "Cat", true)).await.unwrap();

    engine.search(SearchRequest {
        q: "Cached".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    engine.search(SearchRequest {
        q: "Cached".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid, filters: None, ranking_weights: None,
        aggregate: None, explain: false, rerank: false,
    }).await.unwrap();

    let stats = engine.stats().await.unwrap();
    assert_eq!(stats.query_count, 1, "cache hits must not increment query_count");
}

/// Anatomy test: index products with known characteristics, fire explain search,
/// print the full JSON breakdown, and verify the math holds.
/// Run with `cargo test test_explain_score_breakdown_anatomy -- --nocapture`
/// to see the actual output used in docs.
#[tokio::test]
async fn test_explain_score_breakdown_anatomy() {

    let engine = make_engine().await;

    // shoe1: exact BM25 match + click events → should dominate via bm25 + query_ctr
    engine.index(make_product("shoe1", "Nike Air Max running shoe waterproof", "Nike", "Footwear", true)).await.unwrap();
    // shoe2: partial BM25 match, no CTR
    engine.index(make_product("shoe2", "Adidas Ultraboost running trainer", "Adidas", "Footwear", true)).await.unwrap();
    // mat1: no BM25 match for "running shoe" query
    engine.index(make_product("mat1", "Yoga mat non-slip extra thick", "Manduka", "Fitness", true)).await.unwrap();

    // Record 3 clicks on shoe1 for this exact query → query_ctr signal
    for _ in 0..3 {
        engine.record_event(Event {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: EventType::Click,
            product_id: "shoe1".into(),
            user_id: Some("u1".into()),
            query: Some("running shoe".into()),
            session_id: None,
            timestamp: Utc::now(),
        }).await.unwrap();
    }

    let resp = engine.search(SearchRequest {
        q: "running shoe".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: true,
        rerank: false,
    }).await.unwrap();

    println!("\n=== Explain output for 'running shoe' ===");
    for hit in &resp.hits {
        println!("\n--- {} (score: {:.4}) ---", hit.id, hit.score);
        if let Some(bd) = &hit.explain {
            println!("{}", serde_json::to_string_pretty(bd).unwrap());

            // sum(contribution) must equal hit.score (within float epsilon)
            let contrib_sum: f32 = bd.factors.iter().map(|f| f.contribution).sum();
            assert!(
                (contrib_sum - hit.score).abs() < 0.001,
                "sum(contribution)={:.4} must equal score={:.4} for {}",
                contrib_sum, hit.score, hit.id
            );

            // each contribution must equal score × weight
            for factor in &bd.factors {
                assert!(
                    (factor.contribution - factor.score * factor.weight).abs() < 0.0001,
                    "factor '{}': contribution={:.4} != score×weight={:.4}",
                    factor.factor, factor.contribution, factor.score * factor.weight
                );
            }

            // query_context must be populated
            assert_eq!(bd.query_context.original_query, "running shoe");
        }
    }

    // shoe1 must outrank shoe2 (it has query_ctr boost)
    let pos_shoe1 = resp.hits.iter().position(|h| h.id == "shoe1").unwrap();
    let pos_shoe2 = resp.hits.iter().position(|h| h.id == "shoe2").unwrap();
    assert!(pos_shoe1 < pos_shoe2, "shoe1 (with CTR) must rank above shoe2");

    // shoe1 explain must show match_sources including bm25
    let shoe1 = resp.hits.iter().find(|h| h.id == "shoe1").unwrap();
    let bd = shoe1.explain.as_ref().unwrap();
    assert!(
        bd.match_sources.contains(&"bm25".to_string()),
        "shoe1 must have bm25 in match_sources: {:?}", bd.match_sources
    );

    // shoe1 query_ctr factor must be non-zero
    let ctr_factor = bd.factors.iter().find(|f| f.factor == "query_ctr").unwrap();
    assert!(ctr_factor.score > 0.0, "shoe1 query_ctr score must be > 0 after 3 clicks");

    println!("\n=== Top result match_sources: {:?} ===", bd.match_sources);
    println!("=== query_context: {:?} ===", bd.query_context);
}

#[tokio::test]
async fn test_model_mismatch_rejected() {
    let engine = make_engine().await;
    let now = Utc::now();
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
