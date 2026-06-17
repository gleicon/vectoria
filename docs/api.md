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
{"status": "ok", "version": "0.1.2"}
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
GET /products/{id}/similar?limit=10
```

Returns products with similar vectors to the given product.

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
  "explain": false,
  "rerank": false
}
```

Fields:

- `q` — query string, required
- `limit` — results per page, default 20
- `offset` — pagination offset, default 0
- `mode` — `"hybrid"` (default), `"semantic"`, or `"bm25"`
- `filters` — key/value pairs matched against product metadata
- `explain` — include per-result score breakdown
- `rerank` — apply cross-encoder reranking (requires `VECTORIA_ENABLE_RERANKER=1`)

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
  "query": "running shoes"
}
```

### Autocomplete

```
GET /autocomplete?q=runn&limit=5
```

Returns suggested query completions based on indexed text.

---

## Events

Record behavioral signals used to adjust ranking.

```
POST /events
```

Body:
```json
{
  "id": "evt-uuid",
  "product_id": "p1",
  "event_type": "click"
}
```

`event_type` values: `click`, `purchase`, `view`, `cart`.

Events are aggregated in the background every 5 minutes (configurable via `[index] aggregation_interval_secs`). Popularity and conversion signals influence final ranking scores.

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

## Embedded library (Rust)

`vectoria-core` is published on [crates.io](https://crates.io/crates/vectoria-core) and can be embedded directly in any Rust application — no HTTP server required.

```toml
# Cargo.toml
vectoria-core = "0.1.2"
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
use std::{path::Path, sync::Arc};
use vectoria_core::{SearchEngineBuilder, storage::sqlite::SqliteStorage};

let storage = Arc::new(SqliteStorage::open(Path::new("./vectoria.db"))?);

let engine = SearchEngineBuilder::new()
    .storage(storage)
    .build()
    .await?;

// Rebuild BM25 + spell corrector from stored products
engine.reindex_all().await?;
```

For HNSW persistence, pair `EdgeStoreStorage::open(path)` with `EdgeStoreVectorIndex::open(path, model_id, dims)`. The HNSW graph persists to disk and loads automatically — no rebuild needed unless the embedding model changed.

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
| `.weights(RankingWeights)` | semantic=0.7, bm25=0.3, popularity=0.2 |
| `.query_cache(ttl, max)` | disabled |
| `.reranker()` | disabled |

All types implement `Send + Sync`. Storage and vector index backends can be swapped to `SqliteStorage`, `EdgeStoreStorage`, or `EdgeStoreVectorIndex` for persistence.

