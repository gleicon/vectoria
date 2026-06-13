use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Args)]
pub struct EvalArgs {
    /// Judged dataset (NDJSON: {"query": "...", "relevant_ids": [...], "k": 10})
    pub dataset: PathBuf,
    /// Number of results to retrieve per query
    #[arg(long, default_value = "10")]
    pub k: usize,
}

/// One entry in the judged dataset.
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

pub async fn run(args: EvalArgs, server: &str, api_key: Option<String>) -> Result<()> {
    let file = std::fs::File::open(&args.dataset).context("failed to open dataset")?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;

    let client = reqwest::Client::new();
    let api_key = api_key.unwrap_or_default();
    let search_url = format!("{}/search", server);

    let mut total_recall = 0.0f64;
    let mut total_ndcg = 0.0f64;
    let mut total_mrr = 0.0f64;
    let mut queries_with_results = 0usize;
    let mut total_queries = 0usize;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let judged: JudgedQuery = serde_json::from_str(&line)
            .context("invalid judged query line")?;
        let k = judged.k.unwrap_or(args.k);
        total_queries += 1;

        let resp = client
            .post(&search_url)
            .bearer_auth(&api_key)
            .json(&serde_json::json!({ "q": judged.query, "limit": k }))
            .send()
            .await
            .context("search request failed")?;

        if !resp.status().is_success() {
            eprintln!("search failed for query '{}': {}", judged.query, resp.text().await?);
            continue;
        }

        let search_resp: SearchResponse = resp.json().await?;
        let result_ids: Vec<&str> = search_resp.hits.iter().map(|h| h.id.as_str()).collect();
        let relevant: HashSet<&str> = judged.relevant_ids.iter().map(|s| s.as_str()).collect();

        if !result_ids.is_empty() { queries_with_results += 1; }

        // Recall@K
        let hits_in_relevant = result_ids.iter().filter(|id| relevant.contains(**id)).count();
        let recall = hits_in_relevant as f64 / relevant.len().max(1) as f64;
        total_recall += recall;

        // MRR
        let mrr = result_ids.iter().enumerate()
            .find(|(_, id)| relevant.contains(**id))
            .map(|(i, _)| 1.0 / (i + 1) as f64)
            .unwrap_or(0.0);
        total_mrr += mrr;

        // NDCG@K
        let dcg: f64 = result_ids.iter().enumerate()
            .filter(|(_, id)| relevant.contains(**id))
            .map(|(i, _)| 1.0 / (i as f64 + 2.0).log2())
            .sum();
        let ideal_dcg: f64 = (0..relevant.len().min(k))
            .map(|i| 1.0 / (i as f64 + 2.0).log2())
            .sum();
        let ndcg = if ideal_dcg > 0.0 { dcg / ideal_dcg } else { 0.0 };
        total_ndcg += ndcg;
    }

    if total_queries == 0 {
        println!("no queries found in dataset");
        return Ok(());
    }

    let n = total_queries as f64;
    println!("=== Vectoria Eval Results ===");
    println!("Queries:          {}", total_queries);
    println!("Coverage:         {:.1}%", (queries_with_results as f64 / n) * 100.0);
    println!("Recall@{}:        {:.4}", args.k, total_recall / n);
    println!("NDCG@{}:          {:.4}", args.k, total_ndcg / n);
    println!("MRR:              {:.4}", total_mrr / n);

    Ok(())
}
