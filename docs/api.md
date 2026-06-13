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
{"status": "ok", "version": "0.1.0"}
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

## Multi-index API

Provides index-namespaced endpoints. Each `indexName` maps to an isolated search engine instance.

```
POST   /1/indexes/{indexName}                  index an object
PUT    /1/indexes/{indexName}/{objectID}        update an object
DELETE /1/indexes/{indexName}/{objectID}        delete an object
POST   /1/indexes/{indexName}/query             search
POST   /1/indexes/{indexName}/batch             batch operations
```

### Search an index

```
POST /1/indexes/{indexName}/query
```

Body:
```json
{
  "query": "running shoes",
  "hitsPerPage": 20,
  "page": 0
}
```

Response:
```json
{
  "hits": [...],
  "page": 0,
  "nbHits": 42,
  "nbPages": 3,
  "hitsPerPage": 20,
  "processingTimeMS": 12,
  "query": "running shoes"
}
```

### Batch

```
POST /1/indexes/{indexName}/batch
```

Body:
```json
{
  "requests": [
    {"action": "addObject", "body": {"objectID": "p1", "text": "..."}},
    {"action": "deleteObject", "body": {"objectID": "p2"}}
  ]
}
```

Supported actions: `addObject`, `updateObject`, `deleteObject`.
