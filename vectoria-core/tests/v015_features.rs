//! v0.1.15 regression tests:
//! - BM25 single-result bug: BM25 mode must return multiple hits for a query that matches many products.
//! TenantStore TOCTOU tests are in vectoria-server/tests/tenant_store_test.rs (TenantStore lives in vectoria-server).

#[allow(dead_code)]
mod common;

use vectoria_core::model::{SearchMode, SearchRequest};

fn bm25_req(q: &str) -> SearchRequest {
    SearchRequest {
        q: q.to_string(),
        limit: 20,
        mode: SearchMode::Bm25,
        ..Default::default()
    }
}

async fn make_engine() -> vectoria_core::search::SearchEngine {
    let (engine, _) = common::make_engine(4).await;
    engine
}

// ── BM25 multi-result regression ─────────────────────────────────────────────

#[tokio::test]
async fn test_bm25_returns_multiple_results_for_broad_query() {
    let engine = make_engine().await;

    // Index 10 products that all contain "shoe"
    for i in 1..=10 {
        engine
            .index(common::make_product(
                &format!("p{i}"),
                &format!("product {i} shoe running athletic footwear"),
            ))
            .await
            .unwrap();
    }

    let resp = engine.search(bm25_req("shoe")).await.unwrap();
    assert!(
        resp.hits.len() >= 5,
        "BM25 must return multiple hits for 'shoe'; got {} (single-result regression)",
        resp.hits.len()
    );
}

#[tokio::test]
async fn test_bm25_expansion_fires_on_sparse_results() {
    let engine = make_engine().await;

    // Index products where the query matches only one directly but expansion should find more.
    engine.index(common::make_product("p1", "shoe sneaker athletic footwear running")).await.unwrap();
    engine.index(common::make_product("p2", "sneaker casual footwear walking")).await.unwrap();
    engine.index(common::make_product("p3", "athletic shoe sport performance")).await.unwrap();
    engine.index(common::make_product("p4", "running footwear trail marathon")).await.unwrap();
    engine.index(common::make_product("p5", "boot leather formal")).await.unwrap();

    // "shoes" should find shoe-related products via BM25 + expansion, not just 1.
    let resp = engine.search(bm25_req("shoes")).await.unwrap();
    assert!(
        resp.hits.len() >= 2,
        "BM25 with sparse initial results must expand and return more; got {} hit(s)",
        resp.hits.len()
    );
    // The leather boot should not appear — it shares no terms with "shoes"
    let has_boot = resp.hits.iter().any(|h| h.id == "p5");
    assert!(!has_boot, "irrelevant product (leather boot) should not appear in shoe query results");
}

