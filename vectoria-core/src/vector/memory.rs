use super::{BruteForceStore, VectorIndex, VectorIndexStats};
use anyhow::Result;
use async_trait::async_trait;

pub struct MemoryVectorIndex {
    store: BruteForceStore,
    model_id: Option<String>,
    dims: Option<usize>,
}

impl MemoryVectorIndex {
    pub fn new(model_id: Option<String>, dims: Option<usize>) -> Self {
        Self { store: BruteForceStore::new(), model_id, dims }
    }
}

#[async_trait]
impl VectorIndex for MemoryVectorIndex {
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()> {
        self.store.upsert(id, vector);
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.store.delete(id);
        Ok(())
    }

    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f32)>> {
        Ok(self.store.search(query, top_k))
    }

    async fn flush(&self) -> Result<()> { Ok(()) }

    fn model_id(&self) -> Option<&str> { self.model_id.as_deref() }
    fn dims(&self) -> Option<usize> { self.dims }

    async fn stats(&self) -> Result<VectorIndexStats> {
        Ok(VectorIndexStats { vector_count: self.store.len() as u64, index_bytes: 0 })
    }
}
