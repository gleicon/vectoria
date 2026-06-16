use crate::storage::StorageEngine;
use std::sync::Arc;
use std::time::Duration;

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
            let signals = storage.recompute_product_signals(&product.id).await?;
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
