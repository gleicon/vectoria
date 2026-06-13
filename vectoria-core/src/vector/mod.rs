use anyhow::Result;
use async_trait::async_trait;

/// Abstraction over the ANN vector index.
/// Default: EdgeStore HNSW. Alternative: TurboVec (online quantization).
#[async_trait]
pub trait VectorIndex: Send + Sync {
    /// Insert or update a vector for the given product ID.
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()>;
    /// Remove a vector by product ID.
    async fn delete(&self, id: &str) -> Result<()>;
    /// Find the top-k nearest neighbors. Returns (product_id, cosine_score).
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;
    /// Persist index state to disk.
    async fn flush(&self) -> Result<()>;
    /// Model ID this index was built with (dimension check on startup).
    fn model_id(&self) -> Option<&str>;
    fn dims(&self) -> Option<usize>;
    async fn stats(&self) -> Result<VectorIndexStats>;
}

#[derive(Debug, Default)]
pub struct VectorIndexStats {
    pub vector_count: u64,
    pub index_bytes: u64,
}

pub mod edgestore;
pub mod memory;
pub mod turbovec;

pub(super) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    dot / (norm_a * norm_b)
}
