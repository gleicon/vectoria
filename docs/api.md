# Vectoria HTTP API

All endpoints except `/health` require `Authorization: Bearer <api_key>`.

The API key is printed on server startup and can be fixed in `vectoria.toml` under `[server] api_key`.

Base URL: `http://localhost:7700` by default.

---

## Health

```
GET /health
```

No authentication required.

Response:
```json
{"status": "ok", "version": "0.1.11"}
```

---

## Products

### Index a product

```
POST /products
```

Body:
```json
{
  "id": "p1",
  "text": "text used for embedding and BM25 indexing",
  "metadata": {
    "title": "...",
    "price": 99.99,
    "in_stock": true
  }
}
```

- `id` — required, must be unique within the index. Re-posting the same ID replaces (upserts) the product.
- `text` — the search corpus: used for BM25 full-text indexing and vector embedding. If omitted, the server derives it automatically from metadata (see below).
- `metadata` — arbitrary JSON object. Stored and returned in search hits. Some fields are also indexed for search.

#### How the search corpus is built

If `text` is present it is used as-is — metadata fields play no role in ranking.

If `text` is **omitted**, the server derives the corpus from these metadata fields in order:

| Field | Indexed for search |
|---|---|
| `title` | Yes — repeated by field weight (default 1×) |
| `name` | Yes |
| `brand` | Yes |
| `category` | Yes |
| `description` | Yes |
| `attributes` | Yes — each key/value pair appended as `"key: value"` |
| Everything else (`price`, `sku`, `color`, `in_stock`, …) | No — stored and returned, usable in `filters`, not searched |

Field weights (how many times a field's value repeats in the BM25 corpus) are configurable per index.

#### Special metadata fields

| Field | Effect |
|---|---|
| `margin` | Float 0.0–1.0. Feeds the `margin` ranking signal if `ranking_weights.margin > 0`. Absent = 0.0. |
| `in_stock` | Boolean. Can be used in `filters: {"in_stock": true}` to exclude out-of-stock products. |
| `price` | Numeric. Supports `filters: {"price_min": 10, "price_max": 100}` range filter. |

All other metadata fields are stored verbatim and returned in hits. They are filterable via exact-match in `filters` if indexed at search time.

Response: `201 Created`

### Update a product

```
PUT /products/{id}
```

Same body as POST. Replaces the product and re-embeds.

### Delete a product

```
DELETE /products/{id}
```

Response: `200 OK`

### Similar by ID

```
GET /products/{id}/similar
```

Returns up to 10 products with similar vectors to the given product. Limit is fixed at 10; use `POST /products/similar` to control it.

### Similar by vector or text

```
POST /products/similar
```

Body:
```json
{
  "text": "lightweight running shoe",
  "limit": 10
}
```

Or pass `"vector": [...]` directly to skip embedding.

---

## Search

```
POST /search
```

Body:
```json
{
  "q": "running shoes",
  "limit": 20,
  "offset": 0,
  "mode": "hybrid",
  "filters": {"in_stock": true},
  "aggregate": ["brand", "category"],
  "explain": false,
  "rerank": false
}
```

Fields:

- `q` — query string, required
- `limit` — results per page, default 20
- `offset` — pagination offset, default 0
- `mode` — `"hybrid"` (default), `"semantic"`, or `"bm25"`
- `filters` — key/value pairs matched against product metadata. Special keys: `price_min`, `price_max`
- `aggregate` — array of metadata field names to facet-count across all matched candidates
- `explain` — include per-result score breakdown
- `rerank` — apply cross-encoder reranking (requires `index.enable_reranker = true`)

Response:
```json
{
  "hits": [
    {
      "id": "p1",
      "score": 0.82,
      "metadata": {"title": "...", "price": 99.99},
      "explain": null
    }
  ],
  "total": 42,
  "offset": 0,
  "limit": 20,
  "processing_time_ms": 12,
  "query": "running shoes",
  "aggregations": {
    "brand": {"Nike": 12, "Adidas": 8},
    "category": {"Running Shoes": 15}
  }
}
```

`total` reflects matched candidates up to `(limit + offset) * 5` — not an exact index-wide count.

### Score explanation

Set `"explain": true` to include a full score breakdown on each hit.

The example below comes from a real test (`test_explain_score_breakdown_anatomy`): three products indexed,
three clicks recorded on `shoe1` for the query `"running shoe"`, then a hybrid search with `explain:true`.

**Why `shoe1` ranked first (score: 0.7112):**

```json
{
  "id": "shoe1",
  "score": 0.7112,
  "explain": {
    "match_sources": ["bm25", "vector"],
    "query_context": {
      "original_query": "running shoe",
      "effective_query": "running shoe",
      "spell_corrected": false,
      "query_expanded": false
    },
    "factors": [
      {"factor": "semantic_similarity", "score": 0.016, "weight": 0.70, "contribution": 0.011},
      {"factor": "bm25",               "score": 1.000, "weight": 0.30, "contribution": 0.300},
      {"factor": "popularity",         "score": 1.000, "weight": 0.20, "contribution": 0.200},
      {"factor": "query_ctr",          "score": 1.000, "weight": 0.15, "contribution": 0.150},
      {"factor": "availability",       "score": 1.000, "weight": 0.05, "contribution": 0.050},
      {"factor": "margin",             "score": 0.000, "weight": 0.05, "contribution": 0.000}
    ]
  }
}
```

Reading it: `bm25=1.0` means shoe1 was the top BM25 result for "running shoe" (score normalized to max).
`popularity=1.0` means it received the most clicks globally relative to views.
`query_ctr=1.0` means it's the only product clicked for this exact query (normalized to max=1.0 across candidates).
`semantic_similarity=0.016` is low here — this test uses a hash-based stub embedder; with a real model (multilingual-e5-small) semantic scores are typically 0.7–0.95 for relevant products.

**Why `shoe2` ranked second (score: 0.1780):**

```json
{
  "id": "shoe2",
  "score": 0.1780,
  "explain": {
    "match_sources": ["bm25", "vector"],
    "factors": [
      {"factor": "bm25",        "score": 0.363, "weight": 0.30, "contribution": 0.109},
      {"factor": "query_ctr",   "score": 0.000, "weight": 0.15, "contribution": 0.000},
      {"factor": "availability","score": 1.000, "weight": 0.05, "contribution": 0.050},
      ...
    ]
  }
}
```

`bm25=0.363`: partial match ("running" appears, but not "shoe"). `query_ctr=0` and `popularity=0`: never clicked.

**Why `mat1` ranked last (score: 0.0566):**

`match_sources: ["vector"]` — only returned via semantic search, no BM25 match. Score is almost entirely `availability` (0.05) plus a tiny semantic contribution.

---

| Field | Description |
|-------|-------------|
| `match_sources` | How this product entered the candidate set: subset of `["bm25", "vector"]` |
| `query_context.original_query` | Query as submitted |
| `query_context.effective_query` | Query actually used for BM25 (differs if spell-corrected or expanded) |
| `query_context.spell_corrected` | `true` if BM25 returned no results and a corrected query was used |
| `query_context.query_expanded` | `true` if semantically-similar terms were appended to improve recall |
| `factors[].score` | Raw signal value (0.0–1.0) |
| `factors[].weight` | Configured weight for this factor |
| `factors[].contribution` | `score × weight` — actual score contribution |

`sum(contribution)` equals `hit.score`. To diagnose a ranking:

1. Check `match_sources` — if `bm25` is absent, the product wasn't a text match (only semantic retrieval).
2. Check `query_ctr` contribution — if it dominates, ranking is driven by past click behavior on this query.
3. Check `bm25` contribution — if low, the product doesn't contain the query terms verbatim.
4. Check `query_context.spell_corrected` — if `true`, BM25 found no results for the original query.
5. Check `semantic_similarity` contribution — if the only non-zero signal, the product is semantically related but not a keyword match.

### Autocomplete

```
GET /autocomplete?q=runn&limit=5
```

Returns suggested query completions based on indexed text.

---

## Events

Record behavioral signals to improve ranking. Two distinct signals are derived from events:

| Signal | How computed | Ranking effect |
|--------|-------------|----------------|
| **Global popularity** | click\_count / view\_count per product (all queries) | Products with high overall CTR rank slightly higher everywhere |
| **Query CTR** | click + purchase count per (query, product) pair | Products clicked for *this exact query* rank higher for future searches of the same query |

Query CTR is the stronger signal — it captures "users who searched this chose that", which BM25 and vectors cannot. Global popularity captures general demand.

```
POST /events
```

Body:
```json
{
  "product_id": "sku-001",
  "type": "click",
  "query": "waterproof trail shoes",
  "user_id": "u-abc",
  "session_id": "sess-xyz"
}
```

| Field | Description |
|-------|-------------|
| `product_id` | Required. Product that was interacted with. |
| `type` | `view`, `click`, `add_to_cart`, `wishlist`, `purchase` |
| `query` | The search query that led to this product. **Required for query-CTR to work.** |
| `user_id` | Optional. For future per-user personalization. |
| `session_id` | Optional. Groups events within a browsing session. |

**How the feedback loop works:**

1. User searches "running shoes" → results returned
2. User clicks product `p42` → POST /events with `event_type=click`, `query="running shoes"`, `product_id=p42`
3. Background aggregation (every `aggregation_interval_secs`, default 300s) computes per-product signals
4. Next search for "running shoes" → `p42` gets a `query_ctr` boost in the score formula

The `query` field is what activates query-specific CTR. Events without `query` still contribute to global popularity but not to per-query ranking.

**Score formula** (all terms configurable via `[ranking]` weights):

```
score = semantic × w_semantic
      + bm25      × w_bm25
      + popularity × w_popularity     ← global click/view ratio
      + query_ctr  × w_query_ctr      ← clicks for this exact query (default 0.15)
      + availability × w_availability
      + margin    × w_margin
```

Pass `"explain": true` in the search request to see per-factor scores in each hit.

---

## Admin

Admin routes require the global admin API key. See [Admin Overrides](#admin-overrides) for pins, sponsored slots, suppressions, and aggregation. See [Tenants](#tenants) for multi-tenant SaaS management.

### Stats

```
GET /stats
```

Returns index size, vector count, storage backend, and embedding model info.

### Reindex

```
POST /admin/reindex
```

Re-embeds all products using the current embedding model. Use after changing models.

---

## Indexes

Named indexes let you run multiple isolated catalogs in a single server instance. Common use cases: A/B testing different catalogs, multi-tenant isolation, experimental product sets alongside production.

The `"default"` index is always present — it is the persistent index configured at startup. When the server runs with `VECTORIA_VECTOR_BACKEND=edgestore-hnsw`, named indexes are also persisted to disk (`indexes/{name}/` alongside the main database) and survive restarts. In memory-only mode they are lost on restart.

Prefer creating named indexes through the [Tenants API](#tenants) when building multi-tenant SaaS — that flow atomically creates the index, issues an API key, and persists both.

### List indexes

```
GET /indexes
```

Response:
```json
{"indexes": ["default", "staging", "tenant-acme"]}
```

### Create an index

```
POST /indexes
```

Body:
```json
{"name": "staging"}
```

Name rules: 1–64 characters, letters/digits/hyphens/underscores only.

Response: `201 Created`
```json
{"name": "staging", "status": "created"}
```

Error responses:
- `400 Bad Request` — name fails validation
- `409 Conflict` — name already exists
- `422 Unprocessable Entity` — server-wide limit of 100 named indexes reached
- `500 Internal Server Error` — index build failed

### Delete an index

```
DELETE /indexes/{name}
```

Returns `404` if not found. Returns `400` if you attempt to delete `"default"`.

### Index a product into a named index

```
POST /indexes/{name}/products
```

Same body as `POST /products`. Returns `404` if the index doesn't exist.

### Search a named index

```
POST /indexes/{name}/search
```

Same body as `POST /search`. Returns `404` if the index doesn't exist.

### Similar items in a named index

```
POST /indexes/{name}/similar
```

Same body as `POST /products/similar`. Returns `404` if the index doesn't exist.

---

## Admin Overrides

Phase 2 overrides let you apply deterministic, query-scoped rules on top of the algorithmic ranking. They are applied after scoring on every search — no re-indexing required and no aggregation delay.

Three override types, applied in this order:

| Type | Effect |
|---|---|
| **Suppression** | Remove a product from results for a specific query |
| **Pin** | Force a product to an exact position for a specific query |
| **Sponsored slot** | Inject an advertiser product at a fixed position; marks hit with `sponsored: true` |

All override routes exist in two namespaces:

- `/admin/*` — operates on the **default index**, admin key only
- `/indexes/{name}/admin/*` — operates on a **named index**, accessible by the index's tenant key or by an admin

The routes below use `/admin/*`; substitute `/indexes/{name}/admin/` for per-tenant usage.

---

### Pins

Force a product to appear at position N for an exact query string. Bypasses scoring entirely.

```
GET  /admin/pins
POST /admin/pins
DELETE /admin/pins/{id}
```

**Create a pin**

```json
{
  "query": "running shoes",
  "product_id": "sku-001",
  "position": 1
}
```

- `query` — exact match against the search query string
- `position` — 1-indexed; 1 = first result

Response: `201 Created` with the pin object including `id`, `created_at`.

**List pins** — returns `{"pins": [...]}`.

**Delete a pin** — use the `id` returned on creation. Returns `204 No Content`.

---

### Sponsored Slots

Inject an advertiser product before organic results. Supports date-range gating and prefix matching.

```
GET  /admin/sponsored
POST /admin/sponsored
DELETE /admin/sponsored/{id}
```

**Create a sponsored slot**

```json
{
  "query_pattern": "running",
  "product_id": "sku-advert",
  "position": 1,
  "label": "Sponsored",
  "start_at": "2026-01-01T00:00:00Z",
  "end_at": "2026-12-31T23:59:59Z"
}
```

- `query_pattern` — matches any query that starts with this string: `"running"` matches `"running"`, `"running shoes"`, `"running gear"`, etc.
- `label` — returned as `sponsored_label` in the hit metadata (default: `"Sponsored"`)
- `start_at` / `end_at` — optional UTC timestamps; slot is inactive outside this window

The injected hit includes `"sponsored": true` and `"sponsored_label": "<label>"` in its metadata.

**List** — returns `{"sponsored": [...]}`.

**Delete** — use the `id` returned on creation. Returns `204 No Content`.

---

### Suppressions

Remove a product from results for a specific query.

```
GET  /admin/suppressions
POST /admin/suppressions
DELETE /admin/suppressions/{id}
```

**Create a suppression**

```json
{
  "query": "running shoes",
  "product_id": "sku-001"
}
```

**List** — returns `{"suppressions": [...]}`.

**Delete** — restores the product for that query. Returns `204 No Content`.

---

### Override Status

Check whether the index has any active manual overrides ("tainted").

```
GET /admin/overrides
GET /admin/overrides?q=running+shoes
```

Without `?q`, returns a summary of all overrides:

```json
{
  "tainted": true,
  "pin_count": 2,
  "sponsored_count": 1,
  "suppression_count": 0,
  "pins": [...],
  "sponsored": [...],
  "suppressions": [...]
}
```

With `?q=<query>`, the response also includes `active_pins`, `active_sponsored`, and `active_suppressions` — the server-computed subset of overrides that currently apply to that query (prefix matching for sponsored, exact matching for pins/suppressions). Use this to build UIs that show toggle state per result without duplicating the matching logic client-side.

---

### Force Aggregation

Phase 1 behavioral training (click/purchase events) takes effect after the aggregation loop runs (default every 5 minutes). To apply training immediately:

```
POST /admin/aggregate
```

Response: `{"status": "aggregation_complete"}`

---

### Export / Import

Snapshot all overrides to JSON for backup or migration between environments.

```
GET  /admin/training-export
POST /admin/training-import
```

The export format is an `OverrideExport` object containing `pins`, `sponsored`, `suppressions`, and `exported_at`. Pass it back as the body to import. Existing overrides are not cleared before import.

---

## Tenants

The Tenants API is the recommended way to run Vectoria as a multi-tenant SaaS. Each tenant gets:

- A **named index** — isolated product catalog, overrides, and behavioral signals
- A **scoped API key** — can only access their own index namespace
- **Persistent storage** — when running with `VECTORIA_VECTOR_BACKEND=edgestore-hnsw`, the index is written to `indexes/{name}/` and survives restarts

All tenant management routes require the global admin key.

On startup the server reloads all previously-created tenant indexes from disk automatically.

---

### List tenants

```
GET /admin/tenants
```

Response (API keys are redacted in list output):

```json
{
  "tenants": [
    {"name": "acme-corp", "created_at": "2026-07-15T10:00:00Z"},
    {"name": "widgets-inc", "created_at": "2026-07-15T11:30:00Z"}
  ]
}
```

---

### Create a tenant

```
POST /admin/tenants
```

Body:

```json
{"name": "acme-corp"}
```

Name rules: 1–64 characters, letters/digits/hyphens/underscores only.

This atomically:
1. Creates a named index `acme-corp`
2. Generates an API key prefixed `vtk_`
3. Persists both to disk

Response: `201 Created`

```json
{
  "name": "acme-corp",
  "api_key": "vtk_4a7f...",
  "created_at": "2026-07-15T10:00:00Z",
  "index": "acme-corp"
}
```

**The API key is returned once only.** Store it immediately — the list endpoint does not return keys.

Error responses:
- `400 Bad Request` — name fails validation
- `409 Conflict` — tenant already exists

---

### Delete a tenant

```
DELETE /admin/tenants/{name}
```

Removes the tenant key and deletes the named index including all its data on disk.

Response: `204 No Content` on success, `404` if not found.

---

### Rotate API key

Issue a new API key for a tenant. The old key is invalidated immediately — no grace period.

```
POST /admin/tenants/{name}/rotate-key
```

Response:

```json
{"name": "acme-corp", "api_key": "vtk_9b2e..."}
```

---

### Using the tenant API key

The tenant key authenticates the same way as the admin key (`Authorization: Bearer <key>`). The key carries the tenant identity — any index name in the URL path is automatically scoped to that tenant's namespace internally (`{tenant}/{index-name}`). A tenant key for `acme-corp` hitting `POST /indexes/catalog/products` reaches `acme-corp/catalog`, never another tenant's data.

Attempting to access an index that doesn't exist within the tenant's own namespace returns `404 Not Found` (not 403 — the server deliberately avoids confirming whether another tenant's namespace exists).

**Tenant routes available:**

| Route | Description |
|---|---|
| `POST /indexes/{name}/products` | Index a product |
| `POST /indexes/{name}/search` | Search |
| `POST /indexes/{name}/similar` | Similar items |
| `GET /indexes/{name}/admin/pins` | List pins |
| `POST /indexes/{name}/admin/pins` | Create a pin |
| `DELETE /indexes/{name}/admin/pins/{id}` | Delete a pin |
| `GET /indexes/{name}/admin/sponsored` | List sponsored slots |
| `POST /indexes/{name}/admin/sponsored` | Create a sponsored slot |
| `DELETE /indexes/{name}/admin/sponsored/{id}` | Delete a sponsored slot |
| `GET /indexes/{name}/admin/suppressions` | List suppressions |
| `POST /indexes/{name}/admin/suppressions` | Create a suppression |
| `DELETE /indexes/{name}/admin/suppressions/{id}` | Delete a suppression |
| `GET /indexes/{name}/admin/stats` | Index statistics |
| `GET /indexes/{name}/admin/overrides` | Override status (supports `?q=`) |
| `POST /indexes/{name}/admin/aggregate` | Force aggregation |
| `GET /indexes/{name}/admin/training-export` | Export overrides |
| `POST /indexes/{name}/admin/training-import` | Import overrides |

---

### Example: full tenant flow

```bash
# 1. Create a tenant (admin key)
curl -sX POST http://localhost:7700/admin/tenants \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "acme-corp"}' | tee /tmp/tenant.json

TENANT_KEY=$(cat /tmp/tenant.json | python3 -c "import sys,json; print(json.load(sys.stdin)['api_key'])")

# 2. Index products as the tenant
curl -sX POST http://localhost:7700/indexes/acme-corp/products \
  -H "Authorization: Bearer $TENANT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"id": "p1", "text": "running shoe", "metadata": {"title": "Air Runner"}}'

# 3. Search
curl -sX POST http://localhost:7700/indexes/acme-corp/search \
  -H "Authorization: Bearer $TENANT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"q": "running", "limit": 5}'

# 4. Pin a result
curl -sX POST http://localhost:7700/indexes/acme-corp/admin/pins \
  -H "Authorization: Bearer $TENANT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query": "running", "product_id": "p1", "position": 1}'
```

---

## Embedded library (Rust)

`vectoria-core` is published on [crates.io](https://crates.io/crates/vectoria-core) and can be embedded directly in any Rust application — no HTTP server required.

```toml
# Cargo.toml
vectoria-core = "0.1.11"
```

### Async API

```rust
use vectoria_core::{SearchEngineBuilder, model::{SearchRequest, SearchMode}};

let engine = SearchEngineBuilder::new()
    .query_cache(300, 1_000)   // TTL secs, max entries
    .build()
    .await?;

engine.index(product).await?;

let results = engine.search(SearchRequest {
    q: "running shoes".into(),
    mode: SearchMode::Hybrid,
    limit: 10,
    ..Default::default()
}).await?;
```

### Sync API

For callers without an async runtime:

```rust
use vectoria_core::{SearchEngineSync, model::{SearchRequest, SearchMode}};

let engine = SearchEngineSync::new()?;
engine.index(product)?;

let results = engine.search(SearchRequest {
    q: "running shoes".into(),
    ..Default::default()
})?;

engine.reindex()?;   // re-embed all products
engine.stats()?;     // index stats
```

### Preloading an existing database

Point the engine at an existing database directory and call `reindex_all()` after opening to rebuild the in-memory BM25 index and spell corrector from stored products. Storage and vector index share one engine at one path (0.1.11+).

Add `edgestore` as a direct dependency alongside `vectoria-core`:

```toml
vectoria-core = "0.1.11"
edgestore = "1.0"
```

```rust
use std::sync::{Arc, Mutex};
use edgestore::{EdgestoreConfig, Engine};
use vectoria_core::{
    SearchEngineBuilder,
    storage::edgestore::EdgeStoreStorage,
    vector::edgestore::EdgeStoreVectorIndex,
};

// Single engine shared between storage and vector index (0.1.11+)
let engine_handle = Arc::new(Mutex::new(
    Engine::open(EdgestoreConfig::new("./vectoria"))?
));

let storage = Arc::new(EdgeStoreStorage::from_engine(Arc::clone(&engine_handle)));
let vidx = Arc::new(
    EdgeStoreVectorIndex::from_engine(Arc::clone(&engine_handle), None, None)?
);

let engine = SearchEngineBuilder::new()
    .storage(storage)
    .vector_index(vidx)
    .build()
    .await?;

// Rebuild BM25 + spell corrector from stored products
engine.reindex_all().await?;
```

`EdgeStoreStorage` and `EdgeStoreVectorIndex` open the same files on restart. The HNSW graph loads automatically — no rebuild needed unless the embedding model changed.

### Bulk indexing

Index products individually in a loop. If products already carry pre-computed vectors, set `product.vector` and the embedding step is skipped (only storage, BM25, and spell corrector are updated). After bulk loading with HNSW, call `reindex_all()` once to flush the graph:

```rust
for p in products {
    let product = Product {
        id: p.id.clone(),
        text: Some(p.text),
        vector: Some(p.embedding),  // skip embed call
        metadata: p.meta,
        ..Product::new(p.id, p.meta)
    };
    engine.index(product).await?;
}

// Flush HNSW graph after bulk load
engine.reindex_all().await?;
```

For concurrent async bulk loading:

```rust
let engine = Arc::new(engine);
let handles: Vec<_> = products.into_iter().map(|p| {
    let e = Arc::clone(&engine);
    tokio::spawn(async move { e.index(p).await })
}).collect();

for h in handles { h.await??; }
engine.reindex_all().await?;
```

### Builder options

| Method | Default |
|--------|---------|
| `.storage(arc)` | `MemoryStorage` |
| `.vector_index(arc)` | `MemoryVectorIndex` |
| `.embedding(arc)` | `LocalEmbedding` (multilingual-e5-small) |
| `.weights(RankingWeights)` | semantic=0.7, bm25=0.3, popularity=0.2, query_ctr=0.15, availability=0.05, margin=0.05 |
| `.query_cache(ttl, max)` | disabled |
| `.reranker()` | disabled |
| `.field_weights(HashMap<String, usize>)` | uniform (1× repeat for each field) |

All types implement `Send + Sync`. Storage and vector index backends can be swapped to `EdgeStoreStorage` and `EdgeStoreVectorIndex` for persistence.

### OpenAI-compatible embedding provider

Set `embedding.provider = "openai-compatible"` to use any OpenAI-compatible `/v1/embeddings` endpoint (Ollama, llama.cpp, vLLM, LM Studio, OpenAI):

```toml
[embedding]
provider = "openai-compatible"
model = "nomic-embed-text"
base_url = "http://localhost:11434"
dims = 768
```

Or via environment variables:
- `VECTORIA_EMBEDDING_PROVIDER=openai-compatible`
- `VECTORIA_EMBEDDING_BASE_URL=http://localhost:11434`
- `VECTORIA_EMBEDDING_MODEL=nomic-embed-text`

### Margin signal

The `margin` factor in the score formula reads `metadata.margin` (float 0.0–1.0) from indexed product metadata. It is 0.0 if the field is absent. To activate it, include a `margin` field when indexing:

```json
{"id": "p1", "text": "...", "metadata": {"title": "...", "price": 99.0, "margin": 0.35}}
```

Configure its weight in `[ranking]`:
```toml
[ranking]
margin = 0.05
```

