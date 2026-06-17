use crate::{
    embedding::{
        cache::CachedEmbedding,
        local::LocalEmbedding,
        EmbeddingProvider,
    },
    model::{
        Hit, RankingWeights, SearchRequest, SearchResponse, SimilarRequest,
    },
    search::{reranker::CrossEncoderReranker, SearchEngine},
    storage::{memory::MemoryStorage, StorageEngine},
    vector::{memory::MemoryVectorIndex, VectorIndex},
};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// Builder for [`SearchEngine`]. All fields are optional; defaults use
/// in-memory storage and `multilingual-e5-small` local embeddings.
///
/// # Example
/// ```rust,no_run
/// use vectoria_core::SearchEngineBuilder;
///
/// #[tokio::main]
/// async fn main() {
///     let engine = SearchEngineBuilder::new()
///         .query_cache(300, 1000)
///         .build()
///         .await
///         .unwrap();
/// }
/// ```
pub struct SearchEngineBuilder {
    storage: Option<Arc<dyn StorageEngine>>,
    vector_index: Option<Arc<dyn VectorIndex>>,
    embedding: Option<Arc<dyn EmbeddingProvider>>,
    weights: Option<RankingWeights>,
    query_cache_ttl: Option<u64>,
    query_cache_max: Option<usize>,
    reranker: bool,
    reranker_instance: Option<CrossEncoderReranker>,
    field_weights: Option<HashMap<String, usize>>,
}

impl Default for SearchEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchEngineBuilder {
    pub fn new() -> Self {
        Self {
            storage: None,
            vector_index: None,
            embedding: None,
            weights: None,
            query_cache_ttl: None,
            query_cache_max: None,
            reranker: false,
            reranker_instance: None,
            field_weights: None,
        }
    }

    pub fn storage(mut self, storage: Arc<dyn StorageEngine>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn vector_index(mut self, index: Arc<dyn VectorIndex>) -> Self {
        self.vector_index = Some(index);
        self
    }

    pub fn embedding(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding = Some(provider);
        self
    }

    pub fn weights(mut self, weights: RankingWeights) -> Self {
        self.weights = Some(weights);
        self
    }

    /// Enable result cache with the given TTL (seconds) and max entry count.
    pub fn query_cache(mut self, ttl_secs: u64, max_entries: usize) -> Self {
        self.query_cache_ttl = Some(ttl_secs);
        self.query_cache_max = Some(max_entries);
        self
    }

    /// Enable cross-encoder reranking (builder creates the model during `build()`).
    pub fn reranker(mut self) -> Self {
        self.reranker = true;
        self
    }

    /// Supply a pre-built reranker. Avoids creating the model twice when the caller
    /// needs to check for init errors before building the engine.
    pub fn with_reranker_instance(mut self, r: CrossEncoderReranker) -> Self {
        self.reranker_instance = Some(r);
        self
    }

    /// Configure per-field repetition weights for embedding text construction.
    /// Fields repeated more times get proportionally more weight in the embedding.
    pub fn field_weights(mut self, weights: HashMap<String, usize>) -> Self {
        self.field_weights = Some(weights);
        self
    }

    /// Build the engine. Initializes the local ONNX model if no embedding provider was given.
    pub async fn build(self) -> Result<SearchEngine> {
        let embedding: Arc<dyn EmbeddingProvider> = match self.embedding {
            Some(e) => e,
            None => {
                let local = LocalEmbedding::default_model()?;
                Arc::new(CachedEmbedding::new(Arc::new(local), 10_000))
            }
        };

        let storage: Arc<dyn StorageEngine> = self
            .storage
            .unwrap_or_else(|| Arc::new(MemoryStorage::new()));

        let vector_index: Arc<dyn VectorIndex> = self.vector_index.unwrap_or_else(|| {
            Arc::new(MemoryVectorIndex::new(
                Some(embedding.model_id().to_string()),
                Some(embedding.dims()),
            ))
        });

        let weights = self.weights.unwrap_or_default();

        let mut engine = SearchEngine::new(storage, vector_index, embedding, weights);

        if let (Some(ttl), Some(max)) = (self.query_cache_ttl, self.query_cache_max) {
            engine = engine.with_query_cache(ttl, max);
        }

        if let Some(r) = self.reranker_instance {
            engine = engine.with_reranker(r);
        } else if self.reranker {
            if let Ok(r) = CrossEncoderReranker::new() {
                engine = engine.with_reranker(r);
            }
        }

        if let Some(fw) = self.field_weights {
            engine = engine.with_field_weights(fw);
        }

        Ok(engine)
    }
}

/// Synchronous wrapper around [`SearchEngine`] for non-async callers.
///
/// Creates its own single-threaded Tokio runtime internally.
///
/// # Example
/// ```rust,no_run
/// use vectoria_core::SearchEngineSync;
/// use vectoria_core::model::{SearchRequest, SearchMode};
///
/// let engine = SearchEngineSync::new().unwrap();
/// let results = engine.search(SearchRequest {
///     q: "running shoes".into(),
///     mode: SearchMode::Hybrid,
///     limit: 10,
///     ..Default::default()
/// }).unwrap();
/// ```
pub struct SearchEngineSync {
    inner: SearchEngine,
    rt: tokio::runtime::Runtime,
}

impl SearchEngineSync {
    /// Build with defaults (in-memory storage, local ONNX embeddings).
    pub fn new() -> Result<Self> {
        Self::from_builder(SearchEngineBuilder::new())
    }

    pub fn from_builder(builder: SearchEngineBuilder) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let inner = rt.block_on(builder.build())?;
        Ok(Self { inner, rt })
    }

    pub fn index(&self, product: crate::model::Product) -> Result<()> {
        self.rt.block_on(self.inner.index(product))
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.rt.block_on(self.inner.delete(id))
    }

    pub fn search(&self, req: SearchRequest) -> Result<SearchResponse> {
        self.rt.block_on(self.inner.search(req))
    }

    pub fn similar(&self, req: SimilarRequest) -> Result<Vec<Hit>> {
        self.rt.block_on(self.inner.similar(req))
    }

    pub fn reindex(&self) -> Result<crate::search::ReindexReport> {
        self.rt.block_on(self.inner.reindex_all())
    }

    pub fn stats(&self) -> Result<crate::search::EngineStats> {
        self.rt.block_on(self.inner.stats())
    }
}
