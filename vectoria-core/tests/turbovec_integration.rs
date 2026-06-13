/// Integration tests for TurboVecIndex — ephemeral and file-persisted paths.
use vectoria_core::vector::{VectorIndex, turbovec::TurboVecIndex};
use tempfile::TempDir;

fn unit_vec(dim: usize, hot: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    v[hot % dim] = 1.0;
    v
}

#[tokio::test]
async fn test_ephemeral_upsert_and_search() {
    let idx = TurboVecIndex::new(Some("stub-4".into()), Some(4));

    idx.upsert("a", &unit_vec(4, 0)).await.unwrap();
    idx.upsert("b", &unit_vec(4, 1)).await.unwrap();
    idx.upsert("c", &unit_vec(4, 2)).await.unwrap();

    let results = idx.search(&unit_vec(4, 0), 3).await.unwrap();
    assert_eq!(results[0].0, "a", "nearest neighbor of unit_vec(0) must be 'a'");
    assert!((results[0].1 - 1.0).abs() < 1e-5, "cosine score for identical vector must be ~1.0");
}

#[tokio::test]
async fn test_search_top_k_limit() {
    let idx = TurboVecIndex::new(None, None);
    for i in 0..10u32 {
        idx.upsert(&format!("v{}", i), &[i as f32, 0.0, 0.0]).await.unwrap();
    }

    let results = idx.search(&[1.0, 0.0, 0.0], 3).await.unwrap();
    assert_eq!(results.len(), 3, "top_k=3 must return exactly 3 results");
}

#[tokio::test]
async fn test_delete_removes_from_search() {
    let idx = TurboVecIndex::new(None, None);
    idx.upsert("keep", &[1.0, 0.0]).await.unwrap();
    idx.upsert("gone", &[0.0, 1.0]).await.unwrap();

    idx.delete("gone").await.unwrap();

    let results = idx.search(&[0.0, 1.0], 5).await.unwrap();
    assert!(!results.iter().any(|(id, _)| id == "gone"), "deleted vector must not appear");
}

#[tokio::test]
async fn test_zero_vector_score() {
    let idx = TurboVecIndex::new(None, None);
    idx.upsert("zero", &[0.0, 0.0, 0.0]).await.unwrap();

    let results = idx.search(&[1.0, 0.0, 0.0], 1).await.unwrap();
    assert!((results[0].1).abs() < 1e-5, "zero vector scores 0.0 against any query");
}

#[tokio::test]
async fn test_stats_ephemeral() {
    let idx = TurboVecIndex::new(Some("m".into()), Some(3));
    idx.upsert("x", &[1.0, 2.0, 3.0]).await.unwrap();
    idx.upsert("y", &[4.0, 5.0, 6.0]).await.unwrap();

    let stats = idx.stats().await.unwrap();
    assert_eq!(stats.vector_count, 2);
    assert!(stats.index_bytes > 0);
}

#[tokio::test]
async fn test_model_id_and_dims_accessors() {
    let idx = TurboVecIndex::new(Some("my-model".into()), Some(128));
    assert_eq!(idx.model_id(), Some("my-model"));
    assert_eq!(idx.dims(), Some(128));
}

#[tokio::test]
async fn test_flush_no_op_on_ephemeral() {
    let idx = TurboVecIndex::new(None, None);
    idx.upsert("x", &[1.0]).await.unwrap();
    idx.flush().await.unwrap();
}

#[tokio::test]
async fn test_file_persisted_open_and_flush() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("vectors.json");

    {
        let idx = TurboVecIndex::open(&path, Some("stub-4".into()), Some(4)).unwrap();
        idx.upsert("r1", &unit_vec(4, 0)).await.unwrap();
        idx.upsert("r2", &unit_vec(4, 1)).await.unwrap();
        idx.flush().await.unwrap();
    }

    let idx2 = TurboVecIndex::open(&path, Some("stub-4".into()), Some(4)).unwrap();
    let results = idx2.search(&unit_vec(4, 0), 2).await.unwrap();
    assert!(results.iter().any(|(id, _)| id == "r1"), "r1 must survive flush+reopen");

    let stats = idx2.stats().await.unwrap();
    assert_eq!(stats.vector_count, 2, "both vectors must be loaded after reopen");
}

#[tokio::test]
async fn test_atomic_flush_no_partial_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("atomic.json");

    let idx = TurboVecIndex::open(&path, None, None).unwrap();
    idx.upsert("a", &[1.0, 2.0]).await.unwrap();
    idx.flush().await.unwrap();

    let tmp = path.with_extension("tmp");
    assert!(!tmp.exists(), "tmp file must be cleaned up after atomic rename");
    assert!(path.exists(), "snapshot file must exist after flush");
}

#[tokio::test]
async fn test_overwrite_existing_vector() {
    let idx = TurboVecIndex::new(None, None);
    idx.upsert("ov", &[1.0, 0.0]).await.unwrap();
    idx.upsert("ov", &[0.0, 1.0]).await.unwrap();

    let results = idx.search(&[0.0, 1.0], 1).await.unwrap();
    assert_eq!(results[0].0, "ov");
    assert!((results[0].1 - 1.0).abs() < 1e-5, "updated vector must score 1.0 vs identical query");

    let stats = idx.stats().await.unwrap();
    assert_eq!(stats.vector_count, 1, "upsert must not duplicate entries");
}

#[tokio::test]
async fn test_empty_index_search() {
    let idx = TurboVecIndex::new(None, None);
    let results = idx.search(&[1.0, 0.0], 5).await.unwrap();
    assert!(results.is_empty(), "empty index must return no results");
}
