use crate::storage::StorageEngine;
use std::sync::Arc;
use std::time::Duration;

/// Background task that periodically aggregates behavioral events into
/// pre-computed ProductSignals. This amortizes per-search event scanning.
///
/// Runs every `interval_secs` seconds. Iterates all products in batches,
/// computes signals from raw events, and writes cached results via
/// `put_product_signals`. Search code then reads from the cache instead
/// of scanning all events per result.
pub async fn run_aggregation_loop(storage: Arc<dyn StorageEngine>, interval_secs: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        if let Err(e) = aggregate_once(Arc::clone(&storage)).await {
            tracing::error!(error = %e, "aggregation cycle failed");
        }
    }
}

async fn aggregate_once(storage: Arc<dyn StorageEngine>) -> anyhow::Result<()> {
    let mut offset = 0usize;
    const BATCH: usize = 500;
    let mut total = 0usize;

    loop {
        let products = storage.list_products(offset, BATCH).await?;
        if products.is_empty() {
            break;
        }
        let count = products.len();
        for product in products {
            // get_product_signals computes fresh from events (bypasses cache when
            // cache is empty, which it is before the first aggregation cycle).
            // We then persist the result so subsequent search reads are O(1).
            let signals = storage.get_product_signals(&product.id).await?;
            storage.put_product_signals(&product.id, &signals).await?;
        }
        total += count;
        offset += count;
        if count < BATCH {
            break;
        }
    }

    if total > 0 {
        tracing::debug!(products_aggregated = total, "aggregation cycle complete");
    }
    Ok(())
}
