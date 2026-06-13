use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use vectoria_core::{
    embedding::{cache::CachedEmbedding, EmbeddingProvider},
    model::RankingWeights,
    storage::edgestore::EdgeStoreStorage,
    storage::memory::MemoryStorage,
    vector::edgestore::EdgeStoreVectorIndex,
    vector::memory::MemoryVectorIndex,
    SearchEngine,
};

/// Lazy registry of per-indexName SearchEngines.
/// First access to an unknown index creates a new engine backed by its own EdgeStore file.
/// This gives true multi-index isolation at the cost of one EdgeStore file pair per index.
pub struct IndexRegistry {
    engines: RwLock<HashMap<String, Arc<SearchEngine>>>,
    embedding: Arc<dyn EmbeddingProvider>,
    weights: RankingWeights,
    /// Directory where per-index EdgeStore files are created.
    /// None = memory-backed (development mode).
    data_dir: Option<PathBuf>,
    query_cache_ttl: u64,
    query_cache_max: usize,
}

impl IndexRegistry {
    pub fn new(
        embedding: Arc<dyn EmbeddingProvider>,
        weights: RankingWeights,
        data_dir: Option<PathBuf>,
        query_cache_ttl: u64,
        query_cache_max: usize,
    ) -> Self {
        Self {
            engines: RwLock::new(HashMap::new()),
            embedding,
            weights,
            data_dir,
            query_cache_ttl,
            query_cache_max,
        }
    }

    /// Get (or lazily create) the engine for the given indexName.
    pub fn get_or_create(&self, index_name: &str) -> Result<Arc<SearchEngine>> {
        // Fast path: already exists.
        {
            let map = self.engines.read().unwrap();
            if let Some(engine) = map.get(index_name) {
                return Ok(Arc::clone(engine));
            }
        }

        // Slow path: create and insert.
        let engine = self.build_engine(index_name)?;
        let engine = Arc::new(engine);
        {
            let mut map = self.engines.write().unwrap();
            // Double-checked: another thread may have created it while we waited.
            map.entry(index_name.to_string()).or_insert_with(|| Arc::clone(&engine));
        }
        Ok(engine)
    }

    fn build_engine(&self, index_name: &str) -> Result<SearchEngine> {
        let embedding = Arc::new(CachedEmbedding::new(
            Arc::clone(&self.embedding),
            10_000,
        ));

        let engine = match &self.data_dir {
            Some(dir) => {
                let safe_name = sanitize_index_name(index_name);
                let db_path = dir.join(format!("{}.db", safe_name));
                let vec_path = dir.join(format!("{}.vec", safe_name));
                let storage = Arc::new(
                    EdgeStoreStorage::open(&db_path)
                        .map_err(|e| anyhow::anyhow!("failed to open storage for index '{}': {}", index_name, e))?,
                );
                let vidx = Arc::new(
                    EdgeStoreVectorIndex::open(
                        vec_path,
                        Some(self.embedding.model_id().to_string()),
                        Some(self.embedding.dims()),
                    )
                    .map_err(|e| anyhow::anyhow!("failed to open vector index for '{}': {}", index_name, e))?,
                );
                SearchEngine::new(storage, vidx, embedding, self.weights.clone())
            }
            None => {
                let storage = Arc::new(MemoryStorage::new());
                let vidx = Arc::new(MemoryVectorIndex::new(
                    Some(self.embedding.model_id().to_string()),
                    Some(self.embedding.dims()),
                ));
                SearchEngine::new(storage, vidx, embedding, self.weights.clone())
            }
        };

        Ok(engine
            .with_query_cache(self.query_cache_ttl, self.query_cache_max))
    }
}

/// Restrict index names to safe filesystem characters.
fn sanitize_index_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(64)
        .collect()
}
