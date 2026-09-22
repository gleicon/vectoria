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
pub(super) fn gzip_len(text: &str) -> usize {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(text.as_bytes());
    enc.finish().map(|b| b.len()).unwrap_or(text.len())
}

/// NCD similarity using a pre-computed query gzip length.
/// Callers in a scoring loop should compute `gzip_len(query)` once and pass it here.
pub(super) fn ncd_similarity(query_gz: f32, query: &str, doc_text: &str) -> f32 {
    if query.is_empty() || doc_text.is_empty() { return 0.0; }
    let cb = gzip_len(doc_text) as f32;
    let combined = format!("{} {}", query, doc_text);
    let cab = gzip_len(&combined) as f32;
    let ncd = (cab - query_gz.min(cb)) / query_gz.max(cb);
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
    // RRF: rank position is scale-agnostic; BM25 and cosine scores are not comparable.
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
    // NCD: gzip-based sparse signal; catches structural patterns embeddings and BM25 miss.
    let ncd_contribution = candidate.ncd * weights.bm25 * 0.5;
    let rrf_score = rrf_semantic * weights.semantic + rrf_bm25 * weights.bm25 + ncd_contribution;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ncd_similarity_identical_strings() {
        let q = "tênis nike running";
        let qgz = gzip_len(q) as f32;
        let score = ncd_similarity(qgz, q, q);
        // Identical strings compress maximally together — score should be high.
        assert!(score > 0.5, "identical strings should score > 0.5, got {score}");
    }

    #[test]
    fn test_ncd_similarity_unrelated_strings() {
        let q = "tênis nike running";
        let doc = "refrigerador inox 400 litros frost free";
        let qgz = gzip_len(q) as f32;
        let score = ncd_similarity(qgz, q, doc);
        // Unrelated strings should compress poorly together.
        assert!(score < 0.5, "unrelated strings should score < 0.5, got {score}");
    }

    #[test]
    fn test_ncd_similarity_empty_inputs_return_zero() {
        let qgz = gzip_len("query") as f32;
        assert_eq!(ncd_similarity(qgz, "query", ""), 0.0);
        assert_eq!(ncd_similarity(0.0, "", "doc"), 0.0);
    }

    #[test]
    fn test_ncd_precomputed_matches_inline() {
        let q = "cadeira escritório ergonômica";
        let doc = "cadeira de escritório ergonômica couro";
        let qgz = gzip_len(q) as f32;
        let s1 = ncd_similarity(qgz, q, doc);
        // Calling twice with same inputs must be deterministic.
        let s2 = ncd_similarity(qgz, q, doc);
        assert_eq!(s1, s2);
        assert!(s1 > 0.0, "similar strings should have positive score");
    }
}

pub(super) fn percentile_p95(window: &VecDeque<u32>) -> u32 {
    if window.is_empty() { return 0; }
    let mut sorted: Vec<u32> = window.iter().copied().collect();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 * 0.95) as usize).saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}
