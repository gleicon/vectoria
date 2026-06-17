use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use vectoria_core::model::RankingWeights;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VectoriaConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub ranking: RankingWeights,
    #[serde(default)]
    pub index: IndexConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub api_key: Option<String>,
    #[serde(default)]
    pub skip_consent: bool,
    #[serde(default)]
    pub rate_limit_per_second: Option<u32>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: None,
            skip_consent: false,
            rate_limit_per_second: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_path")]
    pub path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self { path: default_storage_path() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub dims: Option<usize>,
    pub fields: Option<std::collections::HashMap<String, usize>>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            base_url: None,
            api_key: None,
            dims: None,
            fields: None,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default = "default_vector_backend")]
    pub vector_backend: String,
    #[serde(default = "default_aggregation_interval_secs")]
    pub aggregation_interval_secs: u64,
    #[serde(default = "default_embedding_cache_size")]
    pub embedding_cache_size: usize,
    #[serde(default = "default_query_cache_ttl_secs")]
    pub query_cache_ttl_secs: u64,
    #[serde(default = "default_query_cache_max_entries")]
    pub query_cache_max_entries: usize,
    #[serde(default)]
    pub enable_reranker: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            vector_backend: default_vector_backend(),
            aggregation_interval_secs: default_aggregation_interval_secs(),
            embedding_cache_size: default_embedding_cache_size(),
            query_cache_ttl_secs: default_query_cache_ttl_secs(),
            query_cache_max_entries: default_query_cache_max_entries(),
            enable_reranker: false,
        }
    }
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 7700 }
fn default_storage_path() -> PathBuf { PathBuf::from("./vectoria.db") }
fn default_provider() -> String { "local".into() }
fn default_model() -> String { "multilingual-e5-small".into() }
fn default_vector_backend() -> String { "memory".into() }
fn default_aggregation_interval_secs() -> u64 { 300 }
fn default_embedding_cache_size() -> usize { 10_000 }
fn default_query_cache_ttl_secs() -> u64 { 60 }
fn default_query_cache_max_entries() -> usize { 1_000 }

impl VectoriaConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let path = std::env::var("VECTORIA_CONFIG").unwrap_or_else(|_| "vectoria.toml".into());

        let mut cfg = if std::path::Path::new(&path).exists() {
            let raw = std::fs::read_to_string(&path)?;
            toml::from_str(&raw)?
        } else {
            VectoriaConfig::default()
        };

        if let Ok(v) = std::env::var("VECTORIA_HOST") { cfg.server.host = v; }
        if let Ok(v) = std::env::var("VECTORIA_PORT") { cfg.server.port = v.parse()?; }
        if let Ok(v) = std::env::var("VECTORIA_API_KEY") { cfg.server.api_key = Some(v); }
        if std::env::var("VECTORIA_SKIP_CONSENT").as_deref() == Ok("1") { cfg.server.skip_consent = true; }
        if let Ok(v) = std::env::var("VECTORIA_STORAGE_PATH") { cfg.storage.path = v.into(); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_PROVIDER") { cfg.embedding.provider = v; }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_BASE_URL") { cfg.embedding.base_url = Some(v); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_MODEL") { cfg.embedding.model = v; }
        if std::env::var("VECTORIA_ENABLE_RERANKER").as_deref() == Ok("1") { cfg.index.enable_reranker = true; }

        Ok(cfg)
    }

    pub fn ensure_api_key(&mut self) -> String {
        if self.server.api_key.is_none() {
            let key = uuid::Uuid::new_v4().to_string().replace('-', "");
            self.server.api_key = Some(key);
        }
        self.server.api_key.clone().unwrap()
    }
}
