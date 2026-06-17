use std::sync::Arc;
use vectoria_core::{
    embedding::EmbeddingProvider,
    storage::{edgestore::EdgeStoreStorage, memory::MemoryStorage, StorageEngine},
    vector::{edgestore::EdgeStoreVectorIndex, memory::MemoryVectorIndex, VectorIndex},
};
use crate::config::IndexConfig;
use crate::config::StorageConfig;

pub fn open(
    index: &IndexConfig,
    storage: &StorageConfig,
    embedding: &Arc<dyn EmbeddingProvider>,
) -> anyhow::Result<(Arc<dyn StorageEngine>, Arc<dyn VectorIndex>)> {
    match index.vector_backend.as_str() {
        "edgestore-hnsw" | "edgestore" => {
            let db_path = &storage.path;
            let vec_path = db_path.with_extension("vec");
            tracing::info!("storage: EdgeStore at {:?}", db_path);
            let store = Arc::new(
                EdgeStoreStorage::open(db_path).map_err(|e| anyhow::anyhow!("failed to open EdgeStore storage: {}", e))?,
            );
            let vidx = Arc::new(
                EdgeStoreVectorIndex::open(vec_path, Some(embedding.model_id().to_string()), Some(embedding.dims()))
                    .map_err(|e| anyhow::anyhow!("failed to open EdgeStore vector index: {}", e))?,
            );
            Ok((store, vidx))
        }
        _ => {
            tracing::info!("storage: in-memory (set index.vector_backend = \"edgestore-hnsw\" for persistence)");
            let store = Arc::new(MemoryStorage::new());
            let vidx = Arc::new(MemoryVectorIndex::new(
                Some(embedding.model_id().to_string()),
                Some(embedding.dims()),
            ));
            Ok((store, vidx))
        }
    }
}
