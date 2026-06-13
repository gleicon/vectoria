/// File-persisted flat vector index for catalogs up to ~50K products.
/// Activate: `[index] vector_backend = "turbovec"` in vectoria.toml.
use super::{VectorIndex, VectorIndexStats};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub struct TurboVecIndex {
    vectors: RwLock<HashMap<String, Vec<f32>>>,
    model_id: Option<String>,
    dims: Option<usize>,
    persist_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct TurboVecSnapshot {
    model_id: Option<String>,
    dims: Option<usize>,
    vectors: HashMap<String, Vec<f32>>,
}

impl TurboVecIndex {
    /// Create a new ephemeral index (no persistence).
    pub fn new(model_id: Option<String>, dims: Option<usize>) -> Self {
        Self {
            vectors: RwLock::new(HashMap::new()),
            model_id,
            dims,
            persist_path: None,
        }
    }

    /// Open or create a file-persisted index at `path`.
    pub fn open(path: &Path, model_id: Option<String>, dims: Option<usize>) -> Result<Self> {
        let (loaded_vectors, loaded_model, loaded_dims) = if path.exists() {
            let raw = std::fs::read(path)
                .with_context(|| format!("failed to read TurboVec snapshot at {:?}", path))?;
            let snap: TurboVecSnapshot = serde_json::from_slice(&raw)
                .with_context(|| "failed to parse TurboVec snapshot")?;
            (snap.vectors, snap.model_id, snap.dims)
        } else {
            (HashMap::new(), model_id, dims)
        };

        Ok(Self {
            vectors: RwLock::new(loaded_vectors),
            model_id: loaded_model,
            dims: loaded_dims,
            persist_path: Some(path.to_path_buf()),
        })
    }
}

#[async_trait]
impl VectorIndex for TurboVecIndex {
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
        let Some(ref path) = self.persist_path else { return Ok(()); };
        let vectors = self.vectors.read().unwrap().clone();
        let snap = TurboVecSnapshot {
            model_id: self.model_id.clone(),
            dims: self.dims,
            vectors,
        };
        let data = serde_json::to_vec(&snap)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &data)
            .with_context(|| format!("failed to write TurboVec snapshot to {:?}", tmp))?;
        std::fs::rename(&tmp, path)
            .with_context(|| "failed to rename TurboVec snapshot")?;
        Ok(())
    }

    fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    fn dims(&self) -> Option<usize> {
        self.dims
    }

    async fn stats(&self) -> Result<VectorIndexStats> {
        let vectors = self.vectors.read().unwrap();
        let vector_count = vectors.len() as u64;
        let index_bytes = vectors
            .iter()
            .map(|(k, v)| k.len() + v.len() * 4)
            .sum::<usize>() as u64;
        Ok(VectorIndexStats { vector_count, index_bytes })
    }
}

