use super::{VectorIndex, VectorIndexStats};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{Dtype, EdgestoreConfig, Engine, Metric, VectorEngine, VectorRecord};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

const NS_VECTORS: &[u8] = b"vectors";

pub struct EdgeStoreVectorIndex {
    engine: Arc<Mutex<Engine>>,
    model_id: Option<String>,
    dims: Option<usize>,
    count: AtomicU64,
}

impl EdgeStoreVectorIndex {
    pub fn open(path: impl AsRef<Path>, model_id: Option<String>, dims: Option<usize>) -> Result<Self> {
        let config = EdgestoreConfig::new(path.as_ref());
        let engine = Engine::open(config).context("failed to open EdgeStore vector index")?;
        // count starts at 0 and is incremented/decremented on upsert/delete.
        // It is NOT seeded from disk, so GET /stats vector_count is 0 until the
        // first write after startup. Search and ranking are unaffected.
        Ok(Self {
            engine: Arc::new(Mutex::new(engine)),
            model_id,
            dims,
            count: AtomicU64::new(0),
        })
    }
}

fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[async_trait]
impl VectorIndex for EdgeStoreVectorIndex {
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let dims = vector.len() as u16;
        let data = f32_to_bytes(vector);
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().vector_put(NS_VECTORS, &key, dims, Dtype::F32, &data)
        })
        .await?
        .context("vector_put failed")?;
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let key = id.as_bytes().to_vec();
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().vector_delete(NS_VECTORS, &key)
        })
        .await?
        .context("vector_delete failed")?;
        self.count.fetch_sub(1, Ordering::Relaxed);
        Ok(())
    }

    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f32)>> {
        let dims = query.len() as u16;
        let data = f32_to_bytes(query);
        let query_record = VectorRecord { dims, dtype: Dtype::F32, data };
        let engine = Arc::clone(&self.engine);
        let results = tokio::task::spawn_blocking(move || {
            engine.lock().unwrap().vector_search(NS_VECTORS, &query_record, top_k, Metric::Cosine)
        })
        .await?
        .context("vector_search failed")?;

        Ok(results
            .into_iter()
            .map(|r| {
                let id = String::from_utf8_lossy(&r.key).to_string();
                let score = 1.0 - r.distance.clamp(0.0, 2.0) / 2.0;
                (id, score)
            })
            .collect())
    }

    async fn flush(&self) -> Result<()> {
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut eng = engine.lock().unwrap();
            eng.flush().context("flush failed")?;
            eng.build_vector_index(NS_VECTORS).context("build_vector_index failed")
        })
        .await??;
        Ok(())
    }

    fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    fn dims(&self) -> Option<usize> {
        self.dims
    }

    async fn stats(&self) -> Result<VectorIndexStats> {
        let path = {
            let eng = self.engine.lock().unwrap();
            eng.db_path().to_path_buf()
        };
        Ok(VectorIndexStats {
            vector_count: self.count.load(Ordering::Relaxed),
            index_bytes: crate::dir_bytes(&path),
        })
    }
}
