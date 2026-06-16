#[allow(dead_code)]
mod common;

use std::sync::{atomic::Ordering, Arc};
use vectoria_core::embedding::{cache::CachedEmbedding, EmbeddingProvider};

#[tokio::test]
async fn test_cache_hits_skip_inner_embed() {
    let stub = Arc::new(common::StubEmbedding::new(32));
    let calls = Arc::clone(&stub.calls);
    let inner: Arc<dyn EmbeddingProvider> = stub;
    let cached = CachedEmbedding::new(inner, 1000);

    let v1 = cached.embed("running shoes").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let v2 = cached.embed("running shoes").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "cache should short-circuit inner");

    assert_eq!(v1, v2);
}

#[tokio::test]
async fn test_cache_different_queries_both_embedded() {
    let stub = Arc::new(common::StubEmbedding::new(32));
    let calls = Arc::clone(&stub.calls);
    let inner: Arc<dyn EmbeddingProvider> = stub;
    let cached = CachedEmbedding::new(inner, 1000);

    cached.embed("query one").await.unwrap();
    cached.embed("query two").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2, "distinct queries must each call inner");

    cached.embed("query one").await.unwrap();
    cached.embed("query two").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2, "repeated queries must be cached");
}
