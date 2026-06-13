use anyhow::Result;
use async_trait::async_trait;

/// Abstraction over embedding providers.
/// Implementations: local ONNX (fastembed-rs), OpenAI-compatible HTTP API.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text string.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed a batch of texts (more efficient than N individual calls).
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Model identifier (e.g. "multilingual-e5-small").
    fn model_id(&self) -> &str;
    /// Output embedding dimensions.
    fn dims(&self) -> usize;
}

/// Build a structured product text for embedding.
/// Combines title, brand, category, and attributes into a single string.
/// Better than embedding title alone — semantic space captures product intent.
pub fn build_product_text(metadata: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    for field in &["title", "name", "brand", "category", "description"] {
        if let Some(v) = metadata.get(field).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                parts.push(v.to_string());
            }
        }
    }

    if let Some(attrs) = metadata.get("attributes").and_then(|v| v.as_object()) {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                parts.push(format!("{}: {}", k, s));
            }
        }
    }

    parts.join(". ")
}

pub mod cache;
pub mod local;
pub mod openai;
