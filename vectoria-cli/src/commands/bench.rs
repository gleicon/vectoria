/// Benchmark command: load Amazon ESCI or custom judged dataset, run queries
/// against the server in each mode (bm25, semantic, hybrid), and report
/// Recall@K, NDCG@K, MRR, Coverage, and latency percentiles.
use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Args)]
pub struct BenchArgs {
    /// Judged dataset (NDJSON: {"query": "...", "relevant_ids": [...], "k": 10})
    pub dataset: PathBuf,
    /// Results per query
    #[arg(long, default_value = "10")]
    pub k: usize,
    /// Search mode to benchmark ("bm25" | "semantic" | "hybrid" | "all")
    #[arg(long, default_value = "all")]
    pub mode: String,
    /// Number of warmup queries (results discarded)
    #[arg(long, default_value = "5")]
    pub warmup: usize,
}

#[derive(Deserialize)]
struct JudgedQuery {
    query: String,
    relevant_ids: Vec<String>,
    #[serde(default)]
    k: Option<usize>,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    id: String,
    #[allow(dead_code)]
    score: f32,
}

pub async fn run(args: BenchArgs, server: &str, api_key: Option<String>) -> Result<()> {
    use std::io::BufRead;

    let file = std::fs::File::open(&args.dataset).context("failed to open dataset")?;
    let queries: Vec<JudgedQuery> = std::io::BufReader::new(file)
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();

    if queries.is_empty() {
        anyhow::bail!("no queries in dataset");
    }

    let modes: Vec<&str> = match args.mode.as_str() {
        "all" => vec!["bm25", "semantic", "hybrid"],
        m => vec![m],
    };

    let client = reqwest::Client::new();
    let api_key = api_key.unwrap_or_default();
    let search_url = format!("{}/search", server);

    println!("=== Vectoria Benchmark ===");
    println!("Dataset:   {} queries", queries.len());
    println!("K:         {}", args.k);
    println!();

    for mode in modes {
        let mut latencies_ms: Vec<f64> = Vec::new();
        let mut total_recall = 0.0f64;
        let mut total_ndcg = 0.0f64;
        let mut total_mrr = 0.0f64;
        let mut queries_with_results = 0usize;
        let mut errors = 0usize;

        for (i, judged) in queries.iter().enumerate() {
            let k = judged.k.unwrap_or(args.k);
            let start = Instant::now();

            let resp = match client
                .post(&search_url)
                .bearer_auth(&api_key)
                .json(&serde_json::json!({ "q": judged.query, "limit": k, "mode": mode }))
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => { errors += 1; continue; }
            };

            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            if !resp.status().is_success() {
                errors += 1;
                continue;
            }

            let search_resp: SearchResponse = match resp.json().await {
                Ok(r) => r,
                Err(_) => { errors += 1; continue; }
            };

            // Skip warmup queries for latency measurement.
            if i >= args.warmup {
                latencies_ms.push(elapsed);
            }

            let result_ids: Vec<&str> = search_resp.hits.iter().map(|h| h.id.as_str()).collect();
            let relevant: HashSet<&str> = judged.relevant_ids.iter().map(|s| s.as_str()).collect();

            if !result_ids.is_empty() { queries_with_results += 1; }

            let hits_in_relevant = result_ids.iter().filter(|id| relevant.contains(**id)).count();
            total_recall += hits_in_relevant as f64 / relevant.len().max(1) as f64;

            total_mrr += result_ids.iter().enumerate()
                .find(|(_, id)| relevant.contains(**id))
                .map(|(i, _)| 1.0 / (i + 1) as f64)
                .unwrap_or(0.0);

            let dcg: f64 = result_ids.iter().enumerate()
                .filter(|(_, id)| relevant.contains(**id))
                .map(|(i, _)| 1.0 / (i as f64 + 2.0).log2())
                .sum();
            let ideal_dcg: f64 = (0..relevant.len().min(k))
                .map(|i| 1.0 / (i as f64 + 2.0).log2())
                .sum();
            total_ndcg += if ideal_dcg > 0.0 { dcg / ideal_dcg } else { 0.0 };
        }

        let n = queries.len() as f64;
        latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile(&latencies_ms, 50.0);
        let p95 = percentile(&latencies_ms, 95.0);
        let p99 = percentile(&latencies_ms, 99.0);

        println!("── Mode: {} ──────────────────────────────", mode);
        println!("  Queries:      {}", queries.len());
        println!("  Errors:       {}", errors);
        println!("  Coverage:     {:.1}%", (queries_with_results as f64 / n) * 100.0);
        println!("  Recall@{}:    {:.4}", args.k, total_recall / n);
        println!("  NDCG@{}:      {:.4}", args.k, total_ndcg / n);
        println!("  MRR:          {:.4}", total_mrr / n);
        println!("  Latency p50:  {:.1}ms", p50);
        println!("  Latency p95:  {:.1}ms", p95);
        println!("  Latency p99:  {:.1}ms", p99);
        println!();
    }

    Ok(())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
