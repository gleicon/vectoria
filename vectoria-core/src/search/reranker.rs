use anyhow::{Context, Result};
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use std::sync::Mutex;

/// Result from any reranker backend.
/// `grade` and `confidence` are only populated by System One backends (LayaReranker).
/// `CrossEncoderReranker` leaves both as `None`.
pub struct RerankResult {
    pub index: usize,
    pub score: f32,
    /// Relevance grade 0–3 (0=irrelevant, 1=marginal, 2=relevant, 3=exact).
    /// Set by LayaReranker; None for cross-encoder.
    pub grade: Option<u8>,
    /// Calibrated confidence for the grade (0.0–1.0, softmax over grade logits).
    pub confidence: Option<f32>,
}

/// Reranker backend abstraction. Implementations: CrossEncoderReranker, LayaReranker.
pub trait Reranker: Send + Sync {
    /// Rerank (query, documents) pairs. Returns results in descending score order.
    /// `documents` must correspond to the same positional index as the caller's hit list.
    fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<RerankResult>>;
}

// ── Cross-encoder (BGERerankerBase via fastembed) ───────────────────────────

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
}

impl Reranker for CrossEncoderReranker {
    fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<RerankResult>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.model.lock().unwrap();
        let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();
        let results = model
            .rerank(query, doc_refs, true, None)
            .context("reranking failed")?;

        let mut scored: Vec<RerankResult> = results
            .into_iter()
            .map(|r| RerankResult { index: r.index, score: r.score as f32, grade: None, confidence: None })
            .collect();
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored)
    }
}

// ── Laya (System One, [MASK]-token classifier via ort + tokenizers) ─────────

/// System One reranker using laya-onnx (ModernBERT-large 421M params, Q8 ~524 MB).
///
/// # Setup
/// Download the Q8 model from HuggingFace before use:
/// ```sh
/// pip install huggingface_hub
/// huggingface-cli download nvkudva/laya-onnx-q8 --local-dir ./laya-q8
/// ```
///
/// Then set `laya_model_path = "./laya-q8"` in `[index]` of `vectoria.toml`,
/// or pass `VECTORIA_LAYA_MODEL_PATH=./laya-q8` as an environment variable.
///
/// Full-precision alternatives (1.6 GB):
///   - `mariojcr/laya-onnx`
///   - `desplega/laya-onnx`
///   - `codenamev/laya-onnx` (English / multilingual / typed-decisions subfolders)
///
/// # Prompt format
/// `score question: How relevant is this product to the query? [SEP] [MASK] irrelevant marginal relevant exact [SEP] {"query":"…","title":"…","description":"…"}`
///
/// The model outputs logits over the four grade labels at the [MASK] position.
/// Grade 0=irrelevant, 1=marginal, 2=relevant, 3=exact.
pub struct LayaReranker {
    #[cfg(feature = "laya")]
    session: ort::Session,
    #[cfg(feature = "laya")]
    tokenizer: tokenizers::Tokenizer,
}

#[cfg(feature = "laya")]
impl LayaReranker {
    /// Load a laya ONNX model from a local directory.
    /// The directory must contain `model.onnx` (or `model_quantized.onnx`) and `tokenizer.json`.
    pub fn from_model_dir(dir: &std::path::Path) -> Result<Self> {
        use ort::Session;
        use tokenizers::Tokenizer;

        // Try quantized model first, fall back to full-precision.
        let model_file = ["model_quantized.onnx", "model.onnx"]
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.exists())
            .ok_or_else(|| anyhow::anyhow!(
                "laya model not found in {:?}. Expected model.onnx or model_quantized.onnx.\n\
                 Download with: huggingface-cli download nvkudva/laya-onnx-q8 --local-dir {}",
                dir, dir.display()
            ))?;

        let tokenizer_file = dir.join("tokenizer.json");
        anyhow::ensure!(
            tokenizer_file.exists(),
            "laya tokenizer not found at {:?}",
            tokenizer_file
        );

        let session = Session::builder()
            .context("failed to create ort session builder")?
            .commit_from_file(&model_file)
            .context("failed to load laya ONNX model")?;

        let tokenizer = Tokenizer::from_file(&tokenizer_file)
            .map_err(|e| anyhow::anyhow!("failed to load laya tokenizer: {e}"))?;

        Ok(Self { session, tokenizer })
    }

    fn build_prompt(query: &str, title: &str, description: &str) -> String {
        // Laya prompt format: score question: ... [SEP] [MASK] <opts> [SEP] <state>
        // The tokenizer handles [CLS]/[SEP]/[MASK] as special tokens.
        let state = serde_json::json!({
            "query": query,
            "title": title,
            "description": description
        })
        .to_string();
        format!(
            "score question: How relevant is this product to the query? [SEP] [MASK] irrelevant marginal relevant exact [SEP] {state}"
        )
    }

    fn run_inference(&self, prompt: &str) -> Result<RerankResult> {
        use ndarray::{Array2, s};
        use ort::inputs;

        let encoding = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| anyhow::anyhow!("tokenizer encode error: {e}"))?;

        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&x| x as i64).collect();
        let seq_len = ids.len();

        let input_ids = Array2::from_shape_vec((1, seq_len), ids)
            .context("failed to shape input_ids")?;
        let attention_mask = Array2::from_shape_vec((1, seq_len), mask)
            .context("failed to shape attention_mask")?;

        let outputs = self.session.run(inputs! {
            "input_ids" => input_ids.view(),
            "attention_mask" => attention_mask.view(),
        }?)
        .context("laya inference failed")?;

        // Output tensor "logits": shape [1, 4] over {irrelevant, marginal, relevant, exact}.
        // If the model uses a different output name, inspect with:
        //   session.outputs.iter().for_each(|o| eprintln!("{}", o.name));
        let logits_view = outputs["logits"]
            .try_extract_tensor::<f32>()
            .context("failed to extract logits from laya output")?;
        let logits = logits_view.view();
        let row = logits.slice(s![0, ..]);
        let scores: Vec<f32> = row.to_vec();

        // Softmax + argmax.
        let max = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp: Vec<f32> = scores.iter().map(|&x| (x - max).exp()).collect();
        let sum: f32 = exp.iter().sum();
        let probs: Vec<f32> = exp.iter().map(|&x| x / sum).collect();

        let (grade, confidence) = probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, &p)| (i as u8, p))
            .unwrap_or((0, 0.0));

        // Score: weighted sum of grade × probability (expected grade ∈ [0,3]).
        let score: f32 = probs.iter().enumerate().map(|(i, &p)| i as f32 * p).sum();

        Ok(RerankResult { index: 0, score, grade: Some(grade), confidence: Some(confidence) })
    }
}

#[cfg(feature = "laya")]
impl Reranker for LayaReranker {
    fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<RerankResult>> {
        // Documents are expected as "title\n{description}" or just titles.
        // Split on first newline to separate title from description.
        let mut results: Vec<RerankResult> = documents
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let (title, description) = doc.split_once('\n').unwrap_or((doc.as_str(), ""));
                let prompt = Self::build_prompt(query, title, description);
                let mut res = self.run_inference(&prompt)?;
                res.index = i;
                Ok(res)
            })
            .collect::<Result<Vec<_>>>()?;

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }
}

// Stub when feature is disabled so the type still exists for config/builder references.
#[cfg(not(feature = "laya"))]
impl LayaReranker {
    pub fn from_model_dir(_dir: &std::path::Path) -> Result<Self> {
        anyhow::bail!(
            "LayaReranker requires the `laya` cargo feature. \
             Rebuild with `--features laya` and download the model: \
             huggingface-cli download nvkudva/laya-onnx-q8 --local-dir ./laya-q8"
        )
    }
}

#[cfg(not(feature = "laya"))]
impl Reranker for LayaReranker {
    fn rerank(&self, _query: &str, _documents: &[String]) -> Result<Vec<RerankResult>> {
        anyhow::bail!("LayaReranker is disabled; build with --features laya")
    }
}
