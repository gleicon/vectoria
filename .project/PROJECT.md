# Vectoria

## Overview

Vectoria is an AI-native embedded ecommerce search engine written in Rust, following the "SQLite philosophy": single binary, local-first, zero external runtime dependencies. It ships hybrid BM25 + vector search, spell correction, pseudo-relevance feedback query expansion, and optional cross-encoder reranking — all in-process via EdgeStore (embedded KV + HNSW). Designed for indie developers and small teams who want production-quality semantic search without cloud search services. Target audience: developers embedding search into apps, hobbyists, self-hosters.

## Stack

| Concern | Dependency | Version |
|---|---|---|
| Language / edition | Rust 2024 | — |
| Async runtime | tokio (full) | 1 |
| HTTP server | axum + tower-http (cors, trace) | 0.8 / 0.6 |
| Embedded storage + vector index + BM25 | edgestore | 1.0 |
| Local embeddings (ONNX) | fastembed | 4 |
| BM25 scoring | bm25 | 2 |
| In-memory LRU cache | foyer | 0.22 |
| Spell correction | symspell | 0.5 |
| Bulk import | parquet + arrow | 54 |
| CLI | clap (derive) | 4 |
| Serialization | serde + serde_json + toml | — |
| Config | config + dotenvy | 0.14 / 0.15 |
| HTTP client (OpenAI embedding) | reqwest (json) | 0.12 |
| Metrics | prometheus | 0.13 |
| Error handling | anyhow + thiserror | 1 / 2 |
| UUID | uuid (v4) | 1 |
| Time | chrono (serde) | 0.4 |
| Build / dist | cargo-dist 0.22.1, 4 targets, Homebrew tap | — |

Build: `cargo build --release`  
Test: `cargo test` (no mocks — integration tests use real storage: `tempfile::TempDir` for EdgeStore, `open_in_memory()` for SQLite, tmp files for TurboVec)  
Docker: multi-stage `vectoria-full` (ONNX) + `vectoria-slim` (OpenAI-only)

## Repo Map

| Path | Purpose |
|---|---|
| `vectoria-core/` | Library crate: all domain logic |
| `vectoria-core/src/model/` | Shared types: `Product`, `SearchRequest/Response`, `SearchMode`, `ProductSignals`, `RankingWeights` |
| `vectoria-core/src/storage/` | `StorageEngine` trait; impls: `EdgeStoreStorage`, `SqliteStorage`, `MemoryStorage` |
| `vectoria-core/src/vector/` | `VectorIndex` trait; impls: `EdgeStoreVectorIndex`, `TurboVecIndex` (file-persisted flat index), `MemoryVectorIndex` |
| `vectoria-core/src/embedding/` | `EmbeddingProvider` trait; impls: `LocalEmbedding` (fastembed), `OpenAIEmbedding`, `CachedEmbedding` (foyer LRU) |
| `vectoria-core/src/search/mod.rs` | `SearchEngine`: hybrid retrieval, SymSpell, PRF expansion, reranking, `QueryResultCache` |
| `vectoria-core/src/search/query_cache.rs` | TTL-bounded `QueryResultCache` (RwLock HashMap, lazy eviction) |
| `vectoria-core/src/aggregation/` | Background loop pre-computing `ProductSignals` from events into EdgeStore NS_SIGNALS |
| `vectoria-server/` | Binary: HTTP server |
| `vectoria-server/src/main.rs` | Entry point: config, embedding init, engine init, aggregation spawn, router mount |
| `vectoria-server/src/config.rs` | `ServerConfig` / `IndexConfig` (toml + env); embedding cache, query cache, aggregation interval |
| `vectoria-server/src/state.rs` | `AppState { engine, index_registry, api_key }` |
| `vectoria-server/src/index_registry.rs` | Lazy-init per-indexName `SearchEngine` pool (`RwLock<HashMap<…>>`, double-checked locking) |
| `vectoria-server/src/auth.rs` | Bearer token middleware |
| `vectoria-server/src/routes/` | `search`, `products` (CRUD), `events`, `admin` (health/stats/reindex + multi-index REST API) |
| `vectoria-cli/` | Binary: CLI tool |
| `vectoria-cli/src/commands/` | `import`, `esci`, `bench`, `eval`, `reindex` subcommands |
| `examples/webstore/index.html` | Single-file SPA demo (vanilla JS, XSS-safe, hybrid/semantic/BM25 mode toggle) |
| `examples/webstore/setup.sh` | Downloads Amazon ESCI parquets to `~/.local/share/vectoria/esci/` and imports catalog |
| `Formula/vectoria.rb` | Homebrew formula (SHA256 placeholders — filled post-release by cargo-dist) |
| `Dockerfile` | Multi-stage: `vectoria-full` with ONNX model, `vectoria-slim` for OpenAI-only |
| `.github/workflows/` | `ci.yml` (matrix lint+test+docker), `release.yml` (cross-compile, GHCR push, GitHub release) |

## Constraints

- **No external runtime deps**: all functionality must work offline (ONNX local model). OpenAI embedding is optional override only.
- **No mocks in tests**: integration tests hit real EdgeStore via `tempfile::TempDir`. Test coverage must use live implementations.
- **Single-writer EdgeStore**: all EdgeStore calls go through `Arc<Mutex<Engine>>` wrapped in `tokio::task::spawn_blocking`.
- **Embedding model**: `multilingual-e5-small`, 384 dims. Model ID is stored per-document for future migration path.
- **Multi-index REST API**: five `/1/indexes/{indexName}/*` endpoints (search, index, update, delete, batch) provide multi-tenant index isolation via `IndexRegistry`.
- **Binary names**: `vectoria-server` (HTTP daemon) and `vectoria` (CLI) — both installed by Homebrew formula and cargo-dist.
- **Hybrid scoring formula**: `semantic * 0.7 + bm25 * 0.3` for retrieval; final score incorporates popularity, availability, and margin weights from `RankingWeights`.
- **vector_backend options**: `edgestore-hnsw` (default, persistent HNSW), `sqlite` (SQLite metadata + TurboVec vectors), `turbovec` (in-memory metadata + TurboVec vectors), `memory` (all in-memory, dev only).
- **Data files not in repo**: ESCI parquets download to `~/.local/share/vectoria/esci/` via `examples/webstore/setup.sh`. `vectoria.toml` is gitignored (contains API key).
