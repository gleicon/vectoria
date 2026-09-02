# Changelog

All notable changes to Vectoria. Follows [Keep a Changelog](https://keepachangelog.com/).

---

## [0.1.18] — 2026-09-01

### Added
- **EdgeStore 1.7 upgrade**: bumped `edgestore` and `edgestore-repl` from `1.5` to `1.7`.
- **BM25 snippet extraction** (`"snippets": true` in `SearchRequest`): each `Hit` gains an optional `snippets: Vec<String>` field containing context windows around matched query terms. Uses edgestore's `search_text_with_snippets` (ENG-11). Requires re-indexing after upgrade to populate position data (v3 index format); documents indexed under v1/v2 still appear in results with empty snippets. Mutually exclusive with `scan_stats` — enabling snippets disables I/O accounting for that request.
- **`StorageEngine::search_text_with_snippets` trait method**: default implementation delegates to `search_text` with empty snippet lists. `EdgeStoreStorage` overrides to call edgestore's native snippet API.
- **Product count cache in `EdgeStoreStorage`**: `total_indexed` in `BM25ScanStats` was previously computed by scanning all product keys on every search query (O(n) I/O). Now cached with a 5-second TTL, recomputed at most once per 5 seconds regardless of query volume.

### Changed
- **`SearchRequest`**: new `snippets: bool` field (default `false`). Requests with `snippets: true` are excluded from the query result cache.
- **`Hit`**: new `snippets: Option<Vec<String>>` field, omitted from JSON when `None`. Present on BM25 hits when `snippets: true`; `None` on vector-only hits and when `snippets: false`.

---

## [0.1.17] — 2026-07-25

### Added
- **EdgeStore 1.5 upgrade**: bumped `edgestore` and `edgestore-repl` from `1.0` to `1.5`. Leverages the new O(n) text indexing path (100K docs: 134s → 1.5s; 1M docs: never completes → ~18.5s).
- **`BM25ScanStats` in `SearchResponse`**: new `scan_stats` field (omitted when `None`) carries `bytes_scanned`, `items_examined`, and `segments_scanned` directly from the EdgeStore text index. Only populated by `EdgeStoreStorage`; `None` for the memory backend and when facet filters are active.
- **`StorageEngine::search_text_with_stats` trait method**: default implementation delegates to `search_text` and returns `None` stats. `EdgeStoreStorage` overrides it with edgestore's native `search_text_with_stats` API when no facet filters are present; falls back to `search_text_with_options` (returns `None` stats) when filters require the options path.
- **Quality panel catalog-change diagnostics** (`examples/saas-console/tenant.html`): three new `scan_stats`-driven issue types replace approximate per-hit heuristics with ground-truth index accounting:
  - `bytes_scanned = 0` → "Text index not built — run reindex from admin panel."
  - `bytes_scanned > 0 && items_examined = 0` → confirmed vocabulary gap: the query term is absent from every product; shows actual index size.
  - `bytes_scanned > 0 && items_examined > 0 && low bm25Rate` → keyword matches exist but rank low; suggests description enrichment.
  - Stats line now includes `index <size> · <N> kw hits` when `scan_stats` is present.

---

## [0.1.16] — 2026-07-25

### Added
- **Multi-tenant SaaS routes**: `POST /admin/tenants`, `DELETE /admin/tenants/{name}`, `POST /admin/tenants/{name}/rotate-key`, `GET /admin/tenants` — atomically create an isolated index + scoped API key per tenant, persisted to disk.
- **Admin override routes per named index**: pins, sponsored slots, and suppressions are now accessible at `/indexes/{name}/admin/*` so tenant keys can manage their own overrides without admin privileges.
- **`POST /indexes/{name}/admin/reindex`**: re-embeds all products in a named index using `spawn_blocking`, keeping the async executor free for concurrent search queries during the operation.

### Fixed
- **`TenantStore::delete()` race condition**: the old implementation read `by_name` under a read lock, dropped it, then acquired `by_key.write()` and `by_name.write()` as separate operations. Two concurrent deletes of the same name could both succeed, and between the two write operations the maps were transiently inconsistent (`lookup_key()` returning `None` while `exists()` returned `true`). Fixed by acquiring both write locks atomically at the start in consistent order (`by_key` before `by_name`).
- **`TenantStore::create()` rollback inconsistency**: on `save()` failure the rollback acquired `by_key.write()` and `by_name.write()` separately, creating the same inconsistency window. Fixed by holding both write locks for the duration of the rollback.
- **LLM rewriter guard pattern**: `self.llm_rewriter.is_some() { ... .unwrap() }` replaced with `if let Some(rewriter) = &self.llm_rewriter`, eliminating the redundant guard + infallible unwrap pair.

### Quality assessment (saas-console)
- **Model-agnostic coverage metrics**: replaced the absolute cosine-threshold heuristic with a score-gap + BM25-presence-rate model. Score gap `(maxScore − medianScore) / maxScore` measures result differentiation without depending on embedding scale; BM25 presence rate measures vocabulary coverage. Low gap + high BM25 = broad query (informational, not a catalog gap); low gap + low BM25 = catalog gap; either low alone = soft warning.
- **Spell-correction transparency**: when `query_context.spell_corrected = true`, the panel shows `"beatles" → "bates"` with a warning explaining results are for the corrected query, not the original.
- **Generic query detection**: `"shoes"` with 29 hits no longer fires "Coverage: Poor" — high BM25 rate with a narrow score gap is correctly identified as a broad query with many relevant results.

---

## [0.1.15] — 2026-07-19

### Fixed
- **BM25 single-result bug**: `SearchMode::Bm25` returned only 1 result for queries like "shoes" while hybrid/semantic returned 15+. Root cause: `expand_query_terms()` was gated on `!candidate_scores.is_empty()`, but in BM25-only mode no semantic search runs so `candidate_scores` was always empty at expansion time. Fix: pre-seed `candidate_scores` from the sparse BM25 hits before the expansion check so term enrichment fires correctly in BM25-only mode.
- **TOCTOU race in `TenantStore::create()`**: `exists()` released its read lock before `create()` acquired write locks, allowing two concurrent creates with the same name to both succeed. Fixed by checking inside the write lock. Also enforced consistent lock ordering (`by_key` before `by_name`) to match `rotate_key()` and prevent deadlock.

### Security
- **Admin key leak**: removed `eprintln!("api_key: {}", api_key)` from server startup — admin key was printed to Docker container logs in plaintext.
- **CSP header** on `platform.vectoriasearch.com`: `default-src 'self'; script-src 'self' 'unsafe-inline'; connect-src *; frame-ancestors 'none'`. Blocks clickjacking and external script injection. `connect-src *` required because the console connects to user-configured server URLs.

### Console (examples/saas-console)
- **Login session fix**: when `VECTORIA_DEFAULT_URL` is set (production platform), saved session data is no longer restored on the login page. Prevents stale `localhost` URLs from prior dev sessions silently blocking HTTPS platform users with a mixed-content error.
- **Tenant-detail load-data panel**: collapsible "load data ▾" per index card. Includes upsert behavior note, which metadata fields are actually indexed for search vs stored-only, single-product curl example, and bash loop for bulk JSONL loading. No Makefile dependency — customer-facing only.
- **JS/CSS cache fix**: changed nginx `Cache-Control` for platform console JS/CSS from `public, immutable` (7-day no-revalidation) to `no-cache, must-revalidate`. Stale `nav.js` was silently blocking button event listeners after deploys.

### Docs
- **Product schema section** in `docs/api.md`: documents `text` field vs metadata field indexing fallback, exactly which 5 metadata fields (`title`, `name`, `brand`, `category`, `description`) + `attributes.*` are indexed for BM25/vector, and that all other fields are stored-only and filter-only. Special fields table: `margin`, `in_stock`, `price`.
- **Tenant namespace isolation** clarified in `docs/api.md`: tenant key scopes URL index name automatically (`{tenant}/{index}` internally); cross-namespace access returns 404 not 403 (avoids namespace enumeration).
- **`examples/saas-console/NOTES.md`** rewritten: full security model, session management, CSP rationale, deploy wiring, split-to-own-repo guide, production hardening checklist.

### Deploy
- Added `VECTORIA_RATE_LIMIT_PER_SECOND: ${VECTORIA_RATE_LIMIT_PER_SECOND:-100}` default to `deploy/docker-compose.prod.yml`.

---

## [0.1.14] — 2026-07-15

### Added
- **Search widget** (`search-widget/vectoria-search.js`): zero-dependency UMD JavaScript widget for embedding Vectoria search in any web page or framework. Client-side synonym expansion and Unicode normalization for `en-US` and `pt-BR` with no extra roundtrips. Works as a Web Component (`<vectoria-search>`), ES module, and plain `<script>` tag. Configurable via attributes or `VectoriaSearch.init()`. Events: `vs-results`, `vs-select`, `vs-error`. Published at `vectoriasearch.com/search-widget/vectoria-search.js`.
- **Widget live demo** at `vectoriasearch.com/search-widget.html`: enhancement inspector showing original vs enhanced query, code panels for HTML / Web Component / React / Vue.
- **`/autocomplete` proxied** in nginx `demo.vectoriasearch.com`: route was missing from the API location regex, causing 404 on OPTIONS preflight. Also added `users` and `admin` to the proxy block to cover all server routes.

### Fixed
- **CORS double-header**: nginx was adding `Access-Control-Allow-Origin: https://vectoriasearch.com` while `CorsLayer::permissive()` in Axum already returns `*`. Browsers reject responses with duplicate ACAO headers. Removed the nginx CORS layer; Axum handles it.

### Docs
- Rustdoc added to `RelationType::as_str/from_str`, `SearchEngine::with_query_embedder`, `aggregate_once_for_test`, and all `WasmConfig` / `WasmProduct` / `WasmSearchRequest` fields.
- README: version references updated to 0.1.14, search widget section added.

---

## [0.1.13] — 2026-07-13

### Added
- **Product relationship graph**: `GET /products/{id}/related?type=brand|co_purchased&limit=N`. Brand relations (same `metadata.brand`) and co-purchased relations (shared user click/purchase history) populated by the aggregation loop. Storage: `NS_RELATIONS` EdgeStore namespace, key `{from}\x00{rel_type}\x00{to}`.
- **Two-tower retrieval**: optional `[query_embedding]` config block (`provider`, `model`, `base_url`, `api_key`, `dims`). When set, query text is embedded with this provider; products keep the main `[embedding]` provider. Enables asymmetric retrieval (e.g. fine-tuned query tower + large product tower). `SearchEngineBuilder::with_query_embedder()`.
- **WASM / edge build target** (`vectoria-wasm` crate, `wasm32-unknown-unknown`): in-memory BM25 + brute-force cosine + OpenAI-compatible remote embedding via JS fetch. `VectoriaWasm::new(config_json)`, `.index(product_json)`, `.search(request_json)`. Deploy on Cloudflare Workers, Deno Deploy, or in browsers. Build with `make wasm-pack` (requires `wasm-pack`).
- `aggregate_once_for_test()` public export for integration tests that need to trigger an aggregation cycle.

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
