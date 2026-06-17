use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn model_id(&self) -> &str;
    fn dims(&self) -> usize;
}

pub mod cache;
pub mod local;
pub mod openai;
