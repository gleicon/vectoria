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
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
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
pub struct IndexConfig {
    // Accepted values: "memory" | "sqlite" | "edgestore" | "edgestore-hnsw"
    #[serde(default = "default_vector_backend")]
    pub vector_backend: String,
    pub aggregation_interval_secs: Option<u64>,
    pub embedding_cache_size: Option<usize>,
    pub query_cache_ttl_secs: Option<u64>,
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
        if let Ok(v) = std::env::var("VECTORIA_STORAGE_PATH") { cfg.storage.path = v.into(); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_PROVIDER") { cfg.embedding.provider = v; }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_BASE_URL") { cfg.embedding.base_url = Some(v); }
        if let Ok(v) = std::env::var("VECTORIA_EMBEDDING_MODEL") { cfg.embedding.model = v; }

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
