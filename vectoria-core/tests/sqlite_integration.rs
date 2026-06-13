/// Integration tests for SqliteStorage — in-memory and file-backed paths.
use vectoria_core::{
    model::{Event, EventType, Product, ProductStatus},
    storage::{ProductSignals, StorageEngine, sqlite::SqliteStorage},
};
use chrono::Utc;
use tempfile::TempDir;

fn make_product(id: &str, title: &str) -> Product {
    let now = Utc::now();
    Product {
        id: id.to_string(),
        text: Some(title.to_string()),
        vector: None,
        metadata: serde_json::json!({ "title": title, "in_stock": true }),
        model_id: None,
        dims: None,
        status: ProductStatus::PendingVector,
        created_at: now,
        updated_at: now,
    }
}

fn make_event(id: &str, product_id: &str, event_type: EventType) -> Event {
    Event {
        id: id.to_string(),
        event_type,
        product_id: product_id.to_string(),
        user_id: None,
        query: None,
        session_id: None,
        timestamp: Utc::now(),
    }
}

#[tokio::test]
async fn test_put_and_get_product() {
    let db = SqliteStorage::open_in_memory().unwrap();
    let p = make_product("p1", "Nike Running Shoe");
    db.put_product(&p).await.unwrap();

    let fetched = db.get_product("p1").await.unwrap().expect("product must exist");
    assert_eq!(fetched.id, "p1");
    assert_eq!(fetched.metadata["title"], "Nike Running Shoe");
}

#[tokio::test]
async fn test_get_product_missing_returns_none() {
    let db = SqliteStorage::open_in_memory().unwrap();
    let result = db.get_product("nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_put_product_upsert() {
    let db = SqliteStorage::open_in_memory().unwrap();
    let p = make_product("up1", "Original Title");
    db.put_product(&p).await.unwrap();

    let updated = Product {
        metadata: serde_json::json!({ "title": "Updated Title" }),
        updated_at: Utc::now(),
        ..p
    };
    db.put_product(&updated).await.unwrap();

    let fetched = db.get_product("up1").await.unwrap().unwrap();
    assert_eq!(fetched.metadata["title"], "Updated Title");
}

#[tokio::test]
async fn test_delete_product() {
    let db = SqliteStorage::open_in_memory().unwrap();
    db.put_product(&make_product("del1", "Temporary")).await.unwrap();
    db.delete_product("del1").await.unwrap();

    let result = db.get_product("del1").await.unwrap();
    assert!(result.is_none(), "deleted product must not exist");
}

#[tokio::test]
async fn test_list_products_pagination() {
    let db = SqliteStorage::open_in_memory().unwrap();
    for i in 0..10u32 {
        db.put_product(&make_product(&format!("lp{}", i), &format!("Shoe {}", i))).await.unwrap();
    }

    let page1 = db.list_products(0, 5).await.unwrap();
    let page2 = db.list_products(5, 5).await.unwrap();

    assert_eq!(page1.len(), 5);
    assert_eq!(page2.len(), 5);

    let p1_ids: std::collections::HashSet<String> = page1.iter().map(|p| p.id.clone()).collect();
    let p2_ids: std::collections::HashSet<String> = page2.iter().map(|p| p.id.clone()).collect();
    assert!(p1_ids.is_disjoint(&p2_ids), "pages must not overlap");
}

#[tokio::test]
async fn test_put_event_and_compute_signals_from_events() {
    let db = SqliteStorage::open_in_memory().unwrap();
    db.put_product(&make_product("sig1", "Popular Item")).await.unwrap();

    for i in 0..5u32 {
        db.put_event(&make_event(&format!("v{}", i), "sig1", EventType::View)).await.unwrap();
    }
    db.put_event(&make_event("c1", "sig1", EventType::Click)).await.unwrap();
    db.put_event(&make_event("pu1", "sig1", EventType::Purchase)).await.unwrap();

    let signals = db.get_product_signals("sig1").await.unwrap();
    assert_eq!(signals.view_count, 5);
    assert_eq!(signals.click_count, 1);
    assert_eq!(signals.purchase_count, 1);
    assert!(signals.popularity > 0.0, "popularity must be > 0");
    assert!(signals.conversion_rate > 0.0, "conversion_rate must be > 0");
}

#[tokio::test]
async fn test_put_product_signals_cached_read() {
    let db = SqliteStorage::open_in_memory().unwrap();
    db.put_product(&make_product("cs1", "Cached Signals Item")).await.unwrap();

    let cached = ProductSignals {
        click_count: 42,
        purchase_count: 7,
        view_count: 100,
        cart_count: 10,
        popularity: 0.42,
        conversion_rate: 0.07,
    };
    db.put_product_signals("cs1", &cached).await.unwrap();

    let fetched = db.get_product_signals("cs1").await.unwrap();
    assert_eq!(fetched.click_count, 42);
    assert_eq!(fetched.purchase_count, 7);
    assert_eq!(fetched.view_count, 100);
}

#[tokio::test]
async fn test_stats() {
    let db = SqliteStorage::open_in_memory().unwrap();
    db.put_product(&make_product("st1", "Shoe A")).await.unwrap();
    db.put_product(&make_product("st2", "Shoe B")).await.unwrap();
    db.put_event(&make_event("ev1", "st1", EventType::Click)).await.unwrap();

    let stats = db.stats().await.unwrap();
    assert_eq!(stats.product_count, 2);
    assert_eq!(stats.event_count, 1);
    assert!(stats.storage_bytes > 0, "storage_bytes must be non-zero");
}

#[tokio::test]
async fn test_duplicate_event_ignored() {
    let db = SqliteStorage::open_in_memory().unwrap();
    db.put_product(&make_product("de1", "Item")).await.unwrap();

    let ev = make_event("same-id", "de1", EventType::View);
    db.put_event(&ev).await.unwrap();
    db.put_event(&ev).await.unwrap();

    let signals = db.get_product_signals("de1").await.unwrap();
    assert_eq!(signals.view_count, 1, "duplicate event must not be counted twice");
}

#[tokio::test]
async fn test_file_backed_persistence() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("store.db");

    {
        let db = SqliteStorage::open(&path).unwrap();
        db.put_product(&make_product("fb1", "Persisted Shoe")).await.unwrap();
    }

    let db2 = SqliteStorage::open(&path).unwrap();
    let fetched = db2.get_product("fb1").await.unwrap().expect("product must persist across opens");
    assert_eq!(fetched.metadata["title"], "Persisted Shoe");
}
