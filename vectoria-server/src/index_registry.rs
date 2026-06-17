use std::{
    collections::HashMap,
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
}

impl IndexRegistry {
    pub fn new(
        default_engine: Arc<SearchEngine>,
        embedding: Arc<dyn EmbeddingProvider>,
        default_weights: RankingWeights,
        query_cache_ttl: Option<u64>,
        query_cache_max: Option<usize>,
        field_weights: Option<HashMap<String, usize>>,
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
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<SearchEngine>> {
        self.engines.read().unwrap().get(name).cloned()
    }

    pub fn default_engine(&self) -> Arc<SearchEngine> {
        self.get("default").expect("default index always present")
    }

    /// Create a new in-memory index with the same configuration as the server default.
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

        let engine = Arc::new(builder.build().await.map_err(|_| CreateIndexError::BuildFailed)?);
        let mut engines = self.engines.write().unwrap();
        if engines.contains_key(name) {
            return Err(CreateIndexError::AlreadyExists);
        }
        engines.insert(name.to_string(), engine);
        Ok(())
    }

    /// Delete a named index. Cannot delete "default". Returns false if not found.
    pub fn delete(&self, name: &str) -> Result<bool, &'static str> {
        if name == "default" {
            return Err("cannot delete default index");
        }
        Ok(self.engines.write().unwrap().remove(name).is_some())
    }

    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.engines.read().unwrap().keys().cloned().collect();
        names.sort();
        names
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
        IndexRegistry::new(engine, embedding, RankingWeights::default(), Some(60), Some(100), None)
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
