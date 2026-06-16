use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;
    async fn flush(&self) -> Result<()>;
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

pub(super) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    dot / (norm_a * norm_b)
}

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Default)]
pub(super) struct BruteForceStore {
    pub(super) vectors: RwLock<HashMap<String, Vec<f32>>>,
}

impl BruteForceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, id: &str, vector: &[f32]) {
        self.vectors.write().unwrap().insert(id.to_string(), vector.to_vec());
    }

    pub fn delete(&self, id: &str) {
        self.vectors.write().unwrap().remove(id);
    }

    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(String, f32)> {
        let vectors = self.vectors.read().unwrap();
        let mut scores: Vec<(String, f32)> = vectors
            .iter()
            .map(|(id, v)| (id.clone(), cosine_similarity(query, v)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }

    pub fn len(&self) -> usize {
        self.vectors.read().unwrap().len()
    }

}
