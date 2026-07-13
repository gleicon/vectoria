# Vectoria — Backlog

Priority: P0 (blocker) → P1 (v1 required) → P2 (v1 nice-to-have) → P3 (Phase 2)

---

## P0 — Foundation

- [x] Rust workspace init (`vectoria-core`, `vectoria-server`, `vectoria-cli`)
- [x] `StorageEngine` trait (put/get/delete/range/prefix/tx)
- [x] EdgeStore `StorageEngine` implementation
- [ ] SQLite `StorageEngine` fallback implementation
- [x] `VectorIndex` trait (insert/delete/search/persist)
- [x] EdgeStore HNSW `VectorIndex` implementation
- [ ] TurboVec `VectorIndex` implementation (stub, swap-in later)
- [x] `EmbeddingProvider` trait (embed_text, embed_batch, model_id, dimensions)
- [x] Local ONNX `EmbeddingProvider` (fastembed-rs, multilingual-e5-small default)
- [x] OpenAI-compatible `EmbeddingProvider` (Ollama, llama.cpp, vLLM)
- [x] First-run consent flow (model download prompt, --skip-consent, --skip-model-download)
- [x] Model ID + dimensions stored per vector (detect incompatibility on startup)
- [x] Product schema (`id`, `text`, `vector`, `metadata`, `model_id`, `dims`, `status`)
- [x] `POST /products` — index product (text or pre-computed vector)
- [x] `PUT /products/{id}` — update product
- [x] `DELETE /products/{id}` — remove product

---

## P0 — Search

- [x] `POST /search` — hybrid mode (BM25 + vector), returns hits
- [x] Semantic-only mode (`"mode": "semantic"`)
- [x] BM25-only mode (`"mode": "bm25"`)
- [x] Hybrid mode as default (`"mode": "hybrid"`)
- [x] `limit` + `offset` pagination
- [x] Metadata filters (`"filters": { "brand": "Nike", "price_max": 150 }`)
- [x] `GET /autocomplete?q=...` — BM25 prefix search, <10ms target

---

## P1 — Search Quality

- [x] SymSpell integration (catalog-seeded, updates on product index)
- [x] Embedding-based query expansion (nearest categories/brands in vector space)
- [x] Facet aggregations (`"aggregate": ["brand", "category"]`)
- [x] Cross-encoder reranking (opt-in `"rerank": true`, ms-marco-MiniLM)
- [x] Explainability (opt-in `"explain": true`, score breakdown per hit)
- [x] `GET /products/{id}/similar` — similar by stored product ID
- [x] `POST /products/similar` — similar by text, vector, or product ID
- [x] Ranking weights: semantic + popularity + availability + margin (config + per-request override)
- [x] Structured product embedding (title + brand + category + attributes → single text for embed)

---

## P1 — API & Auth

- [x] API key auth on all endpoints (auto-generated on first run)
- [x] `vectoria.toml` config file with env var overrides (`VECTORIA_*`)
- [x] `GET /health` — liveness
- [x] `GET /stats` — index size, query count, latency P95
- [x] `POST /admin/reindex` — rebuild vector index from storage (recovery)
- [x] `POST /events` — fire-and-forget behavioral events (click, purchase, view, etc.)
- [x] Background aggregation job (popularity/conversion signals from events)

---

## P1 — Multi-Index REST API

- [x] `POST /1/indexes/{indexName}/query` — multi-index search
- [x] `POST /1/indexes/{indexName}` — index object
- [x] `PUT /1/indexes/{indexName}/{objectID}` — update object
- [x] `DELETE /1/indexes/{indexName}/{objectID}` — delete object
- [x] `POST /1/indexes/{indexName}/batch` — batch ops
- [x] Standard response shape (`hits`, `nbHits`, `page`, `hitsPerPage`, `processingTimeMS`)
- [x] IndexName → EdgeStore namespace mapping

---

## P1 — CLI

- [x] `vectoria import <file>` — bulk import (NDJSON, CSV, Parquet)
- [x] `vectoria reindex --model <name>` — re-embed all products with new model
- [x] `vectoria eval <judged.ndjson>` — Recall@K, NDCG@K, MRR, Coverage

---

## P1 — Caching

- [x] Foyer cache layer on `StorageEngine` (query embedding cache, hot metadata)
- [x] Head query result cache (configurable TTL)

---

## P2 — Distribution

- [x] Dockerfile (`vectoria-full`, `vectoria-slim`)
- [x] GitHub Actions CI (build, test, release binaries)
- [x] `cargo-dist` for automated binary releases
- [x] Homebrew formula

---

## P2 — Benchmark (pre-launch blocker)

- [ ] Amazon ESCI dataset loader
- [x] BM25 baseline runner
- [x] Vectoria benchmark runner (semantic, hybrid, hybrid+rerank)
- [x] Results: Recall@10, NDCG@10, MRR, Coverage, zero-result rate
- [x] `examples/webstore/` demo (side-by-side vs keyword search)

---

## P3 — Phase 2

- [x] LLM provider abstraction (`[llm]` config, openai-compatible)
- [x] LLM-based query rewriting (zero-result fallback after spell correction)
- [x] User embeddings (built from click/purchase events, cached in NS_USERS)
- [x] `GET /users/{id}/recommendations`
- [x] Semantic result clustering (`cluster: true` in SearchRequest, k-means on stored vectors)
- [x] Multi-tenancy (`[[tenants]]` config, per-tenant API keys, named index scoping, auth bypass fix)
- [ ] Product relationship graph (product→product, brand→product)
- [ ] Two-tower retrieval model
- [ ] Server-side WASI build target (edge compute)
