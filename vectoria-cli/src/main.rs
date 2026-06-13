use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "vectoria", version, about = "Vectoria — AI-native ecommerce search engine")]
struct Cli {
    /// Path to config file (default: vectoria.toml)
    #[arg(long, global = true, default_value = "vectoria.toml")]
    config: String,

    /// Vectoria server URL (for import/eval commands)
    #[arg(long, global = true, default_value = "http://localhost:7700")]
    server: String,

    /// API key (overrides config)
    #[arg(long, global = true)]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bulk import products from NDJSON, CSV, or Parquet
    Import(commands::import::ImportArgs),
    /// Re-embed all products with a new model
    Reindex(commands::reindex::ReindexArgs),
    /// Evaluate retrieval quality against a judged dataset
    Eval(commands::eval::EvalArgs),
    /// Benchmark search quality and latency across modes
    Bench(commands::bench::BenchArgs),
    /// Load Amazon ESCI dataset (import products and/or generate judged queries)
    Esci(commands::esci::EsciArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Import(args) => commands::import::run(args, &cli.server, cli.api_key).await,
        Commands::Reindex(args) => commands::reindex::run(args, &cli.server, cli.api_key).await,
        Commands::Eval(args) => commands::eval::run(args, &cli.server, cli.api_key).await,
        Commands::Bench(args) => commands::bench::run(args, &cli.server, cli.api_key).await,
        Commands::Esci(args) => commands::esci::run(args, &cli.server, cli.api_key).await,
    }
}
