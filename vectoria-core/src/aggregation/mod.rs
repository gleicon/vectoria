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
    aggregate_product_signals(Arc::clone(&storage)).await?;
    aggregate_user_vectors(Arc::clone(&storage)).await?;
    Ok(())
}

async fn aggregate_product_signals(storage: Arc<dyn StorageEngine>) -> anyhow::Result<()> {
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
        tracing::debug!(products_aggregated = total, "product signal aggregation complete");
    }
    Ok(())
}

/// Refresh per-user interest vectors by averaging the stored vectors of recently
/// interacted products. Only users with at least one click/purchase are updated.
async fn aggregate_user_vectors(storage: Arc<dyn StorageEngine>) -> anyhow::Result<()> {
    let user_ids = storage.list_user_ids().await?;
    let mut updated = 0usize;

    for user_id in &user_ids {
        let product_ids = storage.get_user_recent_products(user_id, 50).await?;
        if product_ids.is_empty() {
            continue;
        }

        let mut sum: Vec<f64> = Vec::new();
        let mut count = 0usize;

        for pid in &product_ids {
            if let Ok(Some(product)) = storage.get_product(pid).await {
                if let Some(vector) = &product.vector {
                    if sum.is_empty() {
                        sum = vec![0.0f64; vector.len()];
                    }
                    if vector.len() == sum.len() {
                        for (s, v) in sum.iter_mut().zip(vector.iter()) {
                            *s += *v as f64;
                        }
                        count += 1;
                    }
                }
            }
        }

        if count == 0 {
            continue;
        }

        let user_vec: Vec<f32> = sum.iter().map(|s| (*s / count as f64) as f32).collect();
        if let Err(e) = storage.put_user_vector(user_id, &user_vec).await {
            tracing::warn!(user_id = %user_id, error = %e, "failed to store user vector");
        } else {
            updated += 1;
        }
    }

    if updated > 0 {
        tracing::debug!(users_updated = updated, "user vector aggregation complete");
    }
    Ok(())
}
