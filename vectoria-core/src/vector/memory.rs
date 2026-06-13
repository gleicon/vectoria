use super::{VectorIndex, VectorIndexStats};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

/// Brute-force in-memory vector index for tests and small catalogs.
/// Production path: EdgeStore HNSW or TurboVec.
pub struct MemoryVectorIndex {
    vectors: RwLock<HashMap<String, Vec<f32>>>,
    model_id: Option<String>,
    dims: Option<usize>,
}

impl MemoryVectorIndex {
    pub fn new(model_id: Option<String>, dims: Option<usize>) -> Self {
        Self {
            vectors: RwLock::new(HashMap::new()),
            model_id,
            dims,
        }
    }
}

#[async_trait]
impl VectorIndex for MemoryVectorIndex {
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()> {
        self.vectors
            .write()
            .unwrap()
            .insert(id.to_string(), vector.to_vec());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.vectors.write().unwrap().remove(id);
        Ok(())
    }

    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f32)>> {
        let vectors = self.vectors.read().unwrap();
        let mut scores: Vec<(String, f32)> = vectors
            .iter()
            .map(|(id, v)| (id.clone(), super::cosine_similarity(query, v)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.truncate(top_k);
        Ok(scores)
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    fn dims(&self) -> Option<usize> {
        self.dims
    }

    async fn stats(&self) -> Result<VectorIndexStats> {
        let count = self.vectors.read().unwrap().len() as u64;
        Ok(VectorIndexStats {
            vector_count: count,
            index_bytes: 0,
        })
    }
}

