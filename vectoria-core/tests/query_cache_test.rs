#[allow(dead_code)]
mod common;

use vectoria_core::model::{SearchMode, SearchRequest};

#[tokio::test]
async fn test_query_cache_second_call_skips_embedding() {
    let (engine, stub) = common::make_engine_with_cache(32).await;

    engine.index(common::make_product("qc1", "Running Shoes")).await.unwrap();
    let calls_after_index = stub.call_count();

    let req = || SearchRequest {
        q: "running".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false, cluster: false,
    };

    engine.search(req()).await.unwrap();
    let calls_after_first = stub.call_count();
    assert_eq!(calls_after_first, calls_after_index + 1, "first search must embed query");

    engine.search(req()).await.unwrap();
    let calls_after_second = stub.call_count();
    assert_eq!(calls_after_second, calls_after_first, "cache hit must not call embed again");
}

#[tokio::test]
async fn test_query_cache_different_queries_not_shared() {
    let (engine, stub) = common::make_engine_with_cache(32).await;
    engine.index(common::make_product("qc2", "Yoga Mat")).await.unwrap();
    let after_index = stub.call_count();

    let search = |q: &str| SearchRequest {
        q: q.to_string(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false, cluster: false,
    };

    engine.search(search("yoga")).await.unwrap();
    engine.search(search("mat")).await.unwrap();
    assert_eq!(stub.call_count(), after_index + 2);
}

#[tokio::test]
async fn test_explain_not_cached() {
    let (engine, stub) = common::make_engine_with_cache(32).await;
    engine.index(common::make_product("qc3", "Coffee Mug")).await.unwrap();
    let after_index = stub.call_count();

    let req = || SearchRequest {
        q: "coffee".into(),
        limit: 5,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: true,
        rerank: false, cluster: false,
    };

    engine.search(req()).await.unwrap();
    engine.search(req()).await.unwrap();
    assert_eq!(stub.call_count(), after_index + 2, "explain must not be cached");
}
