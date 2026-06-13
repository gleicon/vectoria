mod auth;
mod config;
mod index_registry;
mod routes;
mod state;

use anyhow::Result;
use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use clap::Parser;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vectoria_core::{
    aggregation::run_aggregation_loop,
    embedding::{cache::CachedEmbedding, EmbeddingProvider},
    model::RankingWeights,
    search::reranker::CrossEncoderReranker,
    storage::{edgestore::EdgeStoreStorage, memory::MemoryStorage, sqlite::SqliteStorage},
    vector::{edgestore::EdgeStoreVectorIndex, memory::MemoryVectorIndex, turbovec::TurboVecIndex},
    SearchEngine,
};

use config::VectoriaConfig;
use index_registry::IndexRegistry;
use state::AppState;

#[derive(Parser)]
#[command(name = "vectoria-server", version, about = "Vectoria search server")]
struct ServerArgs {
    /// Skip the first-run model download consent prompt.
    /// Also respected via VECTORIA_SKIP_CONSENT=1.
    #[arg(long)]
    skip_consent: bool,

    /// Skip downloading the local embedding model (server will fail if model is absent).
    #[arg(long)]
    skip_model_download: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = ServerArgs::parse();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "vectoria=info,tower_http=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut cfg = VectoriaConfig::load()?;
    let api_key = cfg.ensure_api_key();

    tracing::info!("vectoria v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("api_key: {}", api_key);

    // First-run consent: if local embedding model is absent, ask before downloading.
    if cfg.embedding.provider == "local" {
        let skip = args.skip_consent
            || std::env::var("VECTORIA_SKIP_CONSENT").as_deref() == Ok("1");
        if !skip && !is_model_cached() {
            prompt_model_download_consent(&cfg.embedding.model)?;
        }
    }

    let raw_embedding = build_embedding_provider(&cfg, args.skip_model_download)?;
    let embedding_cache_size = cfg.index.embedding_cache_size.unwrap_or(10_000);
    let embedding: Arc<dyn EmbeddingProvider> = Arc::new(CachedEmbedding::new(
        Arc::clone(&raw_embedding),
        embedding_cache_size,
    ));

    let (storage, vector_index): (
        Arc<dyn vectoria_core::storage::StorageEngine>,
        Arc<dyn vectoria_core::vector::VectorIndex>,
    ) = match cfg.index.vector_backend.as_str() {
        "edgestore-hnsw" | "edgestore" => {
            let db_path = &cfg.storage.path;
            let vec_path = db_path.with_extension("vec");
            tracing::info!("storage: EdgeStore at {:?}", db_path);
            let storage = Arc::new(
                EdgeStoreStorage::open(db_path)
                    .expect("failed to open EdgeStore storage"),
            );
            let vidx = Arc::new(
                EdgeStoreVectorIndex::open(
                    vec_path,
                    Some(embedding.model_id().to_string()),
                    Some(embedding.dims()),
                )
                .expect("failed to open EdgeStore vector index"),
            );
            (storage, vidx)
        }
        "sqlite" => {
            let db_path = cfg.storage.path.with_extension("sqlite");
            let vec_path = db_path.with_extension("turbovec");
            tracing::info!("storage: SQLite at {:?}, vectors: TurboVec at {:?}", db_path, vec_path);
            let storage = Arc::new(
                SqliteStorage::open(&db_path).expect("failed to open SQLite storage"),
            );
            let vidx = Arc::new(
                TurboVecIndex::open(&vec_path, Some(embedding.model_id().to_string()), Some(embedding.dims()))
                    .expect("failed to open TurboVec index"),
            );
            (storage, vidx)
        }
        "turbovec" => {
            let vec_path = cfg.storage.path.with_extension("turbovec");
            tracing::info!("storage: in-memory (metadata), vectors: TurboVec at {:?}", vec_path);
            let storage = Arc::new(MemoryStorage::new());
            let vidx = Arc::new(
                TurboVecIndex::open(&vec_path, Some(embedding.model_id().to_string()), Some(embedding.dims()))
                    .expect("failed to open TurboVec index"),
            );
            (storage, vidx)
        }
        _ => {
            tracing::info!("storage: in-memory (set index.vector_backend = \"edgestore-hnsw\" for persistence)");
            let storage = Arc::new(MemoryStorage::new());
            let vidx = Arc::new(MemoryVectorIndex::new(
                Some(embedding.model_id().to_string()),
                Some(embedding.dims()),
            ));
            (storage, vidx)
        }
    };

    let weights = RankingWeights {
        semantic: cfg.ranking.semantic,
        popularity: cfg.ranking.popularity,
        availability: cfg.ranking.availability,
        margin: cfg.ranking.margin,
    };

    let query_cache_ttl = cfg.index.query_cache_ttl_secs.unwrap_or(60);
    let query_cache_max = cfg.index.query_cache_max_entries.unwrap_or(1_000);

    let mut engine = SearchEngine::new(
        Arc::clone(&storage),
        Arc::clone(&vector_index),
        Arc::clone(&embedding),
        weights.clone(),
    )
    .with_query_cache(query_cache_ttl, query_cache_max);

    if std::env::var("VECTORIA_ENABLE_RERANKER").as_deref() == Ok("1") {
        match CrossEncoderReranker::new() {
            Ok(reranker) => {
                engine = engine.with_reranker(reranker);
                tracing::info!("cross-encoder reranker: enabled");
            }
            Err(e) => tracing::warn!("reranker init failed (continuing without): {}", e),
        }
    }

    let engine = Arc::new(engine);

    // Background aggregation: pre-compute popularity/conversion signals.
    let agg_storage = Arc::clone(&storage);
    let agg_interval = cfg.index.aggregation_interval_secs.unwrap_or(300);
    tokio::spawn(run_aggregation_loop(agg_storage, agg_interval));

    // Per-index engine registry for multi-index endpoints.
    let data_dir = match cfg.index.vector_backend.as_str() {
        "edgestore-hnsw" | "edgestore" | "sqlite" | "turbovec" => {
            cfg.storage.path.parent().map(|p| p.to_path_buf())
        }
        _ => None,
    };
    let index_registry = Arc::new(IndexRegistry::new(
        raw_embedding,
        weights,
        data_dir,
        query_cache_ttl,
        query_cache_max,
    ));

    let state = AppState { engine, index_registry, api_key: api_key.clone() };

    let protected = Router::new()
        .route("/products", post(routes::products::index_product))
        .route("/products/{id}", put(routes::products::update_product))
        .route("/products/{id}", delete(routes::products::delete_product))
        .route("/products/{id}/similar", get(routes::products::similar_by_id))
        .route("/products/similar", post(routes::products::similar_flexible))
        .route("/search", post(routes::search::search))
        .route("/autocomplete", get(routes::search::autocomplete))
        .route("/events", post(routes::events::record_event))
        .route("/stats", get(routes::admin::stats))
        .route("/admin/reindex", post(routes::admin::reindex))
        .route("/1/indexes/{indexName}", post(routes::admin::indexes::index_object))
        .route("/1/indexes/{indexName}/{objectID}", put(routes::admin::indexes::update_object))
        .route("/1/indexes/{indexName}/{objectID}", delete(routes::admin::indexes::delete_object))
        .route("/1/indexes/{indexName}/query", post(routes::admin::indexes::search))
        .route("/1/indexes/{indexName}/batch", post(routes::admin::indexes::batch))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key));

    let public = Router::new()
        .route("/health", get(routes::admin::health));

    let app = Router::new()
        .merge(protected)
        .merge(public)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", cfg.server.host, cfg.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Returns true if a fastembed model cache directory exists.
/// fastembed-rs caches models under ~/.cache/fastembed/ on Linux/macOS.
fn is_model_cached() -> bool {
    let cache_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("fastembed");
    cache_dir.exists()
        && std::fs::read_dir(&cache_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
}

/// Prompt the user to consent to a model download. Bails if they decline.
fn prompt_model_download_consent(model: &str) -> Result<()> {
    eprintln!();
    eprintln!("Vectoria uses a local embedding model for semantic search.");
    eprintln!("  Model : {}", model);
    eprintln!("  Size  : ~40 MB (quantized ONNX, cached after first download)");
    eprintln!("  Cache : ~/.cache/fastembed/");
    eprintln!();
    eprint!("Download now? [Y/n] ");
    use std::io::Write;
    std::io::stderr().flush().ok();

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_lowercase();
    if answer == "n" || answer == "no" {
        anyhow::bail!(
            "Model download declined. Re-run with --skip-consent to suppress this prompt \
             or set VECTORIA_EMBEDDING_PROVIDER=openai-compatible for a remote model."
        );
    }
    Ok(())
}

fn build_embedding_provider(cfg: &VectoriaConfig, skip_download: bool) -> Result<Arc<dyn EmbeddingProvider>> {
    match cfg.embedding.provider.as_str() {
        "local" => {
            use vectoria_core::embedding::local::LocalEmbedding;
            tracing::info!("embedding: local model '{}'", cfg.embedding.model);
            if skip_download && !is_model_cached() {
                anyhow::bail!(
                    "--skip-model-download set but model not cached. \
                     Remove the flag to allow download on first run."
                );
            }
            let embedding = LocalEmbedding::default_model()?;
            Ok(Arc::new(embedding))
        }
        "openai-compatible" => {
            use vectoria_core::embedding::openai::OpenAIEmbedding;
            let base_url = cfg.embedding.base_url.clone()
                .ok_or_else(|| anyhow::anyhow!("embedding.base_url required for openai-compatible provider"))?;
            let dims = cfg.embedding.dims.unwrap_or(384);
            tracing::info!("embedding: openai-compatible '{}' at {}", cfg.embedding.model, base_url);
            Ok(Arc::new(OpenAIEmbedding::new(
                base_url,
                &cfg.embedding.model,
                cfg.embedding.api_key.clone(),
                dims,
            )))
        }
        "none" => anyhow::bail!(
            "embedding provider is 'none'. Configure [embedding] in vectoria.toml \
             or set VECTORIA_EMBEDDING_PROVIDER."
        ),
        other => anyhow::bail!("unknown embedding provider: '{}'", other),
    }
}
