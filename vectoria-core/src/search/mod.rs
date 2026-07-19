pub mod bm25_index;
pub mod clustering;
pub mod llm_rewriter;
pub mod query_cache;
pub mod reranker;
pub mod scoring;
pub mod spell;

use crate::{
    embedding::EmbeddingProvider,
    model::{
        build_product_text, Event, Hit, OverrideExport, Pin, Product, ProductStatus, QueryContext,
        RankingWeights, SearchMode, SearchRequest, SearchResponse, SimilarRequest,
        SponsoredSlot, Suppression,
    },
    storage::StorageEngine,
    vector::VectorIndex,
};
use chrono::{DateTime, Utc};
use anyhow::{bail, Result};
use bm25_index::Bm25Index;
use llm_rewriter::LlmRewriter;
use query_cache::QueryResultCache;
use reranker::CrossEncoderReranker;
use scoring::{
    compute_aggregations, make_cache_key, matches_filters, percentile_p95, score_candidate,
    CandidateScore,
};
use spell::SpellCorrector;
use std::collections::{HashMap, HashSet, VecDeque};
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
    /// Product embedding: used at index time and as the default query embedder.
    embedding: Arc<dyn EmbeddingProvider>,
    /// Optional separate query tower (two-tower retrieval).
    /// When set, query text is embedded with this provider instead of `embedding`.
    /// Product vectors remain embedded with `embedding`, enabling asymmetric retrieval.
    query_embedder: Option<Arc<dyn EmbeddingProvider>>,
    default_weights: RankingWeights,
    /// In-memory word corpus for autocomplete only. Populated on every index/delete
    /// call so word-prefix suggestions work regardless of storage backend.
    autocomplete_bm25: Arc<Bm25Index>,
    spell: Arc<SpellCorrector>,
    reranker: Option<Arc<CrossEncoderReranker>>,
    llm_rewriter: Option<Arc<LlmRewriter>>,
    query_cache: Option<Arc<QueryResultCache>>,
    query_count: Arc<AtomicU64>,
    latency_window: Arc<Mutex<VecDeque<u32>>>,
    field_weights: Option<HashMap<String, usize>>,
}

impl SearchEngine {
    pub(crate) fn new(
        storage: Arc<dyn StorageEngine>,
        vector_index: Arc<dyn VectorIndex>,
        embedding: Arc<dyn EmbeddingProvider>,
        default_weights: RankingWeights,
    ) -> Self {
        Self {
            storage,
            vector_index,
            embedding,
            query_embedder: None,
            default_weights,
            autocomplete_bm25: Arc::new(Bm25Index::new()),
            spell: Arc::new(SpellCorrector::new()),
            reranker: None,
            llm_rewriter: None,
            query_cache: None,
            query_count: Arc::new(AtomicU64::new(0)),
            latency_window: Arc::new(Mutex::new(VecDeque::with_capacity(LATENCY_WINDOW))),
            field_weights: None,
        }
    }

    /// Set a separate query-side embedding provider (two-tower retrieval).
    ///
    /// When set, query text is embedded with `provider`; product vectors continue
    /// to use the main `embedding` provider set at construction time. This enables
    /// asymmetric retrieval: for example, a fine-tuned query tower paired with a
    /// larger product-side model. Falls back to the product embedder when unset.
    pub fn with_query_embedder(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.query_embedder = Some(provider);
        self
    }

    pub fn with_reranker(mut self, reranker: CrossEncoderReranker) -> Self {
        self.reranker = Some(Arc::new(reranker));
        self
    }

    pub fn with_llm_rewriter(mut self, rewriter: LlmRewriter) -> Self {
        self.llm_rewriter = Some(Arc::new(rewriter));
        self
    }

    pub fn with_query_cache(mut self, ttl_secs: u64, max_entries: usize) -> Self {
        self.query_cache = Some(Arc::new(QueryResultCache::new(ttl_secs, max_entries)));
        self
    }

    pub fn with_field_weights(mut self, weights: HashMap<String, usize>) -> Self {
        self.field_weights = Some(weights);
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
            .unwrap_or_else(|| build_product_text(&product.metadata, self.field_weights.as_ref()));

        // Persist to durable storage before updating in-memory indexes.
        // This ordering means BM25/spell never have phantom entries for products
        // that failed to persist.
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

        self.storage.index_text(&product.id, &product_text, &product.metadata).await?;
        self.autocomplete_bm25.upsert(&product.id, &product_text);
        self.spell.add_text(&product_text);
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        // Delete from storage first so that if the call fails the record is not
        // a zombie: it stays in BM25/vector and a subsequent reindex_all() won't
        // resurrect a product that was already removed from the source of truth.
        self.storage.delete_product(id).await?;
        self.vector_index.delete(id).await?;
        self.storage.delete_text(id).await?;
        self.autocomplete_bm25.remove(id);
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

        let query_embedder = self.query_embedder.as_deref().unwrap_or(self.embedding.as_ref());
        let query_vector = match req.mode {
            SearchMode::Bm25 => None,
            _ => Some(query_embedder.embed(&req.q).await?),
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
        let mut spell_corrected = false;
        let mut query_expanded = false;
        let mut llm_rewritten = false;
        if matches!(req.mode, SearchMode::Hybrid | SearchMode::Bm25) {
            let bm25_results = self.storage.search_text(&req.q, candidate_k, req.filters.as_ref()).await.unwrap_or_default();

            let base_q = if bm25_results.is_empty() {
                let corrected = self.spell.correct(&req.q);
                if corrected != req.q {
                    spell_corrected = true;
                    corrected
                } else {
                    req.q.clone()
                }
            } else {
                req.q.clone()
            };

            // LLM rewriting: fires when BM25 recall is low (< half the desired limit).
            // Rewrites the query into alternative phrasings to improve retrieval.
            // Only applies when a rewriter is configured; never blocks on errors.
            let base_q = if bm25_results.len() < limit.max(1)
                && !spell_corrected
                && self.llm_rewriter.is_some()
            {
                let rewritten = self.llm_rewriter.as_ref().unwrap().rewrite(&base_q).await;
                if rewritten != base_q {
                    llm_rewritten = true;
                    rewritten
                } else {
                    base_q
                }
            } else {
                base_q
            };

            // In BM25-only mode, no semantic search has populated candidate_scores yet.
            // Pre-seed it from the sparse BM25 hits so expansion can fetch their texts.
            if candidate_scores.is_empty() {
                for (id, _) in &bm25_results {
                    candidate_scores.entry(id.clone()).or_default();
                }
            }

            let expanded_q = if bm25_results.len() < (limit / 2).max(1)
                && !candidate_scores.is_empty()
            {
                let expansion_terms = self.expand_query_terms(&base_q, &candidate_scores).await;
                if expansion_terms.is_empty() {
                    base_q.clone()
                } else {
                    query_expanded = true;
                    format!("{} {}", base_q, expansion_terms.join(" "))
                }
            } else {
                base_q.clone()
            };
            let final_bm25 = if expanded_q != req.q {
                self.storage.search_text(&expanded_q, candidate_k, req.filters.as_ref()).await.unwrap_or_default()
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

        let query_ctrs = self.storage.get_query_ctrs(&req.q).await.unwrap_or_default();

        let query_context = QueryContext {
            original_query: req.q.clone(),
            effective_query: effective_q.clone(),
            spell_corrected,
            query_expanded,
            llm_rewritten,
        };

        let mut hits: Vec<Hit> = Vec::new();
        let mut hit_vectors: Vec<Option<Vec<f32>>> = Vec::new();
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
            let ctr = query_ctrs.get(&id).copied().unwrap_or(0.0);

            let scored = score_candidate(
                &candidate, signals.popularity, availability, margin, ctr,
                &weights, req.explain, &query_context,
            );

            hit_vectors.push(product.vector.clone());
            hits.push(Hit {
                id: product.id,
                score: scored.score,
                metadata: product.metadata.clone(),
                explain: scored.explain,
            });
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        if req.rerank {
            if self.reranker.is_none() {
                bail!("rerank requested but not enabled; set index.enable_reranker = true in vectoria.toml");
            }
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

        let aggregations = req.aggregate.as_ref().map(|fields| {
            let capped: Vec<String> = fields.iter().take(MAX_AGGREGATE_FIELDS).cloned().collect();
            compute_aggregations(&hits, &capped)
        });
        let clusters = if req.cluster && hits.len() >= 2 {
            let cs = clustering::cluster_hits(&hits, &hit_vectors, 5);
            if cs.is_empty() { None } else { Some(cs) }
        } else {
            None
        };

        // Phase 2: apply admin overrides (suppressions → pins → sponsored).
        // Applied after scoring/clustering so organic ranking is computed normally;
        // overrides are deterministic and bypass the ranking formula entirely.
        hits = apply_overrides(hits, &req.q, &*self.storage).await;

        let total = hits.len();
        let page_hits: Vec<Hit> = hits.into_iter().skip(offset).take(limit).collect();

        let response = SearchResponse {
            total,
            offset,
            limit,
            processing_time_ms: start.elapsed().as_millis() as u64,
            query: req.q,
            hits: page_hits,
            aggregations,
            clusters,
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
            let embedder = self.query_embedder.as_deref().unwrap_or(self.embedding.as_ref());
            embedder.embed(&text).await?
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

    /// Run one aggregation cycle immediately (updates popularity + query-CTR signals).
    /// Normally the aggregation loop fires every `aggregation_interval_secs`; call this
    /// to apply training events without waiting.
    pub async fn trigger_aggregation(&self) -> Result<()> {
        crate::aggregation::aggregate_once_for_test(Arc::clone(&self.storage)).await
    }

    /// Return product recommendations for a user based on their click/purchase history.
    ///
    /// The user vector is loaded from cache (written by the aggregation loop) or computed
    /// on-demand by averaging the stored vectors of the user's recently interacted products.
    /// Returns an empty list for unknown users (no events recorded).
    pub async fn recommend(&self, user_id: &str, limit: usize) -> Result<Vec<Hit>> {
        const MAX_USER_ID: usize = 256;
        if user_id.is_empty() || user_id.len() > MAX_USER_ID {
            bail!("user_id must be 1–{} bytes", MAX_USER_ID);
        }

        let limit = limit.min(MAX_LIMIT);

        // Prefer the pre-computed vector from the aggregation loop.
        let user_vec = if let Some(v) = self.storage.get_user_vector(user_id).await? {
            v
        } else {
            // On-demand fallback: average vectors of recently interacted products.
            let product_ids = self.storage.get_user_recent_products(user_id, 50).await?;
            if product_ids.is_empty() {
                return Ok(vec![]);
            }

            let mut sum: Vec<f64> = Vec::new();
            let mut count = 0usize;
            for pid in &product_ids {
                if let Ok(Some(product)) = self.storage.get_product(pid).await {
                    if let Some(vector) = &product.vector {
                        if sum.is_empty() {
                            sum = vec![0.0f64; vector.len()];
                        }
                        if vector.len() == sum.len() {
                            for (s, v) in sum.iter_mut().zip(vector.iter()) {
                                *s += *v as f64;
                            }
                            count += 1;
                        }
                    }
                }
            }

            if count == 0 {
                return Ok(vec![]);
            }

            let computed: Vec<f32> = sum.iter().map(|s| (*s / count as f64) as f32).collect();
            // Cache for the next request (best-effort; ignore write errors).
            let _ = self.storage.put_user_vector(user_id, &computed).await;
            computed
        };

        let candidates = self.vector_index.search(&user_vec, limit * 5).await?;
        let mut hits = Vec::new();
        for (id, score) in candidates {
            let Some(product) = self.storage.get_product(&id).await? else { continue };
            hits.push(Hit { id: product.id, score, metadata: product.metadata, explain: None });
            if hits.len() >= limit { break; }
        }
        Ok(hits)
    }

    pub fn autocomplete(&self, prefix: &str, limit: usize) -> Vec<String> {
        self.autocomplete_bm25.suggest(prefix, limit)
    }

    /// Return related products for `product_id`.
    ///
    /// `rel_type`: optional filter — `"brand"` or `"co_purchased"`.
    /// Relations are populated by the aggregation loop.
    pub async fn related_products(
        &self,
        product_id: &str,
        rel_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::model::RelatedHit>> {
        use crate::model::{RelatedHit, RelationType};

        let limit = limit.min(MAX_LIMIT);
        let raw = self.storage.get_related(product_id, rel_type, limit).await?;
        // Normalize scores within the result set (divide by max count).
        let max_count = raw.iter().map(|(_, _, c)| *c).max().unwrap_or(1) as f32;

        let mut hits = Vec::with_capacity(raw.len());
        for (to_id, rt_str, count) in raw {
            let Some(product) = self.storage.get_product(&to_id).await? else { continue };
            let relation_type = RelationType::from_str(&rt_str)
                .unwrap_or(RelationType::CoPurchased);
            hits.push(RelatedHit {
                id: product.id,
                relation_type,
                score: (count as f32 / max_count).min(1.0),
                metadata: product.metadata,
            });
        }
        Ok(hits)
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

        let original_tokens: HashSet<String> = original_query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();

        let mut expansion = Vec::new();
        let mut seen: HashSet<String> = original_tokens.clone();
        for (id, _) in top {
            let Ok(Some(product)) = self.storage.get_product(id).await else { continue };
            let text = product.text.unwrap_or_else(|| build_product_text(&product.metadata, self.field_weights.as_ref()));
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
            bm25_document_count: storage_stats.text_document_count,
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

    fn clear_query_cache(&self) {
        if let Some(cache) = &self.query_cache {
            cache.clear();
        }
    }

    // ── Phase 2: Pins ────────────────────────────────────────────────────────

    pub async fn create_pin(&self, query: String, product_id: String, position: usize) -> Result<Pin> {
        // Evict any existing pin for the same (query, product_id) — upsert by logical key.
        let existing = self.storage.list_pins().await?;
        for old in existing.iter().filter(|p| p.query == query && p.product_id == product_id) {
            self.storage.delete_pin(&old.id).await?;
        }
        let pin = Pin::new(query, product_id, position.max(1));
        self.storage.put_pin(&pin).await?;
        self.clear_query_cache();
        Ok(pin)
    }

    pub async fn delete_pin(&self, id: &str) -> Result<()> {
        self.storage.delete_pin(id).await?;
        self.clear_query_cache();
        Ok(())
    }

    pub async fn list_pins(&self) -> Result<Vec<Pin>> {
        self.storage.list_pins().await
    }

    // ── Phase 2: Sponsored ───────────────────────────────────────────────────

    pub async fn create_sponsored(
        &self,
        query_pattern: String,
        product_id: String,
        position: usize,
        label: String,
        start_at: Option<DateTime<Utc>>,
        end_at: Option<DateTime<Utc>>,
    ) -> Result<SponsoredSlot> {
        let mut slot = SponsoredSlot::new(query_pattern, product_id, position.max(1), label);
        slot.start_at = start_at;
        slot.end_at = end_at;
        self.storage.put_sponsored(&slot).await?;
        self.clear_query_cache();
        Ok(slot)
    }

    pub async fn delete_sponsored(&self, id: &str) -> Result<()> {
        self.storage.delete_sponsored(id).await?;
        self.clear_query_cache();
        Ok(())
    }

    pub async fn list_sponsored(&self) -> Result<Vec<SponsoredSlot>> {
        self.storage.list_sponsored().await
    }

    // ── Phase 2: Suppressions ────────────────────────────────────────────────

    pub async fn create_suppression(&self, query: String, product_id: String) -> Result<Suppression> {
        let sup = Suppression::new(query, product_id);
        self.storage.put_suppression(&sup).await?;
        self.clear_query_cache();
        Ok(sup)
    }

    pub async fn delete_suppression(&self, id: &str) -> Result<()> {
        self.storage.delete_suppression(id).await?;
        self.clear_query_cache();
        Ok(())
    }

    pub async fn list_suppressions(&self) -> Result<Vec<Suppression>> {
        self.storage.list_suppressions().await
    }

    /// Returns the subset of overrides that are currently active for `query`.
    /// Uses the same matching logic as `apply_overrides` so the UI can show
    /// toggle buttons without re-implementing any algorithm client-side.
    pub async fn active_overrides_for_query(
        &self,
        query: &str,
    ) -> Result<(Vec<Pin>, Vec<SponsoredSlot>, Vec<Suppression>)> {
        let now = Utc::now();
        let pins = self.storage.list_pins().await?
            .into_iter()
            .filter(|p| p.query == query)
            .collect();
        let sponsored = self.storage.list_sponsored().await?
            .into_iter()
            .filter(|s| {
                query_matches_pattern(query, &s.query_pattern)
                    && s.start_at.map_or(true, |t| t <= now)
                    && s.end_at.map_or(true, |t| t > now)
            })
            .collect();
        let suppressions = self.storage.list_suppressions().await?
            .into_iter()
            .filter(|s| s.query == query)
            .collect();
        Ok((pins, sponsored, suppressions))
    }

    // ── Phase 2: Export / Import ─────────────────────────────────────────────

    pub async fn export_overrides(&self) -> Result<OverrideExport> {
        Ok(OverrideExport {
            pins: self.storage.list_pins().await?,
            sponsored: self.storage.list_sponsored().await?,
            suppressions: self.storage.list_suppressions().await?,
            exported_at: Utc::now(),
        })
    }

    pub async fn import_overrides(&self, data: OverrideExport) -> Result<ImportReport> {
        let mut imported = 0usize;

        if !data.pins.is_empty() {
            // Read existing pins once; evict any that conflict with incoming (query, product_id).
            let existing = self.storage.list_pins().await?;
            let incoming_keys: HashSet<(&str, &str)> = data.pins.iter()
                .map(|p| (p.query.as_str(), p.product_id.as_str()))
                .collect();
            for old in existing.iter().filter(|p| incoming_keys.contains(&(p.query.as_str(), p.product_id.as_str()))) {
                self.storage.delete_pin(&old.id).await?;
            }
            for pin in &data.pins {
                if pin.query.is_empty() || pin.product_id.is_empty() { continue; }
                let p = Pin::new(pin.query.clone(), pin.product_id.clone(), pin.position.max(1));
                self.storage.put_pin(&p).await?;
                imported += 1;
            }
        }

        for slot in data.sponsored {
            if slot.query_pattern.is_empty() || slot.product_id.is_empty() { continue; }
            self.storage.put_sponsored(&slot).await?;
            imported += 1;
        }
        for sup in data.suppressions {
            if sup.query.is_empty() || sup.product_id.is_empty() { continue; }
            self.storage.put_suppression(&sup).await?;
            imported += 1;
        }
        self.clear_query_cache();
        Ok(ImportReport { imported })
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

#[derive(serde::Serialize)]
pub struct ReindexReport {
    pub reindexed: usize,
    pub errors: usize,
}

#[derive(serde::Serialize)]
pub struct ImportReport {
    pub imported: usize,
}

/// Apply Phase 2 admin overrides to a ranked hit list.
/// Order: suppressions → pins → sponsored.
async fn apply_overrides(
    mut hits: Vec<Hit>,
    query: &str,
    storage: &dyn StorageEngine,
) -> Vec<Hit> {
    let now = Utc::now();

    // Suppressions: remove hidden products for this query.
    if let Ok(suppressions) = storage.list_suppressions().await {
        let suppressed: HashSet<String> = suppressions
            .into_iter()
            .filter(|s| s.query == query)
            .map(|s| s.product_id)
            .collect();
        if !suppressed.is_empty() {
            hits.retain(|h| !suppressed.contains(&h.id));
        }
    }

    // Pins: force a product to a specific 1-indexed position.
    if let Ok(pins) = storage.list_pins().await {
        let mut query_pins: Vec<Pin> = pins.into_iter().filter(|p| p.query == query).collect();
        // Process lowest position first: each insertion shifts later elements right,
        // so applying lower positions before higher ones keeps all target positions correct.
        query_pins.sort_by_key(|p| p.position);
        for pin in query_pins {
            if let Some(idx) = hits.iter().position(|h| h.id == pin.product_id) {
                let hit = hits.remove(idx);
                let target = pin.position.saturating_sub(1).min(hits.len());
                hits.insert(target, hit);
            }
        }
    }

    // Sponsored: inject advertiser products at fixed positions.
    if let Ok(sponsored) = storage.list_sponsored().await {
        let mut active: Vec<SponsoredSlot> = sponsored
            .into_iter()
            .filter(|s| {
                query_matches_pattern(query, &s.query_pattern)
                    && s.start_at.map_or(true, |t| t <= now)
                    && s.end_at.map_or(true, |t| t > now)
            })
            .collect();
        // Process lowest position first so each insertion lands at the correct final index.
        active.sort_by_key(|s| s.position);
        for slot in active {
            if let Ok(Some(product)) = storage.get_product(&slot.product_id).await {
                hits.retain(|h| h.id != slot.product_id);
                let target = slot.position.saturating_sub(1).min(hits.len());
                let mut meta = product.metadata.clone();
                meta["sponsored"] = serde_json::json!(true);
                if !slot.label.is_empty() {
                    meta["sponsored_label"] = serde_json::json!(slot.label);
                }
                hits.insert(target, Hit { id: product.id, score: 0.0, metadata: meta, explain: None });
            }
        }
    }

    hits
}

/// Match a query against a pattern: exact match or query starts with pattern (prefix).
fn query_matches_pattern(query: &str, pattern: &str) -> bool {
    query == pattern || query.starts_with(pattern)
}
