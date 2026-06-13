use super::{ProductSignals, StorageEngine, StorageStats};
use crate::model::{Event, EventType, Product};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// SQLite-backed StorageEngine. Fallback when EdgeStore is unavailable.
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open SQLite at {:?}", path))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS products (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    product_id TEXT NOT NULL,
    data TEXT NOT NULL,
    timestamp TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_product ON events(product_id);
CREATE TABLE IF NOT EXISTS signals (
    product_id TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
";

#[async_trait]
impl StorageEngine for SqliteStorage {
    async fn put_product(&self, product: &Product) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let data = serde_json::to_string(product)?;
        let id = product.id.clone();
        let created = product.created_at.to_rfc3339();
        let updated = product.updated_at.to_rfc3339();
        tokio::task::spawn_blocking(move || {
            conn.lock().unwrap().execute(
                "INSERT INTO products(id, data, created_at, updated_at) VALUES(?1,?2,?3,?4) \
                 ON CONFLICT(id) DO UPDATE SET data=excluded.data, updated_at=excluded.updated_at",
                params![id, data, created, updated],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    async fn get_product(&self, id: &str) -> Result<Option<Product>> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();
        let result = tokio::task::spawn_blocking(move || {
            let db = conn.lock().unwrap();
            let mut stmt = db.prepare_cached("SELECT data FROM products WHERE id=?1")?;
            let mut rows = stmt.query(params![id])?;
            if let Some(row) = rows.next()? {
                let data: String = row.get(0)?;
                Ok::<_, anyhow::Error>(Some(serde_json::from_str(&data)?))
            } else {
                Ok(None)
            }
        }).await??;
        Ok(result)
    }

    async fn delete_product(&self, id: &str) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            conn.lock().unwrap().execute("DELETE FROM products WHERE id=?1", params![id])?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    async fn list_products(&self, offset: usize, limit: usize) -> Result<Vec<Product>> {
        let conn = Arc::clone(&self.conn);
        let result = tokio::task::spawn_blocking(move || {
            let db = conn.lock().unwrap();
            let mut stmt = db.prepare_cached(
                "SELECT data FROM products ORDER BY created_at LIMIT ?1 OFFSET ?2"
            )?;
            let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
                row.get::<_, String>(0)
            })?;
            let mut products = Vec::new();
            for data in rows {
                products.push(serde_json::from_str::<Product>(&data?)?);
            }
            Ok::<_, anyhow::Error>(products)
        }).await??;
        Ok(result)
    }

    async fn put_event(&self, event: &Event) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let data = serde_json::to_string(event)?;
        let id = event.id.clone();
        let product_id = event.product_id.clone();
        let timestamp = event.timestamp.to_rfc3339();
        tokio::task::spawn_blocking(move || {
            conn.lock().unwrap().execute(
                "INSERT OR IGNORE INTO events(id, product_id, data, timestamp) VALUES(?1,?2,?3,?4)",
                params![id, product_id, data, timestamp],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    async fn get_product_signals(&self, product_id: &str) -> Result<ProductSignals> {
        let conn = Arc::clone(&self.conn);
        let pid = product_id.to_string();
        let result = tokio::task::spawn_blocking(move || {
            let db = conn.lock().unwrap();

            // Check cached signals first.
            {
                let mut stmt = db.prepare_cached("SELECT data FROM signals WHERE product_id=?1")?;
                let mut rows = stmt.query(params![pid])?;
                if let Some(row) = rows.next()? {
                    let data: String = row.get(0)?;
                    return Ok::<_, anyhow::Error>(serde_json::from_str(&data)?);
                }
            }

            // Compute from raw events.
            let mut stmt = db.prepare_cached(
                "SELECT data FROM events WHERE product_id=?1"
            )?;
            let rows = stmt.query_map(params![pid], |row| row.get::<_, String>(0))?;

            let mut signals = ProductSignals::default();
            for data in rows {
                let data = data?;
                let event: Event = serde_json::from_str(&data)?;
                match event.event_type {
                    EventType::Click => signals.click_count += 1,
                    EventType::Purchase => signals.purchase_count += 1,
                    EventType::View => signals.view_count += 1,
                    EventType::AddToCart => signals.cart_count += 1,
                    EventType::Wishlist => {}
                }
            }
            let total = signals.view_count.max(1);
            signals.popularity = (signals.click_count as f32 / total as f32).min(1.0);
            signals.conversion_rate = (signals.purchase_count as f32 / total as f32).min(1.0);
            Ok(signals)
        }).await??;
        Ok(result)
    }

    async fn put_product_signals(&self, product_id: &str, signals: &ProductSignals) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let pid = product_id.to_string();
        let data = serde_json::to_string(signals)?;
        tokio::task::spawn_blocking(move || {
            conn.lock().unwrap().execute(
                "INSERT INTO signals(product_id, data) VALUES(?1,?2) \
                 ON CONFLICT(product_id) DO UPDATE SET data=excluded.data",
                params![pid, data],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats> {
        let conn = Arc::clone(&self.conn);
        let result = tokio::task::spawn_blocking(move || {
            let db = conn.lock().unwrap();
            let product_count: u64 = db.query_row(
                "SELECT COUNT(*) FROM products", [], |r| r.get(0)
            )?;
            let event_count: u64 = db.query_row(
                "SELECT COUNT(*) FROM events", [], |r| r.get(0)
            )?;
            // page_count * page_size gives total DB size in bytes.
            let page_count: u64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let page_size: u64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            Ok::<_, anyhow::Error>(StorageStats {
                product_count,
                event_count,
                storage_bytes: page_count * page_size,
            })
        }).await??;
        Ok(result)
    }
}
