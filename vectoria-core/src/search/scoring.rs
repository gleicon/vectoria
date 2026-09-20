use std::collections::{HashMap, VecDeque};
use std::io::Write;
use flate2::{write::GzEncoder, Compression};
use crate::model::{
    Hit, QueryContext, RankingWeights, ScoreBreakdown, ScoreFactor, SearchRequest,
};

/// Standard RRF constant (Cormack et al. 2009). Controls rank-sensitivity:
/// lower k → top ranks contribute much more than lower ranks.
const RRF_K: f32 = 60.0;

/// Compute the gzip-compressed byte length of `text`.
fn gzip_len(text: &str) -> usize {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(text.as_bytes());
    enc.finish().map(|b| b.len()).unwrap_or(text.len())
}

/// Normalized Compression Distance (NCD) between query and document text.
///
/// NCD(a, b) = (C(a+b) - min(C(a), C(b))) / max(C(a), C(b))
///
/// Returns a value in [0, 1] where 0 = identical and 1 = maximally dissimilar.
/// Converted to a similarity score (1 - NCD) before use.
pub(super) fn ncd_similarity(query: &str, doc_text: &str) -> f32 {
    if query.is_empty() || doc_text.is_empty() { return 0.0; }
    let ca = gzip_len(query) as f32;
    let cb = gzip_len(doc_text) as f32;
    let combined = format!("{} {}", query, doc_text);
    let cab = gzip_len(&combined) as f32;
    let ncd = (cab - ca.min(cb)) / ca.max(cb);
    // Clamp to [0,1] — gzip overhead on very short strings can push NCD slightly above 1.
    1.0 - ncd.clamp(0.0, 1.0)
}

#[derive(Default)]
pub(super) struct CandidateScore {
    pub semantic: f32,
    pub bm25: f32,
    /// 1-based rank in the semantic retrieval list. 0 = not retrieved by this signal.
    pub semantic_rank: usize,
    /// 1-based rank in the BM25 retrieval list. 0 = not retrieved by this signal.
    pub bm25_rank: usize,
    /// NCD similarity between query and document text (0–1). Set after product text is fetched.
    pub ncd: f32,
}

pub(super) struct ScoredCandidate {
    pub score: f32,
    pub explain: Option<ScoreBreakdown>,
}

pub(super) fn score_candidate(
    candidate: &CandidateScore,
    popularity: f32,
    availability: f32,
    margin: f32,
    ctr: f32,
    weights: &RankingWeights,
    explain: bool,
    query_context: &QueryContext,
) -> ScoredCandidate {
    // Retrieval signals fused via RRF: 1/(k + rank) for each signal the document
    // was retrieved by. Documents not retrieved by a signal contribute 0 from that signal.
    // RRF is scale-agnostic — BM25 and cosine scores live on incompatible scales;
    // rank position is comparable across any retrieval method.
    let rrf_semantic = if candidate.semantic_rank > 0 {
        1.0 / (RRF_K + candidate.semantic_rank as f32)
    } else {
        0.0
    };
    let rrf_bm25 = if candidate.bm25_rank > 0 {
        1.0 / (RRF_K + candidate.bm25_rank as f32)
    } else {
        0.0
    };
    // RRF score: weighted combination of per-signal rank contributions.
    // weights.semantic and weights.bm25 tune relative emphasis between signals
    // (default 0.7/0.3) without needing score normalization.
    // NCD (gzip-based compression similarity) is a language-agnostic third sparse signal
    // that adds to the BM25 channel — it captures repetitive/structural patterns that
    // both BM25 and embeddings can miss. Weighted at half the BM25 weight to avoid
    // over-indexing on very short queries.
    let ncd_contribution = candidate.ncd * weights.bm25 * 0.5;
    let rrf_score = rrf_semantic * weights.semantic + rrf_bm25 * weights.bm25 + ncd_contribution;

    // Behavioral signals add on top of the RRF base. They are product-level signals
    // (not query-dependent) and don't participate in RRF — they stay as weighted additive bonuses.
    let score = rrf_score
        + popularity * weights.popularity
        + availability * weights.availability
        + margin * weights.margin
        + ctr * weights.query_ctr;

    let breakdown = explain.then(|| {
        let mut sources = Vec::new();
        if candidate.bm25_rank > 0 { sources.push("bm25".to_string()); }
        if candidate.semantic_rank > 0 { sources.push("vector".to_string()); }
        ScoreBreakdown {
            factors: vec![
                ScoreFactor { factor: "semantic_similarity".into(), score: rrf_semantic, weight: weights.semantic, contribution: rrf_semantic * weights.semantic },
                ScoreFactor { factor: "bm25".into(), score: rrf_bm25, weight: weights.bm25, contribution: rrf_bm25 * weights.bm25 },
                ScoreFactor { factor: "ncd".into(), score: candidate.ncd, weight: weights.bm25 * 0.5, contribution: ncd_contribution },
                ScoreFactor { factor: "popularity".into(), score: popularity, weight: weights.popularity, contribution: popularity * weights.popularity },
                ScoreFactor { factor: "query_ctr".into(), score: ctr, weight: weights.query_ctr, contribution: ctr * weights.query_ctr },
                ScoreFactor { factor: "availability".into(), score: availability, weight: weights.availability, contribution: availability * weights.availability },
                ScoreFactor { factor: "margin".into(), score: margin, weight: weights.margin, contribution: margin * weights.margin },
            ],
            match_sources: sources,
            query_context: query_context.clone(),
        }
    });

    ScoredCandidate { score, explain: breakdown }
}

pub(super) fn matches_filters(
    metadata: &serde_json::Value,
    filters: &HashMap<String, serde_json::Value>,
) -> bool {
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

pub(super) fn make_cache_key(req: &SearchRequest) -> String {
    let filters = req.filters.as_ref().map(|f| {
        let mut pairs: Vec<_> = f.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        serde_json::to_string(&pairs).unwrap_or_default()
    }).unwrap_or_default();
    format!("{}|{:?}|{}|{}|{}", req.q, req.mode, req.limit, req.offset, filters)
}

pub(super) fn compute_aggregations(
    hits: &[Hit],
    fields: &[String],
) -> HashMap<String, HashMap<String, usize>> {
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

pub(super) fn percentile_p95(window: &VecDeque<u32>) -> u32 {
    if window.is_empty() { return 0; }
    let mut sorted: Vec<u32> = window.iter().copied().collect();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 * 0.95) as usize).saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}
