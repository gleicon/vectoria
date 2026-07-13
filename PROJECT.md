# Vectoria — Project Status

**Current version**: 0.1.12  
**Stack**: Rust workspace — `vectoria-core` (library), `vectoria-server` (Axum HTTP), `vectoria-cli` (bulk import / eval)

---

## Architecture

```
vectoria-core/
  search/       — hybrid pipeline (BM25 + vector + CTR signals + spell + LLM rewrite + clustering)
  storage/      — StorageEngine trait; EdgeStore (default) and MemoryStorage (tests)
  embedding/    — EmbeddingProvider trait; local ONNX (fastembed), OpenAI-compatible, cached wrapper
  aggregation/  — background loop: product signals + user vectors
  model/        — shared types (Product, Hit, SearchRequest, SearchResponse, Event …)
  engine.rs     — SearchEngineBuilder / SearchEngineSync (sync wrapper for CLI)

vectoria-server/
  auth.rs       — Principal enum (Admin / Tenant); require_api_key + require_admin middleware
  config.rs     — vectoria.toml + VECTORIA_* env var overrides
  index_registry.rs — named index registry (max 100, per-tenant isolation)
  routes/       — products, search, events, admin, indexes, users
  state.rs      — AppState (registry, api_key, tenant_keys, limiter)
```

### Storage namespaces (EdgeStore)

| Namespace | Key | Value |
|---|---|---|
| `products` | product id | Product JSON |
| `events` | event id | Event JSON |
| `signals` | product id | ProductSignals JSON |
| `text` | product id | raw text (BM25) |
| `ctrs` | `query\x00product` | click+view counts |
| `users` | user id | `Vec<f32>` (user vector) |
| `userevents` | `user_id\x00event_id` | click/purchase event (dual-write) |

---

## Active features (v0.1.12)

- Hybrid search: BM25 + HNSW vector + CTR boost + ranking weights
- Spell correction (SymSpell, catalog-seeded, zero-result fallback only)
- LLM query rewriting (optional, OpenAI-compatible, fires when BM25 sparse)
- Semantic clustering (`cluster: true` → k-means on hit vectors, labelled clusters)
- User recommendations (`GET /users/{id}/recommendations`) from click/purchase history
- Cross-encoder reranking (opt-in `rerank: true`, ms-marco-MiniLM)
- Explainability (`explain: true`)
- Multi-tenancy: `[[tenants]]` in config; per-tenant API keys → named index scoping
- EdgeStore replication (`VECTORIA_REPL_BIND` / `VECTORIA_REPL_PRIMARY_URL`)
- Rate limiting, CORS, embedding cache, query result cache

---

## Auth model

All routes require `Authorization: Bearer <key>` or `X-Search-API-Key: <key>`.

| Route group | Who can access |
|---|---|
| `/products`, `/search`, `/events`, `/stats`, `/admin/*`, `/indexes` (create/list/delete) | Admin key only |
| `/indexes/{name}/products`, `/indexes/{name}/search`, `/indexes/{name}/similar` | Admin OR tenant key whose name matches `{name}` |
| `/users/{id}/recommendations` | Admin (default engine) OR tenant (their named index) |
| `/health` | Public |

---

## Config (`vectoria.toml`)

```toml
[server]
host = "0.0.0.0"
port = 8080

[embedding]
provider = "local"           # "local" | "openai-compatible"
model = "multilingual-e5-small"

[index]
vector_backend = "memory"    # "memory" | "edgestore-hnsw"
enable_reranker = false

[llm]
enabled = false
# base_url = "http://localhost:11434"
# model = "llama3"

[[tenants]]
name = "acme"
api_key = "sk-tenant-acme-..."
```

---

## Remaining P3 backlog

- Product relationship graph (product→product, brand→product)
- Two-tower retrieval model
- Server-side WASI build target (edge compute)

---

## Benchmark (ESCI, 5000 products)

| Mode | MRR | Notes |
|---|---|---|
| BM25 | 0.6595 | baseline |
| Hybrid | 0.6576 | BM25 + vector |
| Semantic | 0.5690 | vector only |

Reproduce: `make esci-import && make esci-judges && make bench`
