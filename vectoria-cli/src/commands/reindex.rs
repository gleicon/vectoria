use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct ReindexArgs {
    /// New embedding model to reindex with (informational — model is configured server-side)
    #[arg(long)]
    pub model: Option<String>,
    /// Wait for reindex to complete by polling /stats (default: fire-and-forget)
    #[arg(long)]
    pub wait: bool,
}

pub async fn run(args: ReindexArgs, server: &str, api_key: Option<String>) -> Result<()> {
    let client = reqwest::Client::new();
    let api_key = api_key.unwrap_or_default();
    let url = format!("{}/admin/reindex", server);

    if let Some(model) = &args.model {
        println!("requesting reindex with model hint '{}'", model);
        println!("note: configure [embedding] model in vectoria.toml before triggering reindex");
    }

    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .send()
        .await
        .context("failed to reach server")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("reindex request failed: {}", body);
    }

    println!("reindex started (background job)");

    if args.wait {
        println!("polling /stats until reindex completes...");
        let stats_url = format!("{}/stats", server);
        let mut stable_rounds = 0usize;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            let stats: serde_json::Value = client
                .get(&stats_url)
                .bearer_auth(&api_key)
                .send()
                .await
                .context("stats request failed")?
                .json()
                .await?;
            let count = stats["vector_count"].as_u64().unwrap_or(0);
            let products = stats["product_count"].as_u64().unwrap_or(0);
            print!("\r  vectors: {}/{} ...", count, products);
            if count == products && count > 0 {
                stable_rounds += 1;
                if stable_rounds >= 3 {
                    println!("\nreindex complete: {} products indexed", count);
                    break;
                }
            } else {
                stable_rounds = 0;
            }
        }
    }

    Ok(())
}
