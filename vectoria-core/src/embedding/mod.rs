use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn model_id(&self) -> &str;
    fn dims(&self) -> usize;
}

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
