use super::{VectorIndex, VectorIndexStats};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{Dtype, EdgestoreConfig, Engine, Metric, VectorEngine, VectorRecord};
use std::path::Path;
use std::sync::{Arc, Mutex};

const NS_VECTORS: &[u8] = b"vectors";

pub struct EdgeStoreVectorIndex {
    engine: Arc<Mutex<Engine>>,
    model_id: Option<String>,
    dims: Option<usize>,
}

impl EdgeStoreVectorIndex {
    /// Create from a pre-opened shared engine.
    ///
    /// Preferred over `open` when storage and vector index share one engine.
    pub fn from_engine(
        engine: Arc<Mutex<Engine>>,
        model_id: Option<String>,
        dims: Option<usize>,
    ) -> Result<Self> {
        // Warm HNSW index into RAM so first search after startup isn't cold.
        // Ignore error — a missing or empty index is fine (first run, no vectors yet).
        let _ = engine.lock().unwrap().preload_vector_index(NS_VECTORS);
        Ok(Self { engine, model_id, dims })
    }

    /// Convenience: open a new engine at `path` and wrap it.
    ///
    /// Prefer `from_engine` when a storage backend shares the same engine.
    pub fn open(path: impl AsRef<Path>, model_id: Option<String>, dims: Option<usize>) -> Result<Self> {
        let config = EdgestoreConfig::new(path.as_ref());
        let mut engine = Engine::open(config).context("failed to open EdgeStore vector index")?;
        let _ = engine.preload_vector_index(NS_VECTORS);
        Self::from_engine(Arc::new(Mutex::new(engine)), model_id, dims)
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
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || -> Result<VectorIndexStats> {
            let eng = engine.lock().unwrap();
            // vector_count returns Some(n) if HNSW index is loaded in memory
            // (preload_vector_index ran at open time). None means no vectors yet.
            let vector_count = eng.vector_count(NS_VECTORS).unwrap_or(0);
            Ok(VectorIndexStats {
                vector_count,
                index_bytes: crate::dir_bytes(eng.db_path()),
            })
        })
        .await?
    }
}
