# Vectoria — Open Items

## Benchmark (pre-launch blocker)

Build reproducible benchmark before v1 public launch:

- Dataset: Amazon ESCI (parquets downloaded via `examples/webstore/setup.sh`)
- Baselines: BM25 (EdgeStore FTS5), BM25+boost, Meilisearch, Algolia
- Vectoria: semantic only, hybrid (BM25+vector), hybrid+reranking
- Metrics: Recall@10, NDCG@10, MRR, Coverage, zero-result rate
- Output: published numbers + reproducible script (`vectoria bench`)
- Goal: show long-tail recall improvement over BM25 with real data

Run eval:
```bash
vectoria esci products.parquet examples.parquet --judges judges.ndjson
vectoria bench judges.ndjson --server http://localhost:7700
```

## Signal accumulation consolidation

`record_event` increments `click_count`/`purchase_count`/`view_count` in three storage impls
(`EdgeStoreStorage`, `SqliteStorage`, `MemoryStorage`) with copy-pasted code.
Extract into a shared helper or move logic into the `StorageEngine` trait default.

## Done

- [x] Demo webstore (`examples/webstore/index.html`) — hybrid/semantic/BM25 toggle
- [x] Amazon ESCI loader (`vectoria esci`) with `--import` and `--judges` modes
- [x] ESCI setup script (`examples/webstore/setup.sh`) — downloads data, imports catalog
- [x] SQLite storage backend (`SqliteStorage`) + integration tests
- [x] TurboVec file-persisted vector index + integration tests
- [x] `cosine_similarity` deduplicated into `vector/mod.rs`
- [x] Query cache key fixed to include filters, aggregate, rerank fields
- [x] Binary rename: server is `vectoria-server`, CLI is `vectoria`
- [x] CORS enabled (`CorsLayer::permissive()`) for webstore cross-origin requests
