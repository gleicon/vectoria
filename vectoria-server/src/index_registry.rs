use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use vectoria_core::{
    embedding::EmbeddingProvider,
    model::RankingWeights,
    search::SearchEngine,
    SearchEngineBuilder,
};

#[derive(Debug)]
pub enum CreateIndexError {
    AlreadyExists,
    LimitReached,
    BuildFailed,
}

impl std::fmt::Display for CreateIndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists => f.write_str("index already exists"),
            Self::LimitReached => f.write_str("index limit reached (max 100 named indexes)"),
            Self::BuildFailed => f.write_str("failed to build index"),
        }
    }
}

pub struct IndexRegistry {
    engines: RwLock<HashMap<String, Arc<SearchEngine>>>,
    embedding: Arc<dyn EmbeddingProvider>,
    default_weights: RankingWeights,
    query_cache_ttl: Option<u64>,
    query_cache_max: Option<usize>,
    field_weights: Option<HashMap<String, usize>>,
    /// When set, named indexes are persisted under `{data_dir}/{name}/` using EdgeStore.
    /// When None (tests, memory-only mode), named indexes use MemoryStorage.
    data_dir: Option<PathBuf>,
}

impl IndexRegistry {
    pub fn new(
        default_engine: Arc<SearchEngine>,
        embedding: Arc<dyn EmbeddingProvider>,
        default_weights: RankingWeights,
        query_cache_ttl: Option<u64>,
        query_cache_max: Option<usize>,
        field_weights: Option<HashMap<String, usize>>,
        data_dir: Option<PathBuf>,
    ) -> Self {
        let mut map = HashMap::new();
        map.insert("default".to_string(), default_engine);
        Self {
            engines: RwLock::new(map),
            embedding,
            default_weights,
            query_cache_ttl,
            query_cache_max,
            field_weights,
            data_dir,
        }
    }

    /// Load all previously-persisted indexes from `data_dir`.
    /// System indexes: `{data_dir}/{name}/`
    /// Tenant indexes: `{data_dir}/t/{tenant}/{index}/`
    pub async fn load_persisted(&self) {
        let Some(ref dir) = self.data_dir else { return };

        // System indexes: flat entries in data_dir (skip "t/" tenant dir)
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "t" || !entry.path().is_dir() { continue; }
                if self.engines.read().unwrap().contains_key(&name) { continue; }
                if let Err(e) = self.create(&name).await {
                    tracing::warn!("failed to reload system index '{}': {}", name, e);
                }
            }
        }

        // Tenant indexes: data_dir/t/{tenant}/{index}/
        let tenant_root = dir.join("t");
        if let Ok(tenants) = std::fs::read_dir(&tenant_root) {
            for tenant_entry in tenants.flatten() {
                if !tenant_entry.path().is_dir() { continue; }
                let tenant = tenant_entry.file_name().to_string_lossy().to_string();
                if let Ok(indexes) = std::fs::read_dir(tenant_entry.path()) {
                    for idx_entry in indexes.flatten() {
                        if !idx_entry.path().is_dir() { continue; }
                        let idx = idx_entry.file_name().to_string_lossy().to_string();
                        let key = format!("{tenant}/{idx}");
                        if self.engines.read().unwrap().contains_key(&key) { continue; }
                        if let Err(e) = self.create(&key).await {
                            tracing::warn!("failed to reload tenant index '{}': {}", key, e);
                        }
                    }
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<SearchEngine>> {
        self.engines.read().unwrap().get(name).cloned()
    }

    pub fn default_engine(&self) -> Arc<SearchEngine> {
        self.get("default").expect("default index always present")
    }

    /// Create a named index. When `data_dir` is configured, the index is persisted
    /// to disk via EdgeStore and survives server restarts. Otherwise uses MemoryStorage.
    pub async fn create(&self, name: &str) -> Result<(), CreateIndexError> {
        {
            let engines = self.engines.read().unwrap();
            if engines.contains_key(name) {
                return Err(CreateIndexError::AlreadyExists);
            }
            if engines.len() >= 101 {
                return Err(CreateIndexError::LimitReached);
            }
        }

        let mut builder = SearchEngineBuilder::new()
            .embedding(Arc::clone(&self.embedding))
            .weights(self.default_weights.clone());

        if let (Some(ttl), Some(max)) = (self.query_cache_ttl, self.query_cache_max) {
            builder = builder.query_cache(ttl, max);
        }
        if let Some(fw) = self.field_weights.clone() {
            builder = builder.field_weights(fw);
        }

        if let Some(ref dir) = self.data_dir {
            // Tenant indexes use `t/{tenant}/{index}/`; system indexes use `{name}/`.
            let index_dir = if name.contains('/') {
                dir.join("t").join(name)
            } else {
                dir.join(name)
            };
            std::fs::create_dir_all(&index_dir).map_err(|_| CreateIndexError::BuildFailed)?;
            use edgestore::{EdgestoreConfig, Engine};
            use vectoria_core::{
                storage::edgestore::EdgeStoreStorage,
                vector::edgestore::EdgeStoreVectorIndex,
            };
            let raw = Engine::open(EdgestoreConfig::new(&index_dir))
                .map_err(|_| CreateIndexError::BuildFailed)?;
            let engine_arc = Arc::new(std::sync::Mutex::new(raw));
            let store = Arc::new(EdgeStoreStorage::from_engine(Arc::clone(&engine_arc)));
            let vidx = Arc::new(
                EdgeStoreVectorIndex::from_engine(
                    engine_arc,
                    Some(self.embedding.model_id().to_string()),
                    Some(self.embedding.dims()),
                ).map_err(|_| CreateIndexError::BuildFailed)?,
            );
            builder = builder.storage(store).vector_index(vidx);
        }

        let engine = Arc::new(builder.build().await.map_err(|_| CreateIndexError::BuildFailed)?);
        let mut engines = self.engines.write().unwrap();
        if engines.contains_key(name) {
            return Err(CreateIndexError::AlreadyExists);
        }
        engines.insert(name.to_string(), engine);
        Ok(())
    }

    /// Delete a named index. Cannot delete "default". Returns false if not found.
    /// Also removes the persisted data directory if storage is configured.
    pub fn delete(&self, name: &str) -> Result<bool, &'static str> {
        if name == "default" {
            return Err("cannot delete default index");
        }
        let removed = self.engines.write().unwrap().remove(name).is_some();
        if removed {
            if let Some(ref dir) = self.data_dir {
                let index_dir = if name.contains('/') {
                    dir.join("t").join(name)
                } else {
                    dir.join(name)
                };
                if index_dir.exists() {
                    let _ = std::fs::remove_dir_all(&index_dir);
                }
            }
        }
        Ok(removed)
    }

    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.engines.read().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns the index names owned by `tenant` (strips the `{tenant}/` prefix).
    pub fn list_for_tenant(&self, tenant: &str) -> Vec<String> {
        let prefix = format!("{tenant}/");
        let mut names: Vec<String> = self.engines.read().unwrap()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|s| s.to_string()))
            .collect();
        names.sort();
        names
    }

    /// Removes all indexes whose key starts with `{tenant}/`.
    /// Called on tenant deletion to cascade-delete their indexes.
    pub fn delete_by_prefix(&self, tenant: &str) {
        let prefix = format!("{tenant}/");
        let keys: Vec<String> = self.engines.read().unwrap()
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in keys {
            let _ = self.delete(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use vectoria_core::model::{Product, SearchMode, SearchRequest};

    struct StubEmbed;

    #[async_trait]
    impl EmbeddingProvider for StubEmbed {
        async fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let b = text.as_bytes();
            Ok((0..16).map(|i| b.get(i % b.len().max(1)).copied().unwrap_or(0) as f32 / 255.0).collect())
        }
        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            let mut out = Vec::new();
            for t in texts { out.push(self.embed(t).await?); }
            Ok(out)
        }
        fn model_id(&self) -> &str { "stub" }
        fn dims(&self) -> usize { 16 }
    }

    async fn make_registry() -> IndexRegistry {
        let embedding: Arc<dyn EmbeddingProvider> = Arc::new(StubEmbed);
        let engine = Arc::new(
            SearchEngineBuilder::new()
                .embedding(Arc::clone(&embedding))
                .build()
                .await
                .unwrap(),
        );
        IndexRegistry::new(engine, embedding, RankingWeights::default(), Some(60), Some(100), None, None)
    }

    #[tokio::test]
    async fn test_create_and_list() {
        let r = make_registry().await;
        assert_eq!(r.list(), vec!["default"]);
        r.create("alpha").await.unwrap();
        let names = r.list();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"default".to_string()));
    }

    #[tokio::test]
    async fn test_duplicate_create_fails() {
        let r = make_registry().await;
        r.create("idx").await.unwrap();
        assert!(matches!(r.create("idx").await, Err(CreateIndexError::AlreadyExists)));
    }

    #[tokio::test]
    async fn test_delete() {
        let r = make_registry().await;
        r.create("temp").await.unwrap();
        assert_eq!(r.delete("temp"), Ok(true));
        assert_eq!(r.delete("temp"), Ok(false));
        assert_eq!(r.delete("default"), Err("cannot delete default index"));
    }

    #[tokio::test]
    async fn test_named_index_isolation() {
        let r = make_registry().await;
        r.create("ns-a").await.unwrap();

        let default_engine = r.default_engine();
        let ns_a = r.get("ns-a").unwrap();

        let mut p = Product::new("p1", serde_json::json!({"title": "running shoe"}));
        p.text = Some("running shoe".into());
        default_engine.index(p).await.unwrap();

        let resp = ns_a.search(SearchRequest {
            q: "running".into(),
            mode: SearchMode::Bm25,
            ..Default::default()
        }).await.unwrap();
        assert_eq!(resp.total, 0, "named index must not contain products from default");
    }
}
