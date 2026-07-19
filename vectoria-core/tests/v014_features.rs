//! Phase 2 feature tests: pins, sponsored slots, suppressions, export/import.
//! Uses the in-memory backend (MemoryStorage) — no disk I/O needed.

#[allow(dead_code)]
mod common;

use vectoria_core::model::{SearchMode, SearchRequest};

fn req(q: &str) -> SearchRequest {
    SearchRequest {
        q: q.to_string(),
        limit: 10,
        offset: 0,
        mode: SearchMode::Hybrid,
        filters: None,
        ranking_weights: None,
        aggregate: None,
        explain: false,
        rerank: false,
        cluster: false,
    }
}

async fn make_engine() -> vectoria_core::search::SearchEngine {
    let (engine, _) = common::make_engine(384).await;
    engine
}

async fn seed(engine: &vectoria_core::search::SearchEngine) {
    engine.index(common::make_product("p1", "Nike Air Max running shoe")).await.unwrap();
    engine.index(common::make_product("p2", "Adidas Ultraboost running shoe")).await.unwrap();
    engine.index(common::make_product("p3", "Puma Velocity running shoe")).await.unwrap();
    engine.index(common::make_product("p4", "New Balance running shoe")).await.unwrap();
}

// ── Pins ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_pin_forces_product_to_position_1() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Find organic last result
    let before = engine.search(req("running shoe")).await.unwrap();
    assert!(before.total >= 2);
    let last_id = before.hits.last().unwrap().id.clone();

    // Pin it to position 1
    engine.create_pin("running shoe".into(), last_id.clone(), 1).await.unwrap();

    let after = engine.search(req("running shoe")).await.unwrap();
    assert_eq!(after.hits[0].id, last_id, "pinned product should be first");
}

#[tokio::test]
async fn test_pin_to_middle_position() {
    let engine = make_engine().await;
    seed(&engine).await;

    engine.create_pin("running shoe".into(), "p4".into(), 2).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    let pos = resp.hits.iter().position(|h| h.id == "p4").unwrap();
    assert_eq!(pos, 1, "p4 should be at index 1 (position 2)");
}

#[tokio::test]
async fn test_pin_list_and_delete() {
    let engine = make_engine().await;
    seed(&engine).await;

    let pin = engine.create_pin("running shoe".into(), "p1".into(), 1).await.unwrap();
    let pins = engine.list_pins().await.unwrap();
    assert_eq!(pins.len(), 1);

    engine.delete_pin(&pin.id).await.unwrap();
    assert!(engine.list_pins().await.unwrap().is_empty());

    // After delete, p1 is no longer pinned — it should not necessarily be first
    let resp = engine.search(req("running shoe")).await.unwrap();
    assert!(resp.total > 0); // results still come back
}

#[tokio::test]
async fn test_pin_upserts_by_query_and_product_id() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Pin p1 to position 1, then re-pin p1 to position 3 for the same query.
    // There must never be two active pins for the same (query, product_id).
    engine.create_pin("running shoe".into(), "p1".into(), 1).await.unwrap();
    engine.create_pin("running shoe".into(), "p1".into(), 3).await.unwrap();

    let pins = engine.list_pins().await.unwrap();
    assert_eq!(pins.len(), 1, "duplicate (query, product_id) pin must be replaced, not added");
    assert_eq!(pins[0].position, 3, "second create_pin must update the position");

    let resp = engine.search(req("running shoe")).await.unwrap();
    let pos = resp.hits.iter().position(|h| h.id == "p1").unwrap();
    assert_eq!(pos, 2, "p1 should be at index 2 (position 3)");
}

#[tokio::test]
async fn test_pin_only_applies_to_matching_query() {
    let engine = make_engine().await;
    engine.index(common::make_product("x1", "laptop computer")).await.unwrap();
    engine.index(common::make_product("x2", "running shoe")).await.unwrap();

    engine.create_pin("running shoe".into(), "x2".into(), 1).await.unwrap();

    // Pin should NOT affect a different query
    let resp = engine.search(req("laptop")).await.unwrap();
    if !resp.hits.is_empty() {
        assert_ne!(resp.hits[0].id, "x2", "pin should not affect unrelated query");
    }
}

#[tokio::test]
async fn test_multiple_pins_same_query_correct_order() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Pin two products simultaneously; both must land at their declared positions.
    engine.create_pin("running shoe".into(), "p4".into(), 1).await.unwrap();
    engine.create_pin("running shoe".into(), "p3".into(), 2).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    assert_eq!(resp.hits[0].id, "p4", "p4 should be at position 1");
    assert_eq!(resp.hits[1].id, "p3", "p3 should be at position 2");
}

#[tokio::test]
async fn test_pin_position_beyond_list_clamps_to_end() {
    let engine = make_engine().await;
    seed(&engine).await; // 4 products

    // Request position 100 — should clamp to last position.
    engine.create_pin("running shoe".into(), "p2".into(), 100).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    assert_eq!(resp.hits.last().unwrap().id, "p2", "out-of-range pin should land at last position");
    assert_eq!(resp.hits.iter().filter(|h| h.id == "p2").count(), 1, "p2 must appear exactly once");
}

// ── Suppressions ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_suppression_removes_product_from_results() {
    let engine = make_engine().await;
    seed(&engine).await;

    let before = engine.search(req("running shoe")).await.unwrap();
    assert!(before.hits.iter().any(|h| h.id == "p2"), "p2 should appear before suppression");

    engine.create_suppression("running shoe".into(), "p2".into()).await.unwrap();

    let after = engine.search(req("running shoe")).await.unwrap();
    assert!(!after.hits.iter().any(|h| h.id == "p2"), "p2 should be suppressed");
    assert!(after.total < before.total, "total should decrease after suppression");
}

#[tokio::test]
async fn test_suppression_only_applies_to_matching_query() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Suppress p1 only for "running shoe"
    engine.create_suppression("running shoe".into(), "p1".into()).await.unwrap();

    // A different query must still return p1
    let resp = engine.search(req("Nike Air Max")).await.unwrap();
    assert!(resp.hits.iter().any(|h| h.id == "p1"), "suppression for 'running shoe' must not affect 'Nike Air Max' results");
}

#[tokio::test]
async fn test_suppression_list_and_delete() {
    let engine = make_engine().await;
    seed(&engine).await;

    let sup = engine.create_suppression("running shoe".into(), "p3".into()).await.unwrap();
    assert_eq!(engine.list_suppressions().await.unwrap().len(), 1);

    engine.delete_suppression(&sup.id).await.unwrap();
    assert!(engine.list_suppressions().await.unwrap().is_empty());

    // p3 visible again after deletion
    let resp = engine.search(req("running shoe")).await.unwrap();
    assert!(resp.hits.iter().any(|h| h.id == "p3"), "p3 should reappear after suppression removed");
}

// ── Sponsored ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_sponsored_injects_product_with_flag() {
    let engine = make_engine().await;
    seed(&engine).await;

    engine.create_sponsored(
        "running".into(), "p1".into(), 1, "Ad".into(), None, None,
    ).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    let first = &resp.hits[0];
    assert_eq!(first.id, "p1");
    assert_eq!(first.metadata.get("sponsored").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(first.metadata.get("sponsored_label").and_then(|v| v.as_str()), Some("Ad"));
}

#[tokio::test]
async fn test_sponsored_prefix_matches_longer_query() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Pattern "running" should match query "running shoe"
    engine.create_sponsored(
        "running".into(), "p4".into(), 1, "Sponsored".into(), None, None,
    ).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    assert_eq!(resp.hits[0].id, "p4");
}

#[tokio::test]
async fn test_sponsored_date_range_expired_not_injected() {
    use chrono::{Duration, Utc};
    let engine = make_engine().await;
    seed(&engine).await;

    // end_at in the past → slot should be inactive
    let past = Utc::now() - Duration::hours(1);
    engine.create_sponsored(
        "running".into(), "p1".into(), 1, "Ad".into(),
        None, Some(past),
    ).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    // p1 may still appear organically, but NOT at position 1 with sponsored flag
    if !resp.hits.is_empty() && resp.hits[0].id == "p1" {
        assert_ne!(
            resp.hits[0].metadata.get("sponsored").and_then(|v| v.as_bool()),
            Some(true),
            "expired sponsored slot should not inject with sponsored flag"
        );
    }
}

#[tokio::test]
async fn test_sponsored_list_and_delete() {
    let engine = make_engine().await;
    let slot = engine.create_sponsored(
        "shoes".into(), "p2".into(), 1, "Ad".into(), None, None,
    ).await.unwrap();

    assert_eq!(engine.list_sponsored().await.unwrap().len(), 1);
    engine.delete_sponsored(&slot.id).await.unwrap();
    assert!(engine.list_sponsored().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_sponsored_product_appears_exactly_once() {
    let engine = make_engine().await;
    seed(&engine).await;

    // p1 is in organic results AND a sponsored slot — must appear exactly once.
    engine.create_sponsored("running".into(), "p1".into(), 1, "Ad".into(), None, None).await.unwrap();

    let resp = engine.search(req("running shoe")).await.unwrap();
    let count = resp.hits.iter().filter(|h| h.id == "p1").count();
    assert_eq!(count, 1, "sponsored product already in organic results must appear exactly once");
    assert_eq!(resp.hits[0].id, "p1", "sponsored product should be at declared position");
}

// ── Export / Import ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_export_import_round_trip() {
    let engine = make_engine().await;
    seed(&engine).await;

    engine.create_pin("running shoe".into(), "p1".into(), 1).await.unwrap();
    engine.create_suppression("running shoe".into(), "p2".into()).await.unwrap();
    engine.create_sponsored("running".into(), "p3".into(), 2, "Ad".into(), None, None).await.unwrap();

    let export = engine.export_overrides().await.unwrap();
    assert_eq!(export.pins.len(), 1);
    assert_eq!(export.suppressions.len(), 1);
    assert_eq!(export.sponsored.len(), 1);

    // Import into a fresh engine
    let engine2 = make_engine().await;
    seed(&engine2).await;
    let report = engine2.import_overrides(export).await.unwrap();
    assert_eq!(report.imported, 3);

    // Verify overrides are active in the new engine
    let resp = engine2.search(req("running shoe")).await.unwrap();
    assert_eq!(resp.hits[0].id, "p1", "pin should be active after import");
    assert!(!resp.hits.iter().any(|h| h.id == "p2"), "suppression should be active after import");
}

// ── Interaction: pin + suppression ────────────────────────────────────────────

#[tokio::test]
async fn test_suppressed_product_not_shown_even_if_organically_top() {
    let engine = make_engine().await;
    seed(&engine).await;

    // Suppress the product that would otherwise appear first
    let before = engine.search(req("running shoe")).await.unwrap();
    let top_id = before.hits[0].id.clone();
    engine.create_suppression("running shoe".into(), top_id.clone()).await.unwrap();

    let after = engine.search(req("running shoe")).await.unwrap();
    assert!(!after.hits.iter().any(|h| h.id == top_id), "suppressed top product should not appear");
    assert!(after.total < before.total);
}
