use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectoriaConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub ranking: RankingConfig,
    #[serde(default)]
    pub index: IndexConfig,
}

impl Default for VectoriaConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            embedding: EmbeddingConfig::default(),
            ranking: RankingConfig::default(),
            index: IndexConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Auto-generated on first run if absent.
    pub api_key: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: None,
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
    /// "local" | "openai-compatible" | "none"
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Required when provider = "openai-compatible".
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    /// Output dimensions (required for openai-compatible provider).
    pub dims: Option<usize>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            base_url: None,
            api_key: None,
            dims: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    #[serde(default = "w_semantic")]
    pub semantic: f32,
    #[serde(default = "w_popularity")]
    pub popularity: f32,
    #[serde(default = "w_availability")]
    pub availability: f32,
    #[serde(default = "w_margin")]
    pub margin: f32,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            semantic: w_semantic(),
            popularity: w_popularity(),
            availability: w_availability(),
            margin: w_margin(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// "edgestore-hnsw" | "turbovec" | "memory"
    #[serde(default = "default_vector_backend")]
    pub vector_backend: String,
    /// How often (seconds) the background aggregation job runs. Default: 300 (5 min).
    pub aggregation_interval_secs: Option<u64>,
    /// Max number of query embedding vectors to hold in the foyer LRU cache. Default: 10_000.
    pub embedding_cache_size: Option<usize>,
    /// TTL seconds for head query result cache. Default: 60.
    pub query_cache_ttl_secs: Option<u64>,
    /// Max cached query results. Default: 1_000.
    pub query_cache_max_entries: Option<usize>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            vector_backend: default_vector_backend(),
            aggregation_interval_secs: None,
            embedding_cache_size: None,
            query_cache_ttl_secs: None,
            query_cache_max_entries: None,
        }
    }
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 7700 }
fn default_storage_path() -> PathBuf { PathBuf::from("./vectoria.db") }
fn default_provider() -> String { "local".into() }
fn default_model() -> String { "multilingual-e5-small".into() }
fn default_vector_backend() -> String { "memory".into() }
fn w_semantic() -> f32 { 0.6 }
fn w_popularity() -> f32 { 0.2 }
fn w_availability() -> f32 { 0.1 }
fn w_margin() -> f32 { 0.1 }

impl VectoriaConfig {
    /// Load from vectoria.toml + env var overrides (VECTORIA_*).
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let path = std::env::var("VECTORIA_CONFIG").unwrap_or_else(|_| "vectoria.toml".into());

        let mut cfg = if std::path::Path::new(&path).exists() {
            let raw = std::fs::read_to_string(&path)?;
            toml::from_str(&raw)?
        } else {
            VectoriaConfig::default()
        };

        // Env var overrides.
        if let Ok(v) = std::env::var("VECTORIA_HOST") { cfg.server.host = v; }
        if let Ok(v) = std::env::var("VECTORIA_PORT") { cfg.server.port = v.parse()?; }
        if let Ok(v) = std::env::var("VECTORIA_API_KEY") { cfg.server.api_key = Some(v); }
        if let Ok(v) = std::env::var("VECTORIA_STORAGE_PATH") { cfg.storage.path = v.into(); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_PROVIDER") { cfg.embedding.provider = v; }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_BASE_URL") { cfg.embedding.base_url = Some(v); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_MODEL") { cfg.embedding.model = v; }

        Ok(cfg)
    }

    /// Generate and persist an API key if not already set.
    pub fn ensure_api_key(&mut self) -> String {
        if self.server.api_key.is_none() {
            let key = uuid::Uuid::new_v4().to_string().replace('-', "");
            self.server.api_key = Some(key);
        }
        self.server.api_key.clone().unwrap()
    }
}
