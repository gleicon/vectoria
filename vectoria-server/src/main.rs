mod auth;
mod config;
mod index_registry;
mod rate_limit;
mod routes;
mod state;
mod storage_factory;
mod tenants;

use anyhow::Result;
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use clap::Parser;
use index_registry::IndexRegistry;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vectoria_core::{
    aggregation::run_aggregation_loop,
    embedding::{cache::CachedEmbedding, EmbeddingProvider},
    search::{llm_rewriter::LlmRewriter, reranker::CrossEncoderReranker},
    SearchEngineBuilder,
};

use config::VectoriaConfig;
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

    if cfg.embedding.provider == "local" {
        let skip = args.skip_consent || cfg.server.skip_consent;
        if !skip && !is_model_cached() {
            prompt_model_download_consent(&cfg.embedding.model)?;
        }
    }

    let embedding: Arc<dyn EmbeddingProvider> = Arc::new(CachedEmbedding::new(
        build_embedding_provider(&cfg, args.skip_model_download)?,
        cfg.index.embedding_cache_size,
    ));

    let (storage, vector_index) = storage_factory::open(&cfg.index, &cfg.storage, &embedding)?;

    let weights = cfg.ranking.clone();
    let query_cache_ttl = cfg.index.query_cache_ttl_secs;
    let query_cache_max = cfg.index.query_cache_max_entries;

    let mut builder = SearchEngineBuilder::new()
        .storage(Arc::clone(&storage))
        .vector_index(Arc::clone(&vector_index))
        .embedding(Arc::clone(&embedding))
        .weights(weights.clone())
        .query_cache(query_cache_ttl, query_cache_max);

    if let Some(fw) = cfg.embedding.fields.clone() {
        builder = builder.field_weights(fw);
    }

    if cfg.index.enable_reranker {
        match CrossEncoderReranker::new() {
            Ok(reranker) => {
                builder = builder.with_reranker_instance(reranker);
                tracing::info!("cross-encoder reranker: enabled");
            }
            Err(e) => tracing::warn!("reranker init failed (continuing without): {}", e),
        }
    }

    if let Some(qe_provider) = &cfg.query_embedding.provider {
        let query_embedder: Option<Arc<dyn EmbeddingProvider>> = match qe_provider.as_str() {
            "openai-compatible" => {
                use vectoria_core::embedding::openai::OpenAIEmbedding;
                if let Some(base_url) = &cfg.query_embedding.base_url {
                    let model = cfg.query_embedding.model.as_deref().unwrap_or("text-embedding-3-small");
                    let dims = cfg.query_embedding.dims.unwrap_or(384);
                    tracing::info!("query tower: openai-compatible '{}' at {}", model, base_url);
                    Some(Arc::new(OpenAIEmbedding::new(
                        base_url.clone(),
                        model,
                        cfg.query_embedding.api_key.clone(),
                        dims,
                    )))
                } else {
                    tracing::warn!("query_embedding.provider=openai-compatible but no base_url set; skipped");
                    None
                }
            }
            other => {
                tracing::warn!("unknown query_embedding.provider '{}'; skipped", other);
                None
            }
        };
        if let Some(qe) = query_embedder {
            builder = builder.with_query_embedder(qe);
        }
    }

    if cfg.llm.enabled {
        if let Some(base_url) = &cfg.llm.base_url {
            let rewriter = LlmRewriter::new(base_url, &cfg.llm.model, cfg.llm.api_key.clone());
            builder = builder.with_llm_rewriter(rewriter);
            tracing::info!("llm rewriter: enabled ({})", cfg.llm.model);
        } else {
            tracing::warn!("llm.enabled = true but llm.base_url not set; rewriter disabled");
        }
    }

    let engine = Arc::new(builder.build().await?);

    tokio::spawn(run_aggregation_loop(Arc::clone(&storage), cfg.index.aggregation_interval_secs));

    let limiter = cfg.server.rate_limit_per_second.map(|rps| {
        tracing::info!("rate limiting: {} requests/sec per IP", rps);
        rate_limit::new_limiter(rps)
    });

    // Derive sibling directories from the storage path: `./vectoria.db` → `./indexes/`, `./tenants.json`
    let data_root = cfg.storage.path.parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let named_indexes_dir = if cfg.index.vector_backend.starts_with("edgestore") {
        Some(data_root.join("indexes"))
    } else {
        None
    };
    let tenant_store_path = data_root.join("tenants.json");
    let tenant_store = Arc::new(match tenants::TenantStore::load(tenant_store_path) {
        Ok(s) => s,
        Err(e) => { tracing::warn!("tenant store failed to load, starting empty: {e}"); tenants::TenantStore::empty() }
    });

    let registry = Arc::new(IndexRegistry::new(
        Arc::clone(&engine),
        Arc::clone(&embedding),
        weights,
        Some(query_cache_ttl),
        Some(query_cache_max),
        cfg.embedding.fields.clone(),
        named_indexes_dir,
    ));

    registry.load_persisted().await;

    let tenant_keys: std::collections::HashMap<String, String> = cfg
        .tenants
        .iter()
        .map(|t| (t.api_key.clone(), t.name.clone()))
        .collect();
    if !tenant_keys.is_empty() {
        tracing::info!("multi-tenancy: {} tenant(s) configured", tenant_keys.len());
    }

    let state = AppState {
        registry,
        api_key: api_key.clone(),
        tenant_keys: std::sync::Arc::new(tenant_keys),
        tenant_store,
        limiter,
    };

    // Admin-only routes: require both a valid API key AND the admin principal.
    let admin_routes = Router::new()
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
        .route("/admin/aggregate", post(routes::admin::trigger_aggregation))
        .route("/admin/overrides", get(routes::admin::list_overrides))
        .route("/admin/training-export", get(routes::admin::export_overrides))
        .route("/admin/training-import", post(routes::admin::import_overrides))
        .route("/admin/pins", get(routes::pins::list_pins))
        .route("/admin/pins", post(routes::pins::create_pin))
        .route("/admin/pins/{id}", delete(routes::pins::delete_pin))
        .route("/admin/sponsored", get(routes::sponsored::list_sponsored))
        .route("/admin/sponsored", post(routes::sponsored::create_sponsored))
        .route("/admin/sponsored/{id}", delete(routes::sponsored::delete_sponsored))
        .route("/admin/suppressions", get(routes::suppressions::list_suppressions))
        .route("/admin/suppressions", post(routes::suppressions::create_suppression))
        .route("/admin/suppressions/{id}", delete(routes::suppressions::delete_suppression))
        .route("/admin/tenants", get(routes::tenants::list_tenants))
        .route("/admin/tenants", post(routes::tenants::create_tenant))
        .route("/admin/tenants/{name}", delete(routes::tenants::delete_tenant))
        .route("/admin/tenants/{name}/indexes", get(routes::tenants::list_tenant_indexes))
        .route("/admin/tenants/{name}/rotate-key", post(routes::tenants::rotate_key))
        .route("/products/{id}/related", get(routes::products::related_products))
        .layer(middleware::from_fn(auth::require_admin));

    // Tenant-accessible routes: API key auth only; handlers enforce namespace scoping.
    // Per-index admin routes are here (not under require_admin) because tenants need
    // to manage their own index's pins, sponsored slots, and suppressions.
    let tenant_routes = Router::new()
        .route("/indexes", get(routes::indexes::list_indexes))
        .route("/indexes", post(routes::indexes::create_index))
        .route("/indexes/{name}", delete(routes::indexes::delete_index))
        .route("/indexes/{name}/products", post(routes::indexes::index_product))
        .route("/indexes/{name}/search", post(routes::indexes::search))
        .route("/indexes/{name}/similar", post(routes::indexes::similar))
        .route("/indexes/{name}/admin/pins", get(routes::index_admin::list_pins))
        .route("/indexes/{name}/admin/pins", post(routes::index_admin::create_pin))
        .route("/indexes/{name}/admin/pins/{id}", delete(routes::index_admin::delete_pin))
        .route("/indexes/{name}/admin/sponsored", get(routes::index_admin::list_sponsored))
        .route("/indexes/{name}/admin/sponsored", post(routes::index_admin::create_sponsored))
        .route("/indexes/{name}/admin/sponsored/{id}", delete(routes::index_admin::delete_sponsored))
        .route("/indexes/{name}/admin/suppressions", get(routes::index_admin::list_suppressions))
        .route("/indexes/{name}/admin/suppressions", post(routes::index_admin::create_suppression))
        .route("/indexes/{name}/admin/suppressions/{id}", delete(routes::index_admin::delete_suppression))
        .route("/indexes/{name}/admin/reindex", post(routes::index_admin::reindex))
        .route("/indexes/{name}/admin/stats", get(routes::index_admin::stats))
        .route("/indexes/{name}/admin/overrides", get(routes::index_admin::list_overrides))
        .route("/indexes/{name}/admin/aggregate", post(routes::index_admin::trigger_aggregation))
        .route("/indexes/{name}/admin/training-export", get(routes::index_admin::export_overrides))
        .route("/indexes/{name}/admin/training-import", post(routes::index_admin::import_overrides))
        .route("/users/{id}/recommendations", get(routes::users::get_recommendations));

    let protected = Router::new()
        .merge(admin_routes)
        .merge(tenant_routes)
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key));

    let public = Router::new()
        .route("/health", get(routes::admin::health));

    let app = Router::new()
        .merge(protected)
        .merge(public)
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit::rate_limit_middleware))
        .with_state(state)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", cfg.server.host, cfg.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{}", addr);
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    Ok(())
}

fn is_model_cached() -> bool {
    let cache_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("fastembed");
    cache_dir.exists()
        && std::fs::read_dir(&cache_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
}

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
            Ok(Arc::new(LocalEmbedding::default_model()?))
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
