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
            let repl_bind    = std::env::var("VECTORIA_REPL_BIND").ok();
            let repl_primary = std::env::var("VECTORIA_REPL_PRIMARY_URL").ok();

            let engine = match (repl_bind, repl_primary) {
                (Some(bind_addr), _) => {
                    // Primary: start replication server on bind_addr.
                    tracing::info!("storage: EdgeStore primary at {:?}, repl on {}", db_path, bind_addr);
                    use edgestore_repl::ReplicatedEngine;
                    use edgestore::EdgestoreConfig;
                    let re = ReplicatedEngine::open_primary(
                        EdgestoreConfig::new(db_path),
                        &bind_addr,
                    ).map_err(|e| anyhow::anyhow!("failed to open EdgeStore primary: {}", e))?;
                    tracing::info!("replication server on port {}", re.bound_port().unwrap_or(0));
                    Arc::clone(re.engine())
                }
                (None, Some(primary_url)) => {
                    // Replica: pull from primary; engine is read-only.
                    tracing::info!("storage: EdgeStore replica at {:?}, primary {}", db_path, primary_url);
                    use edgestore_repl::ReplicatedEngine;
                    use edgestore::EdgestoreConfig;
                    let re = ReplicatedEngine::open_replica(
                        EdgestoreConfig::new(db_path),
                        &primary_url,
                    ).map_err(|e| anyhow::anyhow!("failed to open EdgeStore replica: {}", e))?;
                    Arc::clone(re.engine())
                }
                (None, None) => {
                    // Standalone: plain engine, no replication.
                    tracing::info!("storage: EdgeStore standalone at {:?}", db_path);
                    use edgestore::{EdgestoreConfig, Engine};
                    let engine = Engine::open(EdgestoreConfig::new(db_path))
                        .map_err(|e| anyhow::anyhow!("failed to open EdgeStore: {}", e))?;
                    Arc::new(std::sync::Mutex::new(engine))
                }
            };

            let store = Arc::new(EdgeStoreStorage::from_engine(Arc::clone(&engine)));
            let vidx = Arc::new(
                EdgeStoreVectorIndex::from_engine(
                    engine,
                    Some(embedding.model_id().to_string()),
                    Some(embedding.dims()),
                ).map_err(|e| anyhow::anyhow!("failed to init EdgeStore vector index: {}", e))?,
            );
            Ok((store, vidx))
        }
        _ => {
            tracing::info!("storage: in-memory (set VECTORIA_VECTOR_BACKEND=edgestore-hnsw for persistence)");
            let store = Arc::new(MemoryStorage::new());
            let vidx = Arc::new(MemoryVectorIndex::new(
                Some(embedding.model_id().to_string()),
                Some(embedding.dims()),
            ));
            Ok((store, vidx))
        }
    }
}
