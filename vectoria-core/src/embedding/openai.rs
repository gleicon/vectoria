use super::EmbeddingProvider;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// OpenAI-compatible embedding provider.
/// Works with: Ollama, llama.cpp server, vLLM, LM Studio, OpenAI API.
pub struct OpenAIEmbedding {
    base_url: String,
    model: String,
    api_key: Option<String>,
    dims: usize,
    client: reqwest::Client,
}

impl OpenAIEmbedding {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
        dims: usize,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key,
            dims,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text]).await?;
        Ok(results.remove(0))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut req = self.client.post(format!("{}/v1/embeddings", self.base_url))
            .json(&EmbedRequest {
                model: &self.model,
                input: texts.to_vec(),
            });
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp: EmbedResponse = req
            .send()
            .await
            .context("embedding request failed")?
            .error_for_status()
            .context("embedding API error")?
            .json()
            .await
            .context("failed to parse embedding response")?;
        Ok(resp.data.into_iter().map(|d| d.embedding).collect())
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn dims(&self) -> usize {
        self.dims
    }
}
