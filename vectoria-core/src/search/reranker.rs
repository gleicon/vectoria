use anyhow::{Context, Result};
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use std::sync::Mutex;

/// Cross-encoder reranker using fastembed TextRerank.
/// Default model: BAAI/bge-reranker-base (~80MB quantized).
/// Opt-in per query: `"rerank": true` in SearchRequest.
pub struct CrossEncoderReranker {
    model: Mutex<TextRerank>,
}

impl CrossEncoderReranker {
    pub fn new() -> Result<Self> {
        let model = TextRerank::try_new(
            RerankInitOptions::new(RerankerModel::BGERerankerBase)
                .with_show_download_progress(true),
        )
        .context("failed to initialize cross-encoder reranker")?;
        Ok(Self { model: Mutex::new(model) })
    }

    /// Rerank (query, document_text) pairs. Returns indices in new ranked order.
    /// Input: query + Vec<document_text>
    /// Output: Vec<(original_index, rerank_score)> sorted by score desc.
    pub fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<(usize, f32)>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.model.lock().unwrap();
        let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();
        let results = model
            .rerank(query, doc_refs, true, None)
            .context("reranking failed")?;

        let mut scored: Vec<(usize, f32)> = results
            .into_iter()
            .map(|r| (r.index, r.score as f32))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(scored)
    }
}
