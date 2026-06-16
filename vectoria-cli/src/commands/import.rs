use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ImportArgs {
    /// Path to input file (NDJSON, CSV, or Parquet)
    pub file: PathBuf,
    /// Batch size for embedding calls
    #[arg(long, default_value = "64")]
    pub batch_size: usize,
}

enum Format { Ndjson, Csv, Parquet }

impl Format {
    fn detect(path: &PathBuf) -> Result<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("ndjson") | Some("jsonl") => Ok(Format::Ndjson),
            Some("csv") => Ok(Format::Csv),
            Some("parquet") => Ok(Format::Parquet),
            other => anyhow::bail!("unsupported format: {:?}. Use .ndjson, .csv, or .parquet", other),
        }
    }
}

pub async fn run(args: ImportArgs, server: &str, api_key: Option<String>) -> Result<()> {
    let format = Format::detect(&args.file)?;
    let client = reqwest::Client::new();
    let api_key = api_key.unwrap_or_default();
    let url = format!("{}/products", server);

    let mut count = 0usize;
    let mut errors = 0usize;

    match format {
        Format::Ndjson => {
            import_ndjson(&args.file, &client, &url, &api_key, &mut count, &mut errors).await?;
        }
        Format::Csv => {
            import_csv(&args.file, &client, &url, &api_key, &mut count, &mut errors).await?;
        }
        Format::Parquet => {
            import_parquet(&args.file, &client, &url, &api_key, &mut count, &mut errors).await?;
        }
    }

    println!("imported {} products ({} errors)", count, errors);
    Ok(())
}

async fn index_product(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    product: serde_json::Value,
    count: &mut usize,
    errors: &mut usize,
) -> Result<()> {
    let resp = client.post(url).bearer_auth(api_key).json(&product).send().await?;
    if resp.status().is_success() {
        *count += 1;
        if *count % 1000 == 0 {
            println!("  ... {} products indexed", count);
        }
    } else {
        *errors += 1;
        if *errors <= 5 {
            eprintln!("index error: {}", resp.text().await.unwrap_or_default());
        }
    }
    Ok(())
}

async fn import_ndjson(
    path: &PathBuf,
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    count: &mut usize,
    errors: &mut usize,
) -> Result<()> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).context("failed to open file")?;
    let reader = std::io::BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let product: serde_json::Value = serde_json::from_str(&line).context("invalid JSON")?;
        index_product(client, url, api_key, product, count, errors).await?;
    }
    Ok(())
}

async fn import_csv(
    path: &PathBuf,
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    count: &mut usize,
    errors: &mut usize,
) -> Result<()> {
    let mut rdr = csv::Reader::from_path(path).context("failed to open CSV")?;
    let headers = rdr.headers()?.clone();
    for result in rdr.records() {
        let record = result?;
        let mut map = serde_json::Map::new();
        for (h, f) in headers.iter().zip(record.iter()) {
            map.insert(h.to_string(), serde_json::Value::String(f.to_string()));
        }
        let id = map.get("id")
            .or_else(|| map.get("sku"))
            .or_else(|| map.get("product_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let product = serde_json::json!({ "id": id, "metadata": serde_json::Value::Object(map) });
        index_product(client, url, api_key, product, count, errors).await?;
    }
    Ok(())
}

async fn import_parquet(
    path: &PathBuf,
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    count: &mut usize,
    errors: &mut usize,
) -> Result<()> {
    use arrow::array::{Array, StringArray};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path).context("failed to open Parquet file")?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .context("failed to read Parquet metadata")?;
    let schema = builder.schema().clone();
    let reader = builder.build().context("failed to build Parquet reader")?;

    for batch in reader {
        let batch = batch.context("failed to read Parquet batch")?;
        let num_rows = batch.num_rows();

        for row_idx in 0..num_rows {
            let mut map = serde_json::Map::new();

            for (col_idx, field) in schema.fields().iter().enumerate() {
                let col = batch.column(col_idx);
                let value = if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                    if arr.is_null(row_idx) {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(arr.value(row_idx).to_string())
                    }
                } else {
                    serde_json::Value::String(
                        arrow::util::display::array_value_to_string(col.as_ref(), row_idx)
                            .unwrap_or_default()
                    )
                };
                map.insert(field.name().clone(), value);
            }

            let id = map.get("id")
                .or_else(|| map.get("sku"))
                .or_else(|| map.get("product_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("row_{}", count));

            let product = serde_json::json!({ "id": id, "metadata": serde_json::Value::Object(map) });
            index_product(client, url, api_key, product, count, errors).await?;
        }
    }
    Ok(())
}
