use super::EmbeddingProvider;
use anyhow::{Context, Result};
use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

/// Local ONNX embedding via fastembed-rs.
/// Default model: multilingual-e5-small (EN + PT-BR, ~40MB quantized).
pub struct LocalEmbedding {
    model: Mutex<TextEmbedding>,
    model_id: String,
    dims: usize,
}

impl LocalEmbedding {
    pub fn new(model: EmbeddingModel) -> Result<Self> {
        let model_id = format!("{:?}", model);
        let dims = model_dims(&model);
        let embedding = TextEmbedding::try_new(
            InitOptions::new(model).with_show_download_progress(true),
        )
        .context("failed to initialize local embedding model")?;
        Ok(Self {
            model: Mutex::new(embedding),
            model_id,
            dims,
        })
    }

    /// Default: multilingual-e5-small — EN + PT-BR coverage.
    pub fn default_model() -> Result<Self> {
        Self::new(EmbeddingModel::MultilingualE5Small)
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text];
        let model = self.model.lock().unwrap();
        let mut results = model
            .embed(texts, None)
            .context("embedding failed")?;
        Ok(results.remove(0))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let model = self.model.lock().unwrap();
        model
            .embed(texts.to_vec(), None)
            .context("batch embedding failed")
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dims(&self) -> usize {
        self.dims
    }
}

fn model_dims(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::MultilingualE5Small => 384,
        EmbeddingModel::BGESmallENV15 => 384,
        EmbeddingModel::BGEBaseENV15 => 768,
        EmbeddingModel::BGELargeENV15 => 1024,
        _ => 384,
    }
}
