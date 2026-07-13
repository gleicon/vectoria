/// LLM-based query rewriter for ecommerce search.
///
/// Calls any OpenAI-compatible `/v1/chat/completions` endpoint and asks it to
/// rephrase the original query into 2-3 alternative product-search phrases.
/// Only used when BM25 returns sparse results (< threshold); good queries skip it.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct LlmRewriter {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl LlmRewriter {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key,
        }
    }

    /// Rewrite `query` into alternative search phrases.
    /// Returns the joined rewritten terms, or the original query on any error so the
    /// pipeline is never blocked by a failing LLM endpoint.
    pub async fn rewrite(&self, query: &str) -> String {
        match self.rewrite_inner(query).await {
            Ok(rewritten) if !rewritten.trim().is_empty() => rewritten,
            _ => query.to_string(),
        }
    }

    async fn rewrite_inner(&self, query: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Req {
            model: String,
            messages: Vec<Message>,
            max_tokens: u32,
            temperature: f32,
        }
        #[derive(Serialize)]
        struct Message { role: &'static str, content: String }
        #[derive(Deserialize)]
        struct Resp { choices: Vec<Choice> }
        #[derive(Deserialize)]
        struct Choice { message: MsgOut }
        #[derive(Deserialize)]
        struct MsgOut { content: String }

        let system = "You rewrite ecommerce search queries. \
            Given a query, output 2-3 alternative search phrases that describe the same \
            product intent. Output only the phrases, one per line, no explanations.";

        let body = Req {
            model: self.model.clone(),
            messages: vec![
                Message { role: "system", content: system.to_string() },
                Message { role: "user",   content: format!("Query: {}", query) },
            ],
            max_tokens: 60,
            temperature: 0.3,
        };

        let mut req = self.client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body);

        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp: Resp = req.send().await.context("llm request failed")?
            .json().await.context("llm response parse failed")?;

        let raw = resp.choices.into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        // Join all non-empty lines into a single expanded query string.
        let joined = raw.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(joined)
    }
}
