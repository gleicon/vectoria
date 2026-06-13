use super::{VectorIndex, VectorIndexStats};
use anyhow::{Context, Result};
use async_trait::async_trait;
use edgestore::{Dtype, EdgestoreConfig, Engine, Metric, VectorEngine, VectorRecord};
use std::path::Path;
use std::sync::{Arc, Mutex};

const NS_VECTORS: &[u8] = b"vectors";

/// EdgeStore-backed VectorIndex.
/// Uses flat SIMD search by default (always up-to-date after inserts).
/// Build HNSW via POST /admin/reindex for faster ANN at large scale.
pub struct EdgeStoreVectorIndex {
    engine: Arc<Mutex<Engine>>,
    model_id: Option<String>,
    dims: Option<usize>,
}

impl EdgeStoreVectorIndex {
    pub fn open(path: impl AsRef<Path>, model_id: Option<String>, dims: Option<usize>) -> Result<Self> {
        let config = EdgestoreConfig::new(path.as_ref());
        let engine = Engine::open(config).context("failed to open EdgeStore vector index")?;
        Ok(Self {
            engine: Arc::new(Mutex::new(engine)),
            model_id,
            dims,
        })
    }

    /// Build HNSW index for faster ANN search. Triggered by POST /admin/reindex.
    pub fn build_hnsw(&self) -> Result<()> {
        self.engine
            .lock()
            .unwrap()
            .build_vector_index(NS_VECTORS)
            .context("HNSW build failed")
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
                // Cosine metric: distance 0 = identical (score 1.0).
                let score = 1.0 - r.distance.clamp(0.0, 2.0) / 2.0;
                (id, score)
            })
            .collect())
    }

    async fn flush(&self) -> Result<()> {
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || engine.lock().unwrap().flush())
            .await?
            .context("flush failed")
    }

    fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    fn dims(&self) -> Option<usize> {
        self.dims
    }

    async fn stats(&self) -> Result<VectorIndexStats> {
        Ok(VectorIndexStats { vector_count: 0, index_bytes: 0 })
    }
}
