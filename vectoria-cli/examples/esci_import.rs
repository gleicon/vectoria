// Amazon ESCI dataset importer for Vectoria.
//
// IMPORTANT: The Amazon ESCI dataset requires a separate license agreement.
// Download and terms of use: https://github.com/amazon-science/esci-data
// Do NOT redistribute the dataset files.
//
// Usage:
//   cargo run --example esci_import -- \
//     shopping_queries_dataset_products.parquet \
//     shopping_queries_dataset_examples.parquet \
//     --import --judges judges.ndjson
use anyhow::{Context, Result};
use arrow::array::Array;
use clap::Parser;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "esci_import", about = "Load Amazon ESCI dataset into Vectoria (requires separate ESCI license)")]
struct Args {
    pub products_file: PathBuf,
    pub examples_file: PathBuf,
    #[arg(long)]
    pub import: bool,
    #[arg(long)]
    pub judges: Option<PathBuf>,
    #[arg(long)]
    pub locale: Option<String>,
    #[arg(long, default_value = "test")]
    pub split: String,
    #[arg(long, default_value = "0")]
    pub max_products: usize,
    /// ESCI relevance labels to include in judges file (comma-separated: E,S,C)
    #[arg(long, default_value = "E,S")]
    pub labels: String,
    #[arg(long, default_value = "64")]
    pub batch_size: usize,
    #[arg(long, default_value = "http://localhost:7700")]
    pub server: String,
    #[arg(long, default_value = "")]
    pub api_key: String,
    /// Named index to import into. When set, posts to /indexes/{name}/products instead of /products.
    #[arg(long)]
    pub index: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.import && args.judges.is_none() {
        anyhow::bail!("Specify --import, --judges <path>, or both.");
    }

    println!("Loading ESCI products from {:?}...", args.products_file);
    let products = load_products(&args.products_file, args.locale.as_deref(), args.max_products)
        .context("failed to load ESCI products")?;
    println!("  {} products loaded", products.len());

    if args.import {
        let target = match &args.index {
            Some(name) => format!("{}/indexes/{}/products", args.server, name),
            None       => format!("{}/products", args.server),
        };
        println!("Importing products into {}...", target);
        import_products(&products, &target, &args.api_key, args.batch_size).await?;
        println!("  Import complete.");
    }

    if let Some(ref out_path) = args.judges {
        let label_set: Vec<&str> = args.labels.split(',').map(|s| s.trim()).collect();
        println!("Loading ESCI examples from {:?}...", args.examples_file);
        let judged = build_judged_queries(&args.examples_file, &products, &args.split, &label_set)
            .context("failed to build judged queries")?;
        println!("  {} judged queries ({} labels, split={})", judged.len(), args.labels, args.split);
        let file = File::create(out_path)
            .with_context(|| format!("failed to create {:?}", out_path))?;
        let mut writer = BufWriter::new(file);
        for jq in &judged {
            writeln!(writer, "{}", serde_json::to_string(jq)?)?;
        }
        println!("  Judged dataset written to {:?}", out_path);
    }

    Ok(())
}

#[derive(Serialize)]
struct JudgedQuery {
    query: String,
    relevant_ids: Vec<String>,
    k: usize,
}

struct EsciProduct {
    id: String,
    title: String,
    brand: String,
    color: String,
    description: String,
    locale: String,
}

fn get_str_col<'a>(batch: &'a arrow::record_batch::RecordBatch, name: &str) -> Option<&'a arrow::array::StringArray> {
    batch.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>())
}

fn load_products(path: &PathBuf, locale_filter: Option<&str>, max: usize) -> Result<HashMap<String, EsciProduct>> {
    let file = File::open(path).with_context(|| format!("cannot open {:?}", path))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut products: HashMap<String, EsciProduct> = HashMap::new();

    for batch in reader {
        let batch = batch.context("failed to read record batch")?;
        let ids = get_str_col(&batch, "product_id");
        let titles = get_str_col(&batch, "product_title");
        let brands = get_str_col(&batch, "product_brand");
        let colors = get_str_col(&batch, "product_color");
        let descs = get_str_col(&batch, "product_description");
        let locales = get_str_col(&batch, "product_locale");
        let (Some(ids), Some(titles)) = (ids, titles) else { continue };

        for i in 0..batch.num_rows() {
            if max > 0 && products.len() >= max { break; }
            let locale = locales.map(|c| c.value(i)).unwrap_or("us");
            if let Some(lf) = locale_filter { if locale != lf { continue; } }
            let id = ids.value(i).to_string();
            if id.is_empty() { continue; }
            products.insert(id.clone(), EsciProduct {
                id,
                title: titles.value(i).to_string(),
                brand: brands.map(|c| c.value(i)).unwrap_or("").to_string(),
                color: colors.map(|c| c.value(i)).unwrap_or("").to_string(),
                description: descs.map(|c| c.value(i)).unwrap_or("").to_string(),
                locale: locale.to_string(),
            });
        }
    }
    Ok(products)
}

async fn import_products(products: &HashMap<String, EsciProduct>, url: &str, api_key: &str, batch_size: usize) -> Result<()> {
    let client = reqwest::Client::new();
    let mut count = 0usize;
    let mut errors = 0usize;
    let mut batch: Vec<serde_json::Value> = Vec::with_capacity(batch_size);

    for product in products.values() {
        let mut text_parts = vec![product.title.clone()];
        if !product.brand.is_empty() { text_parts.push(product.brand.clone()); }
        if !product.color.is_empty() { text_parts.push(product.color.clone()); }
        batch.push(serde_json::json!({
            "id": product.id,
            "text": text_parts.join(". "),
            "metadata": {
                "title": product.title, "brand": product.brand, "color": product.color,
                "description": product.description.chars().take(500).collect::<String>(),
                "locale": product.locale,
            }
        }));
        if batch.len() >= batch_size {
            flush_batch(&client, &url, api_key, &mut batch, &mut count, &mut errors).await;
        }
    }
    if !batch.is_empty() {
        flush_batch(&client, &url, api_key, &mut batch, &mut count, &mut errors).await;
    }
    println!("  Imported {} products ({} errors)", count, errors);
    Ok(())
}

async fn flush_batch(client: &reqwest::Client, url: &str, api_key: &str, batch: &mut Vec<serde_json::Value>, count: &mut usize, errors: &mut usize) {
    for body in batch.drain(..) {
        match client.post(url).bearer_auth(api_key).json(&body).send().await {
            Ok(r) if r.status().is_success() => *count += 1,
            Ok(r) => {
                let status = r.status();
                // Abort immediately on auth/not-found errors — retrying won't help.
                if status == reqwest::StatusCode::UNAUTHORIZED {
                    eprintln!("error: 401 Unauthorized — check your API key (--api-key)");
                    std::process::exit(1);
                }
                if status == reqwest::StatusCode::NOT_FOUND {
                    eprintln!("error: 404 Not Found — does the index exist? (--index <name>)");
                    std::process::exit(1);
                }
                eprintln!("warning: HTTP {} for one product", status);
                *errors += 1;
            }
            Err(e) => {
                eprintln!("error: request failed: {}", e);
                *errors += 1;
            }
        }
    }
}

fn build_judged_queries(path: &PathBuf, products: &HashMap<String, EsciProduct>, split: &str, labels: &[&str]) -> Result<Vec<JudgedQuery>> {
    let file = File::open(path).with_context(|| format!("cannot open {:?}", path))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut relevant: HashMap<String, Vec<String>> = HashMap::new();

    for batch in reader {
        let batch = batch.context("failed to read record batch")?;
        let (Some(queries), Some(product_ids), Some(esci_labels)) = (
            get_str_col(&batch, "query"),
            get_str_col(&batch, "product_id"),
            get_str_col(&batch, "esci_label"),
        ) else { continue };
        let splits = get_str_col(&batch, "split");

        for i in 0..batch.num_rows() {
            if let Some(s) = splits { if s.value(i) != split { continue; } }
            if !labels.contains(&esci_labels.value(i)) { continue; }
            let product_id = product_ids.value(i).to_string();
            if !products.contains_key(&product_id) { continue; }
            relevant.entry(queries.value(i).to_string()).or_default().push(product_id);
        }
    }

    Ok(relevant.into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(query, relevant_ids)| JudgedQuery { query, k: 10, relevant_ids })
        .collect())
}
