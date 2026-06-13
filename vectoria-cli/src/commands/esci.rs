/// Amazon ESCI (E-commerce Search Challenge) dataset loader.
///
/// Reads the two official ESCI Parquet files and either:
///   1. Imports products into a running Vectoria server (`--import`)
///   2. Writes a judged NDJSON file for use with `vectoria bench` (`--judges`)
///
/// ESCI dataset structure:
///   shopping_queries_dataset_products.parquet
///     columns: product_id, product_title, product_description,
///              product_bullet_point, product_brand, product_color, product_locale
///   shopping_queries_dataset_examples.parquet
///     columns: query_id, query, product_id, esci_label, small_version, split
///
/// ESCI labels: E=Exact, S=Substitute, C=Complement, I=Irrelevant
/// Only Exact (E) judgments are treated as relevant in the judged output.
use anyhow::{Context, Result};
use arrow::array::{Array, StringArray};
use clap::Args;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Args)]
pub struct EsciArgs {
    /// Path to shopping_queries_dataset_products.parquet
    pub products_file: PathBuf,

    /// Path to shopping_queries_dataset_examples.parquet
    pub examples_file: PathBuf,

    /// Import products into the Vectoria server
    #[arg(long)]
    pub import: bool,

    /// Write judged dataset (for `vectoria bench`) to this path
    #[arg(long)]
    pub judges: Option<PathBuf>,

    /// Only process this locale (e.g. "us", "es", "jp")
    #[arg(long)]
    pub locale: Option<String>,

    /// Only use examples from this split ("train" | "test")
    #[arg(long, default_value = "test")]
    pub split: String,

    /// Max products to import (0 = all)
    #[arg(long, default_value = "0")]
    pub max_products: usize,

    /// Batch size for server import
    #[arg(long, default_value = "64")]
    pub batch_size: usize,
}

#[derive(Serialize)]
struct JudgedQuery {
    query: String,
    relevant_ids: Vec<String>,
    k: usize,
}

pub async fn run(args: EsciArgs, server: &str, api_key: Option<String>) -> Result<()> {
    let api_key = api_key.unwrap_or_default();

    println!("Loading ESCI products from {:?}...", args.products_file);
    let products = load_products(&args.products_file, args.locale.as_deref(), args.max_products)
        .context("failed to load ESCI products")?;
    println!("  {} products loaded", products.len());

    if args.import {
        println!("Importing products into {}...", server);
        import_products(&products, server, &api_key, args.batch_size).await?;
        println!("  Import complete.");
    }

    if let Some(ref out_path) = args.judges {
        println!("Loading ESCI examples from {:?}...", args.examples_file);
        let judged = build_judged_queries(&args.examples_file, &products, &args.split)
            .context("failed to build judged queries")?;
        println!("  {} judged queries (Exact labels, split={})", judged.len(), args.split);

        let file = File::create(out_path)
            .with_context(|| format!("failed to create {:?}", out_path))?;
        let mut writer = BufWriter::new(file);
        for jq in &judged {
            let line = serde_json::to_string(jq)?;
            writeln!(writer, "{}", line)?;
        }
        println!("  Judged dataset written to {:?}", out_path);
    }

    if !args.import && args.judges.is_none() {
        anyhow::bail!("Specify --import, --judges <path>, or both.");
    }

    Ok(())
}

struct EsciProduct {
    id: String,
    title: String,
    brand: String,
    color: String,
    description: String,
    locale: String,
}

fn load_products(
    path: &PathBuf,
    locale_filter: Option<&str>,
    max: usize,
) -> Result<HashMap<String, EsciProduct>> {
    let file = File::open(path).with_context(|| format!("cannot open {:?}", path))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    let mut products: HashMap<String, EsciProduct> = HashMap::new();

    for batch in reader {
        let batch = batch.context("failed to read record batch")?;

        let get_str_col = |name: &str| -> Option<&StringArray> {
            batch.column_by_name(name)
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        };

        let ids = get_str_col("product_id");
        let titles = get_str_col("product_title");
        let brands = get_str_col("product_brand");
        let colors = get_str_col("product_color");
        let descs = get_str_col("product_description");
        let locales = get_str_col("product_locale");

        let (Some(ids), Some(titles)) = (ids, titles) else { continue };

        for i in 0..batch.num_rows() {
            if max > 0 && products.len() >= max { break; }

            let locale = locales.map(|c| c.value(i)).unwrap_or("us");
            if let Some(lf) = locale_filter {
                if locale != lf { continue; }
            }

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

async fn import_products(
    products: &HashMap<String, EsciProduct>,
    server: &str,
    api_key: &str,
    batch_size: usize,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/products", server);
    let mut count = 0usize;
    let mut errors = 0usize;

    let mut batch: Vec<serde_json::Value> = Vec::with_capacity(batch_size);

    for product in products.values() {
        let mut text_parts = vec![product.title.clone()];
        if !product.brand.is_empty() { text_parts.push(product.brand.clone()); }
        if !product.color.is_empty() { text_parts.push(product.color.clone()); }

        let body = serde_json::json!({
            "id": product.id,
            "text": text_parts.join(". "),
            "metadata": {
                "title": product.title,
                "brand": product.brand,
                "color": product.color,
                "description": product.description.chars().take(500).collect::<String>(),
                "locale": product.locale,
            }
        });
        batch.push(body);

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

async fn flush_batch(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    batch: &mut Vec<serde_json::Value>,
    count: &mut usize,
    errors: &mut usize,
) {
    for body in batch.drain(..) {
        let resp = client
            .post(url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => *count += 1,
            _ => *errors += 1,
        }
    }
}

fn build_judged_queries(
    path: &PathBuf,
    products: &HashMap<String, EsciProduct>,
    split: &str,
) -> Result<Vec<JudgedQuery>> {
    let file = File::open(path).with_context(|| format!("cannot open {:?}", path))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    // Map: query text → list of Exact product IDs.
    let mut relevant: HashMap<String, Vec<String>> = HashMap::new();

    for batch in reader {
        let batch = batch.context("failed to read record batch")?;

        let get_str_col = |name: &str| -> Option<&StringArray> {
            batch.column_by_name(name)
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        };

        let (Some(queries), Some(product_ids), Some(labels)) = (
            get_str_col("query"),
            get_str_col("product_id"),
            get_str_col("esci_label"),
        ) else { continue };

        let splits = get_str_col("split");

        for i in 0..batch.num_rows() {
            if let Some(s) = splits {
                if s.value(i) != split { continue; }
            }

            let label = labels.value(i);
            if label != "E" { continue; }

            let product_id = product_ids.value(i).to_string();
            // Skip products not in our loaded product set.
            if !products.contains_key(&product_id) { continue; }

            let query = queries.value(i).to_string();
            relevant.entry(query).or_default().push(product_id);
        }
    }

    let judged: Vec<JudgedQuery> = relevant
        .into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(query, relevant_ids)| JudgedQuery {
            query,
            k: 10,
            relevant_ids,
        })
        .collect();

    Ok(judged)
}
