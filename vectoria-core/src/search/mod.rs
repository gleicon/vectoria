pub mod bm25_index;
pub mod query_cache;
pub mod reranker;
pub mod spell;

use crate::{
    embedding::{build_product_text, EmbeddingProvider},
    model::{
        Event, Hit, Product, ProductStatus, RankingWeights, ScoreBreakdown, ScoreFactor,
        SearchMode, SearchRequest, SearchResponse, SimilarRequest,
    },
    storage::StorageEngine,
    vector::VectorIndex,
};
use anyhow::{bail, Result};
use bm25_index::Bm25Index;
use query_cache::QueryResultCache;
use reranker::CrossEncoderReranker;
use spell::SpellCorrector;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const LATENCY_WINDOW: usize = 1000;
const MAX_LIMIT: usize = 1_000;
const MAX_OFFSET: usize = 10_000;
const MAX_AGGREGATE_FIELDS: usize = 20;

pub struct SearchEngine {
    storage: Arc<dyn StorageEngine>,
    vector_index: Arc<dyn VectorIndex>,
    embedding: Arc<dyn EmbeddingProvider>,
    default_weights: RankingWeights,
    bm25: Arc<Bm25Index>,
    spell: Arc<SpellCorrector>,
    reranker: Option<Arc<CrossEncoderReranker>>,
    query_cache: Option<Arc<QueryResultCache>>,
    query_count: Arc<AtomicU64>,
    latency_window: Arc<Mutex<VecDeque<u32>>>,
}

impl SearchEngine {
    pub fn new(
        storage: Arc<dyn StorageEngine>,
        vector_index: Arc<dyn VectorIndex>,
        embedding: Arc<dyn EmbeddingProvider>,
        default_weights: RankingWeights,
    ) -> Self {
        Self {
            storage,
            vector_index,
            embedding,
            default_weights,
            bm25: Arc::new(Bm25Index::new()),
            spell: Arc::new(SpellCorrector::new()),
            reranker: None,
            query_cache: None,
            query_count: Arc::new(AtomicU64::new(0)),
            latency_window: Arc::new(Mutex::new(VecDeque::with_capacity(LATENCY_WINDOW))),
        }
    }

    pub fn with_reranker(mut self, reranker: CrossEncoderReranker) -> Self {
        self.reranker = Some(Arc::new(reranker));
        self
    }

    pub fn with_query_cache(mut self, ttl_secs: u64, max_entries: usize) -> Self {
        self.query_cache = Some(Arc::new(QueryResultCache::new(ttl_secs, max_entries)));
        self
    }

    pub async fn index(&self, mut product: Product) -> Result<()> {
        if let Some(stored_model) = &product.model_id {
            let current_model = self.embedding.model_id();
            if stored_model != current_model {
                bail!(
                    "vector model mismatch: stored '{}', current '{}'. \
                     Run `vectoria reindex --model {}` to migrate.",
                    stored_model, current_model, current_model
                );
            }
        }

        let product_text = product
            .text
            .clone()
            .unwrap_or_else(|| build_product_text(&product.metadata));

        self.spell.add_text(&product_text);
        self.bm25.upsert(&product.id, &product_text);

        if product.vector.is_none() {
            let vector = self.embedding.embed(&product_text).await?;
            product.vector = Some(vector.clone());
            product.model_id = Some(self.embedding.model_id().to_string());
            product.dims = Some(self.embedding.dims());
            self.vector_index.upsert(&product.id, &vector).await?;
        } else if let Some(vector) = &product.vector {
            product.model_id.get_or_insert_with(|| self.embedding.model_id().to_string());
            product.dims.get_or_insert(vector.len());
            self.vector_index.upsert(&product.id, vector).await?;
        }

        product.status = ProductStatus::Indexed;
        self.storage.put_product(&product).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        self.vector_index.delete(id).await?;
        self.bm25.remove(id);
        self.storage.delete_product(id).await?;
        Ok(())
    }

    pub async fn search(&self, req: SearchRequest) -> Result<SearchResponse> {
        let cacheable = !req.explain && !req.rerank && req.aggregate.is_none() && req.ranking_weights.is_none();

        let cache_key = if cacheable {
            if let Some(cache) = &self.query_cache {
                let key = make_cache_key(&req);
                if let Some(cached) = cache.get(&key) {
                    return Ok(cached);
                }
                Some(key)
            } else {
                None
            }
        } else {
            None
        };

        let start = Instant::now();
        let weights = req.ranking_weights.clone().unwrap_or_else(|| self.default_weights.clone());
        let limit = req.limit.min(MAX_LIMIT);
        let offset = req.offset.min(MAX_OFFSET);
        let candidate_k = (limit + offset) * 5;

        let query_vector = match req.mode {
            SearchMode::Bm25 => None,
            _ => Some(self.embedding.embed(&req.q).await?),
        };

        let mut candidate_scores: HashMap<String, CandidateScore> = HashMap::new();

        if let Some(ref qv) = query_vector {
            for (id, semantic_score) in self.vector_index.search(qv, candidate_k).await? {
                candidate_scores
                    .entry(id)
                    .or_default()
                    .semantic = semantic_score;
            }
        }

        // effective_q starts as the original; falls back to spell-corrected only when BM25
        // returns zero results (preserves precision for well-formed queries).
        let effective_q;
        if matches!(req.mode, SearchMode::Hybrid | SearchMode::Bm25) {
            let bm25_results = self.bm25.search(&req.q, candidate_k);

            let base_q = if bm25_results.is_empty() {
                let corrected = self.spell.correct(&req.q);
                if corrected != req.q { corrected } else { req.q.clone() }
            } else {
                req.q.clone()
            };

            let expanded_q = if bm25_results.len() < (limit / 2).max(1)
                && !candidate_scores.is_empty()
            {
                let expansion_terms = self.expand_query_terms(&base_q, &candidate_scores).await;
                if expansion_terms.is_empty() {
                    base_q.clone()
                } else {
                    format!("{} {}", base_q, expansion_terms.join(" "))
                }
            } else {
                base_q.clone()
            };
            let final_bm25 = if expanded_q != req.q {
                self.bm25.search(&expanded_q, candidate_k)
            } else {
                bm25_results
            };

            let max_bm25 = final_bm25.iter().map(|(_, s)| *s).fold(0.0f32, f32::max);
            for (id, raw_score) in final_bm25 {
                let normalized = if max_bm25 > 0.0 { raw_score / max_bm25 } else { 0.0 };
                candidate_scores.entry(id).or_default().bm25 = normalized;
            }
            effective_q = expanded_q;
        } else {
            effective_q = req.q.clone();
        }

        let mut hits: Vec<Hit> = Vec::new();
        for (id, candidate) in candidate_scores {
            let Some(product) = self.storage.get_product(&id).await? else { continue };
            if let Some(filters) = &req.filters {
                if !matches_filters(&product.metadata, filters) { continue; }
            }

            let signals = self.storage.get_product_signals(&id).await?;
            let availability = product.metadata.get("in_stock")
                .and_then(|v| v.as_bool()).unwrap_or(true) as u8 as f32;
            let margin = product.metadata.get("margin")
                .and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

            let score = candidate.semantic * weights.semantic
                + candidate.bm25 * weights.bm25
                + signals.popularity * weights.popularity
                + availability * weights.availability
                + margin * weights.margin;

            let explain = req.explain.then(|| ScoreBreakdown {
                factors: vec![
                    ScoreFactor { factor: "semantic_similarity".into(), score: candidate.semantic, weight: weights.semantic },
                    ScoreFactor { factor: "bm25".into(), score: candidate.bm25, weight: weights.bm25 },
                    ScoreFactor { factor: "popularity".into(), score: signals.popularity, weight: weights.popularity },
                    ScoreFactor { factor: "availability".into(), score: availability, weight: weights.availability },
                    ScoreFactor { factor: "margin".into(), score: margin, weight: weights.margin },
                ],
            });

            hits.push(Hit { id: product.id, score, metadata: product.metadata.clone(), explain });
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        if req.rerank {
            if let Some(reranker) = &self.reranker {
                let top_n = hits.len().min(50);
                let texts: Vec<String> = hits[..top_n]
                    .iter()
                    .map(|h| {
                        h.metadata.get("title")
                            .or_else(|| h.metadata.get("text"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                let reranked = reranker.rerank(&effective_q, &texts)?;
                let reranked_hits: Vec<Hit> = reranked
                    .into_iter()
                    .filter_map(|(idx, _score)| hits.get(idx).cloned())
                    .collect();
                hits.splice(..top_n, reranked_hits);
            }
        }

        let total = hits.len();
        let page_hits: Vec<Hit> = hits.into_iter().skip(offset).take(limit).collect();

        let aggregations = req.aggregate.as_ref().map(|fields| {
            let capped: Vec<String> = fields.iter().take(MAX_AGGREGATE_FIELDS).cloned().collect();
            compute_aggregations(&page_hits, &capped)
        });

        let response = SearchResponse {
            total,
            offset,
            limit,
            processing_time_ms: start.elapsed().as_millis() as u64,
            query: req.q,
            hits: page_hits,
            aggregations,
        };

        if let (Some(key), Some(cache)) = (cache_key, &self.query_cache) {
            cache.put(key, response.clone());
        }

        let elapsed_ms = response.processing_time_ms as u32;
        self.query_count.fetch_add(1, Ordering::Relaxed);
        {
            let mut win = self.latency_window.lock().unwrap();
            if win.len() >= LATENCY_WINDOW {
                win.pop_front();
            }
            win.push_back(elapsed_ms);
        }

        Ok(response)
    }

    pub async fn similar(&self, req: SimilarRequest) -> Result<Vec<Hit>> {
        let query_vector = if let Some(v) = req.vector {
            v
        } else if let Some(text) = req.text {
            self.embedding.embed(&text).await?
        } else if let Some(id) = req.product_id {
            let product = self.storage.get_product(&id).await?;
            match product.and_then(|p| p.vector) {
                Some(v) => v,
                None => bail!("product '{}' not found or has no vector", id),
            }
        } else {
            bail!("similar request must include text, vector, or product_id");
        };

        let sim_limit = req.limit.min(MAX_LIMIT);
        let candidates = self.vector_index.search(&query_vector, sim_limit * 5).await?;
        let mut hits = Vec::new();
        for (id, score) in candidates {
            let Some(product) = self.storage.get_product(&id).await? else { continue };
            if let Some(filters) = &req.filters {
                if !matches_filters(&product.metadata, filters) { continue; }
            }
            hits.push(Hit { id: product.id, score, metadata: product.metadata, explain: None });
            if hits.len() >= sim_limit { break; }
        }
        Ok(hits)
    }

    pub async fn record_event(&self, event: Event) -> Result<()> {
        self.storage.put_event(&event).await
    }

    async fn expand_query_terms(
        &self,
        original_query: &str,
        candidates: &HashMap<String, CandidateScore>,
    ) -> Vec<String> {
        let mut top: Vec<(&String, f32)> = candidates
            .iter()
            .map(|(id, s)| (id, s.semantic))
            .collect();
        top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        top.truncate(3);

        let original_tokens: std::collections::HashSet<String> = original_query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();

        let mut expansion = Vec::new();
        let mut seen: std::collections::HashSet<String> = original_tokens.clone();
        for (id, _) in top {
            let Ok(Some(product)) = self.storage.get_product(id).await else { continue };
            let text = product.text.unwrap_or_else(|| build_product_text(&product.metadata));
            for word in text.split_whitespace() {
                let lower = word.to_lowercase().trim_matches(|c: char| !c.is_alphabetic()).to_string();
                if lower.len() >= 3 && !seen.contains(&lower) {
                    seen.insert(lower.clone());
                    expansion.push(lower);
                    if expansion.len() >= 5 { break; }
                }
            }
            if expansion.len() >= 5 { break; }
        }
        expansion
    }

    pub async fn stats(&self) -> Result<EngineStats> {
        let storage_stats = self.storage.stats().await?;
        let vector_stats = self.vector_index.stats().await?;
        let query_count = self.query_count.load(Ordering::Relaxed);
        let latency_p95_ms = {
            let win = self.latency_window.lock().unwrap();
            percentile_p95(&win)
        };
        Ok(EngineStats {
            product_count: storage_stats.product_count,
            event_count: storage_stats.event_count,
            storage_bytes: storage_stats.storage_bytes,
            vector_count: vector_stats.vector_count,
            bm25_document_count: self.bm25.len() as u64,
            model_id: self.embedding.model_id().to_string(),
            dims: self.embedding.dims(),
            query_count,
            latency_p95_ms,
        })
    }

    pub async fn reindex_all(&self) -> Result<ReindexReport> {
        let mut offset = 0usize;
        const BATCH: usize = 100;
        let mut reindexed = 0usize;
        let mut errors = 0usize;

        loop {
            let products = self.storage.list_products(offset, BATCH).await?;
            if products.is_empty() { break; }
            let count = products.len();
            for product in products {
                match self.index(product).await {
                    Ok(_) => reindexed += 1,
                    Err(e) => {
                        errors += 1;
                        tracing::warn!(error = %e, "reindex: skipped product");
                    }
                }
            }
            offset += count;
            if count < BATCH { break; }
        }
        self.vector_index.flush().await?;
        Ok(ReindexReport { reindexed, errors })
    }
}

#[derive(serde::Serialize)]
pub struct EngineStats {
    pub product_count: u64,
    pub event_count: u64,
    pub storage_bytes: u64,
    pub vector_count: u64,
    pub bm25_document_count: u64,
    pub model_id: String,
    pub dims: usize,
    pub query_count: u64,
    pub latency_p95_ms: u32,
}

fn percentile_p95(window: &VecDeque<u32>) -> u32 {
    if window.is_empty() { return 0; }
    let mut sorted: Vec<u32> = window.iter().copied().collect();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 * 0.95) as usize).saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

#[derive(serde::Serialize)]
pub struct ReindexReport {
    pub reindexed: usize,
    pub errors: usize,
}

#[derive(Default)]
struct CandidateScore {
    semantic: f32,
    bm25: f32,
}

fn matches_filters(metadata: &serde_json::Value, filters: &HashMap<String, serde_json::Value>) -> bool {
    for (key, expected) in filters {
        if key == "price_max" {
            let price = metadata.get("price").and_then(|v| v.as_f64()).unwrap_or(f64::MAX);
            if let Some(max) = expected.as_f64() { if price > max { return false; } }
            continue;
        }
        if key == "price_min" {
            let price = metadata.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if let Some(min) = expected.as_f64() { if price < min { return false; } }
            continue;
        }
        if metadata.get(key) != Some(expected) { return false; }
    }
    true
}

fn make_cache_key(req: &SearchRequest) -> String {
    let filters = req.filters.as_ref().map(|f| {
        let mut pairs: Vec<_> = f.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        serde_json::to_string(&pairs).unwrap_or_default()
    }).unwrap_or_default();
    format!("{}|{:?}|{}|{}|{}", req.q, req.mode, req.limit, req.offset, filters)
}

fn compute_aggregations(hits: &[Hit], fields: &[String]) -> HashMap<String, HashMap<String, usize>> {
    let mut aggs: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for field in fields {
        let counts = aggs.entry(field.clone()).or_default();
        for hit in hits {
            if let Some(v) = hit.metadata.get(field).and_then(|v| v.as_str()) {
                *counts.entry(v.to_string()).or_insert(0) += 1;
            }
        }
    }
    aggs
}
