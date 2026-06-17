#[allow(dead_code)]
mod common;

use common::{make_engine, make_engine_with_cache, make_product};
use vectoria_core::model::{Hit, RankingWeights, SearchMode, SearchRequest};

#[test]
fn verify_nan_scores_do_not_panic_sort() {
    let mut hits = vec![
        Hit { id: "a".into(), score: 0.8, metadata: serde_json::json!({}), explain: None },
        Hit { id: "b".into(), score: f32::NAN, metadata: serde_json::json!({}), explain: None },
        Hit { id: "c".into(), score: 0.5, metadata: serde_json::json!({}), explain: None },
    ];
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let non_nan: Vec<_> = hits.iter().filter(|h| !h.score.is_nan()).collect();
    assert!(non_nan[0].score >= non_nan[1].score, "non-NaN scores must be descending");
}

// ── Claim 2: limit > MAX_LIMIT capped to 1_000 ───────────────────────────

#[tokio::test]
async fn verify_limit_capped_at_max() {
    let (engine, _) = make_engine(32).await;
    engine.index(make_product("lim1", "Shoe")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "shoe".into(), limit: 9_999, offset: 0,
        mode: SearchMode::Hybrid,
        filters: None, ranking_weights: None, aggregate: None,
        explain: false, rerank: false,
    }).await.unwrap();

    assert_eq!(resp.limit, 1_000, "limit must be capped at 1_000, got {}", resp.limit);
}

#[tokio::test]
async fn verify_offset_capped_at_max() {
    let (engine, _) = make_engine(32).await;
    engine.index(make_product("off1", "Boot")).await.unwrap();

    let resp = engine.search(SearchRequest {
        q: "boot".into(), limit: 10, offset: 999_999,
        mode: SearchMode::Hybrid,
        filters: None, ranking_weights: None, aggregate: None,
        explain: false, rerank: false,
    }).await.unwrap();

    assert_eq!(resp.offset, 10_000, "offset must be capped at 10_000, got {}", resp.offset);
}

#[tokio::test]
async fn verify_custom_weights_bypass_cache() {
    let (engine, embed) = make_engine_with_cache(32).await;
    engine.index(make_product("wt1", "Tent")).await.unwrap();
    let after_index = embed.call_count();

    let req_with_weights = |semantic: f32| SearchRequest {
        q: "tent".into(), limit: 5, offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: Some(RankingWeights { semantic, ..RankingWeights::default() }),
        aggregate: None, explain: false, rerank: false,
    };

    engine.search(req_with_weights(0.9)).await.unwrap();
    let after_first = embed.call_count();
    assert_eq!(after_first, after_index + 1, "first custom-weight search must embed");

    engine.search(req_with_weights(0.1)).await.unwrap();
    assert_eq!(embed.call_count(), after_first + 1, "custom-weight request must bypass cache");
}
