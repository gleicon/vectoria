# Changelog

All notable changes to Vectoria. Follows [Keep a Changelog](https://keepachangelog.com/).

---

## [0.1.12] — 2026-07-13

### Added
- **User recommendations** (`GET /users/{id}/recommendations?limit=N`): user vectors built from click/purchase embeddings, cached after first call; unknown users return empty.
- **LLM query rewriting**: optional OpenAI-compatible endpoint (`[llm]` config block); fires only when BM25 is sparse and spell correction didn't trigger; fails silently to original query.
- **Semantic clustering**: `SearchRequest.cluster: bool` (default `false`) groups hits by k-means on stored vectors; response includes labelled `clusters` array.
- **Multi-tenancy**: `[[tenants]]` config block maps per-tenant API keys to named index namespaces.
- `NS_USERS` / `NS_USER_EVENTS` EdgeStore namespaces; click/purchase events dual-written for O(user\_events) lookup.
- `aggregate_user_vectors()` in the aggregation loop recomputes and caches user vectors.

### Security
- **Auth bypass fix** (CRITICAL): tenant API keys previously had unrestricted route access. Introduced `Principal` enum (`Admin` / `Tenant(name)`) set by `require_api_key` middleware; `require_admin` gates all admin routes; named-index handlers enforce namespace match.
- Tenant recommendation requests are scoped to the tenant's named index, preventing cross-tenant user data exposure.

---

## [0.1.11] — 2026-07-07

### Changed
- **Single-engine architecture**: `EdgeStoreStorage` and `EdgeStoreVectorIndex` now share one EdgeStore engine at one path instead of opening separate engines at `vectoria.db` and `vectoria.vec`. New constructor: `from_engine(Arc<Mutex<Engine>>)` on both types. Old `open(path)` constructors still present for simple cases.
- Default `VECTORIA_STORAGE_PATH` changed from `./vectoria.db` to `./vectoria` (directory, no extension). **Migration**: existing data at `vectoria.db`/`vectoria.vec` will not be read; re-index after upgrading.
- `vector_count` in stats now reads directly from the in-memory HNSW index via `Engine::vector_count()` instead of an `AtomicU64` counter that reset to 0 on restart.
- Docker server image no longer includes the `vectoria` CLI binary. This removes `parquet` and `arrow` from the server build, cutting Docker build time from ~25 min to ~8 min. Build the CLI separately with `cargo build --release -p vectoria-cli`.

### Added
- **Replication support** via `edgestore-repl`:
  - `VECTORIA_REPL_BIND=0.0.0.0:8900` — start as a replication primary (writable, serves pull endpoint)
  - `VECTORIA_REPL_PRIMARY_URL=http://host:8900` — start as a replica (read-only, pulls from primary)
  - Anti-entropy loop syncs replicas within one flush interval (~60s) via Merkle delta comparison
  - Replica engine is opened read-only; accidental write attempts return an error
- `Engine::open_readonly` enforced on replicas via `ReplicatedEngine::open_replica`

### Fixed
- `vector_count: 0` in `/stats` after server restart (was `AtomicU64` starting at 0)

---

## [0.1.10] — 2026-06-30

### Added
- **Faceted pre-filtering**: metadata fields are indexed as EdgeStore facets at `POST /products` time and applied before BM25/vector retrieval (not post-filter). Supported scalar types: string, bool, integer. Price range (`price_min`, `price_max`) remains a post-filter.
- EdgeStore upgraded to 1.2.0: persistent BM25 inverted index, atomic CTR increments in a single transaction, warm HNSW preload at startup.
- `bm25_document_count` added to `/stats` response.

### Fixed
- `StorageStats` marked `#[non_exhaustive]`; downstream embedders no longer break on new fields.
- Autocomplete (`GET /autocomplete`) broken after EdgeStore migration — restored.

---

## [0.1.9] — 2026-06-16

### Added
- `VECTORIA_VECTOR_BACKEND` env var (`memory` or `edgestore-hnsw`) — overrides `[index] vector_backend` without editing `vectoria.toml`. Default stays `memory`.
- EdgeStore HNSW persistence: vector graph survives restart when `vector_backend = "edgestore-hnsw"`.

---

## [0.1.8] — 2026-06-14

### Added
- `vectoria-algolia` Algolia-compatible adapter demo at `a.vectoriasearch.com`.

---

## [0.1.6] — 2026-06-05

### Changed
- SQLite storage backend removed. All persistence now uses EdgeStore (KV + BM25 + HNSW in one engine).

### Added
- Query-CTR signal stored in EdgeStore atomically alongside click/purchase events.

---

## [0.1.5] — 2026-05-28

### Added
- Score explainability: pass `"explain": true` in search requests to get per-factor score breakdown in each hit (`bm25`, `semantic_similarity`, `popularity`, `query_ctr`, `availability`, `margin`, `match_sources`, `query_context`).

---

## [0.1.4] and earlier

Initial releases: BM25 + vector hybrid search, behavioral ranking (clicks/purchases/views), multi-index namespaces, spell-correction fallback, CLI import (NDJSON/CSV/Parquet), ESCI and WANDS benchmark harness, embedded Rust library (`vectoria-core`).
