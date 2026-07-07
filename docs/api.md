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

- `id` — required, must be unique
- `text` — used for embedding generation and full-text search
- `metadata` — arbitrary JSON object stored and returned with search results

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
      {"factor": "bm25",        "score": 0.363, "weight": 0.30, "contribution": 0.1.11},
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

The `"default"` index is always present — it is the persistent index configured at startup (`[index] vector_backend` setting). Named indexes created via the API use in-memory storage and are lost on restart.

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

Both persistent backends accept a file path. Point them at an existing database and call `reindex_all()` after opening to rebuild the in-memory BM25 index and spell corrector from stored products:

```rust
use std::sync::Arc;
use vectoria_core::{
    SearchEngineBuilder,
    storage::edgestore::EdgeStoreStorage,
    vector::edgestore::EdgeStoreVectorIndex,
};

let storage = Arc::new(EdgeStoreStorage::open("./vectoria.db")?);
let vidx = Arc::new(EdgeStoreVectorIndex::open("./vectoria.vec", None, None)?);

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

