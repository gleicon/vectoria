/// Vectoria WASM — minimal search engine for edge compute.
///
/// Target: `wasm32-unknown-unknown` (Cloudflare Workers, Deno Deploy, browsers).
///
/// Architecture:
/// - In-memory corpus (Vec of (id, text, vector, metadata))
/// - BM25 via `bm25::SearchEngineBuilder::with_corpus()` (rebuild on each search — fine at edge scale)
/// - Vector: brute-force cosine similarity (no HNSW needed at <10k docs)
/// - Embedding: OpenAI-compatible HTTP endpoint via JS fetch (`reqwest` wasm feature)
/// - Interior mutability via `RefCell` — WASM is single-threaded
///
/// # Quick start (Cloudflare Worker / Deno)
/// ```js
/// import init, { VectoriaWasm } from "./vectoria_wasm.js";
/// await init();
/// const v = VectoriaWasm.new(JSON.stringify({
///   embedding_base_url: "https://your-api",
///   embedding_model: "text-embedding-3-small",
///   embedding_api_key: "sk-...",
///   dims: 384,
/// }));
/// await v.index('{"id":"p1","text":"running shoes","metadata":{}}');
/// const res = await v.search('{"q":"shoes","limit":10}');
/// console.log(JSON.parse(res).hits);
/// ```
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use serde::{Deserialize, Serialize};

// ── Config ────────────────────────────────────────────────────────────────────

/// Configuration passed to [`VectoriaWasm::new`] as JSON.
///
/// All fields are optional except that vector search requires either
/// `embedding_base_url` (remote embedding) or pre-computed vectors in each
/// indexed product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    /// Base URL of an OpenAI-compatible embedding endpoint (e.g. `"https://api.openai.com"`).
    /// Required for automatic embedding; omit if you supply vectors directly.
    pub embedding_base_url: Option<String>,
    /// Embedding model name passed in the request body. Default: `"text-embedding-3-small"`.
    #[serde(default = "default_model")]
    pub embedding_model: String,
    /// Bearer token sent as `Authorization: Bearer <key>`. Optional for private endpoints.
    pub embedding_api_key: Option<String>,
    /// Expected vector dimension. Must match the model. Default: `384`.
    #[serde(default = "default_dims")]
    pub dims: usize,
}

fn default_model() -> String { "text-embedding-3-small".into() }
fn default_dims() -> usize { 384 }

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            embedding_base_url: None,
            embedding_model: default_model(),
            embedding_api_key: None,
            dims: default_dims(),
        }
    }
}

// ── Request / Response types ─────────────────────────────────────────────────

/// A product to index. Pass as JSON to [`VectoriaWasm::index`].
///
/// Either `text` or `vector` (or both) should be provided.
/// When only `text` is given, the vector is fetched from `embedding_base_url`.
/// When only `vector` is given, BM25 falls back to the product `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmProduct {
    /// Stable product identifier. Must be unique within this engine instance.
    pub id: String,
    /// Human-readable text used for BM25 indexing and (optionally) auto-embedding.
    pub text: Option<String>,
    /// Pre-computed embedding. Skips the remote embedding call when provided.
    pub vector: Option<Vec<f32>>,
    /// Arbitrary JSON returned verbatim in search hits.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Search request. Pass as JSON to [`VectoriaWasm::search`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSearchRequest {
    /// Query string.
    pub q: String,
    /// Maximum number of hits to return. Default: 20, capped at 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Search mode. Default: `hybrid` (0.7 × semantic + 0.3 × BM25).
    #[serde(default)]
    pub mode: WasmSearchMode,
}

fn default_limit() -> usize { 20 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WasmSearchMode { #[default] Hybrid, Semantic, Bm25 }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WasmHit {
    id: String,
    score: f32,
    metadata: serde_json::Value,
}

// ── Remote embedding ─────────────────────────────────────────────────────────

async fn fetch_embedding(text: &str, cfg: &WasmConfig) -> Result<Vec<f32>, String> {
    let base = cfg.embedding_base_url.as_deref()
        .ok_or("embedding_base_url not set")?
        .trim_end_matches('/');
    let url = format!("{}/v1/embeddings", base);

    let body = serde_json::json!({ "input": text, "model": cfg.embedding_model });
    let mut builder = reqwest::Client::new().post(&url).json(&body);
    if let Some(key) = &cfg.embedding_api_key {
        builder = builder.header("Authorization", format!("Bearer {}", key));
    }

    let resp: serde_json::Value = builder.send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let vec: Vec<f32> = resp["data"][0]["embedding"]
        .as_array().ok_or("no embedding in response")?
        .iter().filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();
    if vec.is_empty() { return Err("empty embedding in response".into()); }
    Ok(vec)
}

// ── Vector math ──────────────────────────────────────────────────────────────

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() { return 0.0; }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { return 0.0; }
    (dot / (na * nb)).clamp(0.0, 1.0)
}

// ── VectoriaWasm ─────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct VectoriaWasm {
    config: WasmConfig,
    // id → (vector, metadata, text)
    store: RefCell<HashMap<String, (Vec<f32>, serde_json::Value, String)>>,
    // ordered list of (id, text) for BM25 corpus rebuild
    corpus: RefCell<Vec<(String, String)>>,
}

#[wasm_bindgen]
impl VectoriaWasm {
    /// Create a new engine. `config_json`: JSON-encoded `WasmConfig`.
    #[wasm_bindgen(constructor)]
    pub fn new(config_json: &str) -> Result<VectoriaWasm, JsValue> {
        let config: WasmConfig = serde_json::from_str(config_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(VectoriaWasm {
            config,
            store: RefCell::new(HashMap::new()),
            corpus: RefCell::new(Vec::new()),
        })
    }

    /// Index a product. Embeds text remotely if no vector is provided.
    /// `product_json`: JSON-encoded `WasmProduct`. Returns a JS `Promise<void>`.
    pub async fn index(&self, product_json: String) -> Result<(), JsValue> {
        let p: WasmProduct = serde_json::from_str(&product_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let text = p.text.clone().unwrap_or_else(|| {
            p.metadata.get("title").or(p.metadata.get("name"))
                .and_then(|v| v.as_str()).unwrap_or(&p.id).to_string()
        });

        let vector = if let Some(v) = p.vector {
            v
        } else {
            fetch_embedding(&text, &self.config).await.map_err(|e| JsValue::from_str(&e))?
        };

        let id = p.id.clone();
        let mut store = self.store.borrow_mut();
        let mut corpus = self.corpus.borrow_mut();
        if let Some(pos) = corpus.iter().position(|(k, _)| k == &id) {
            corpus[pos].1 = text.clone();
        } else {
            corpus.push((id.clone(), text.clone()));
        }
        store.insert(id, (vector, p.metadata, text));
        Ok(())
    }

    /// Remove a product by ID.
    pub fn delete(&self, product_id: &str) {
        self.store.borrow_mut().remove(product_id);
        self.corpus.borrow_mut().retain(|(k, _)| k != product_id);
    }

    /// Search the index. Returns a JS `Promise<string>` (JSON `{ hits: [...] }`).
    pub async fn search(&self, request_json: String) -> Result<String, JsValue> {
        let req: WasmSearchRequest = serde_json::from_str(&request_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let limit = req.limit.min(200);
        let candidate_k = limit * 5;
        let mut scores: HashMap<String, f32> = HashMap::new();

        if !matches!(req.mode, WasmSearchMode::Bm25) {
            let qv = fetch_embedding(&req.q, &self.config)
                .await.map_err(|e| JsValue::from_str(&e))?;
            let store = self.store.borrow();
            let mut vec_hits: Vec<(String, f32)> = store
                .iter()
                .map(|(id, (v, _, _))| (id.clone(), cosine(&qv, v)))
                .collect();
            vec_hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            vec_hits.truncate(candidate_k);
            for (id, s) in vec_hits {
                *scores.entry(id).or_insert(0.0) += 0.7 * s;
            }
        }

        if !matches!(req.mode, WasmSearchMode::Semantic) {
            let corpus = self.corpus.borrow();
            let texts: Vec<&str> = corpus.iter().map(|(_, t)| t.as_str()).collect();
            if !texts.is_empty() {
                use bm25::{Language, SearchEngineBuilder};
                let engine = SearchEngineBuilder::<u32>::with_corpus(Language::English, texts).build();
                for hit in engine.search(&req.q, candidate_k) {
                    if let Some((id, _)) = corpus.get(hit.document.id as usize) {
                        *scores.entry(id.clone()).or_insert(0.0) += 0.3 * hit.score;
                    }
                }
            }
        }

        let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        let store = self.store.borrow();
        let hits: Vec<WasmHit> = ranked.into_iter().filter_map(|(id, score)| {
            let (_, metadata, _) = store.get(&id)?;
            Some(WasmHit { id, score, metadata: metadata.clone() })
        }).collect();

        serde_json::to_string(&serde_json::json!({ "hits": hits }))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Number of indexed products.
    pub fn product_count(&self) -> usize {
        self.store.borrow().len()
    }
}
