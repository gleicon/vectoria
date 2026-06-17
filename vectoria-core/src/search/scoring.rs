use std::collections::{HashMap, VecDeque};
use crate::model::{
    Hit, QueryContext, RankingWeights, ScoreBreakdown, ScoreFactor, SearchRequest,
};

#[derive(Default)]
pub(super) struct CandidateScore {
    pub semantic: f32,
    pub bm25: f32,
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
    let score = candidate.semantic * weights.semantic
        + candidate.bm25 * weights.bm25
        + popularity * weights.popularity
        + availability * weights.availability
        + margin * weights.margin
        + ctr * weights.query_ctr;

    let breakdown = explain.then(|| {
        let mut sources = Vec::new();
        if candidate.bm25 > 0.0 { sources.push("bm25".to_string()); }
        if candidate.semantic > 0.0 { sources.push("vector".to_string()); }
        ScoreBreakdown {
            factors: vec![
                ScoreFactor { factor: "semantic_similarity".into(), score: candidate.semantic, weight: weights.semantic, contribution: candidate.semantic * weights.semantic },
                ScoreFactor { factor: "bm25".into(), score: candidate.bm25, weight: weights.bm25, contribution: candidate.bm25 * weights.bm25 },
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
