# Vectoria

Embedded hybrid search engine for ecommerce. Single binary, no external services required.

Vectoria combines BM25 full-text search, vector similarity, and behavioral signals (clicks, purchases, views) into a single ranking formula. Its main advantages over BM25-only search (SQLite FTS5, Meilisearch basic tier):

- **Zero-result elimination** — semantic mode returns results for long-tail queries that share no keywords with any product. Hybrid mode keeps BM25 precision while removing zero results entirely.
- **Query-level CTR feedback** — products previously clicked for a given query rank higher for future searches of the same query. The feedback loop activates immediately after the first click event with a `query` field.

The embedding model runs locally via ONNX; no external API calls required unless you configure an OpenAI-compatible provider.

Designed for catalogs up to ~500K products on a single node.

## Requirements

- Rust 1.80+ (for building from source)
- ~40 MB disk space for the default embedding model (downloaded on first run)

## Getting started

```sh
git clone https://github.com/yourorg/vectoria
cd vectoria
cargo build --release
```

Start the server:

```sh
./target/release/vectoria-server
```

On first run, it downloads the `multilingual-e5-small` embedding model (~40 MB) and prints an API key:

```
INFO vectoria v0.1.5
INFO api_key: a1b2c3d4e5f6...
INFO listening on http://0.0.0.0:7700
```

Index a product:

```sh
curl -X POST http://localhost:7700/products \
  -H "Authorization: Bearer <api_key>" \
  -H "Content-Type: application/json" \
  -d '{"id":"p1","text":"Nike Air Max running shoe lightweight","metadata":{"title":"Nike Air Max","brand":"Nike","price":120}}'
```

Search:

```sh
curl -X POST http://localhost:7700/search \
  -H "Authorization: Bearer <api_key>" \
  -H "Content-Type: application/json" \
  -d '{"q":"running shoes","limit":10}'
```

## Configuration

Place a `vectoria.toml` in the working directory. All fields are optional.

```toml
[server]
host = "0.0.0.0"
port = 7700
api_key = "your-key"        # auto-generated if absent
skip_consent = false        # skip model download prompt on first run

[storage]
path = "./vectoria.db"      # path for persistent index files

[embedding]
provider = "local"          # "local" | "openai-compatible"
model = "multilingual-e5-small"

[index]
vector_backend = "edgestore-hnsw"   # see below
enable_reranker = false             # cross-encoder reranking (slower, higher quality)
aggregation_interval_secs = 300     # how often behavioral signals fold into ranking
query_cache_ttl_secs = 60
query_cache_max_entries = 1000
embedding_cache_size = 10000

[ranking]
semantic     = 0.7
bm25         = 0.3
popularity   = 0.2
query_ctr    = 0.15   # boost products previously clicked for this exact query
availability = 0.05
margin       = 0.05
```

**vector_backend options:**

- `edgestore-hnsw` — persistent HNSW index (activated after `POST /admin/reindex`), recommended for production
- `sqlite` — SQLite metadata + EdgeStore flat vector index
- `memory` — everything in-memory, lost on restart (development only)

**Environment variable overrides** (take precedence over `vectoria.toml`):

```
VECTORIA_HOST
VECTORIA_PORT
VECTORIA_API_KEY
VECTORIA_STORAGE_PATH
VECTORIA_EMBEDDING_PROVIDER
VECTORIA_EMBEDDING_BASE_URL
VECTORIA_EMBEDDING_MODEL
VECTORIA_CONFIG              # path to config file, default: vectoria.toml
VECTORIA_SKIP_CONSENT=1      # maps to server.skip_consent
VECTORIA_ENABLE_RERANKER=1   # maps to index.enable_reranker
```

## Behavioral ranking

Vectoria uses two behavioral signals derived from `POST /events`:

| Signal | Source | Effect |
|--------|--------|--------|
| **Global popularity** | click_count / view_count (all queries) | Products with high overall CTR rank slightly higher everywhere |
| **Query CTR** | click + purchase count per (query, product) | Products clicked for *this exact query* rank higher for future searches |

Always include `query` in click/purchase events to activate query-level CTR:

```sh
curl -X POST http://localhost:7700/events \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"event_type":"click","product_id":"sku-001","query":"running shoes"}'
```

Events without `query` still contribute to global popularity but not per-query ranking. Background aggregation runs every `aggregation_interval_secs` (default 300s). The `query_ctr` weight defaults to `0.15` and is configurable in `[ranking]`.

Use `"explain": true` in search requests to see per-factor scores:

```json
{
  "factor": "query_ctr",
  "score": 1.0,
  "weight": 0.15
}
```

## CLI

The `vectoria` CLI handles bulk operations against a running server.

```sh
# Bulk import from NDJSON, CSV, or Parquet
vectoria import products.ndjson --server http://localhost:7700 --api-key <key>

# Re-embed all products after a model change
vectoria reindex --server http://localhost:7700 --api-key <key>

# Benchmark search quality (Recall@K, NDCG@K, MRR) across all modes
vectoria bench judges.ndjson --mode all --server http://localhost:7700 --api-key <key>
```

## Embedded usage (Rust)

Add `vectoria-core` to your `Cargo.toml`:

```toml
vectoria-core = "0.1.5"
```

**Async (with Tokio):**

```rust
use vectoria_core::{SearchEngineBuilder, model::{SearchRequest, SearchMode}};

let engine = SearchEngineBuilder::new()
    .query_cache(300, 1_000)
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

**Sync (no Tokio required in caller):**

```rust
use vectoria_core::{SearchEngineSync, model::{SearchRequest, SearchMode}};

let engine = SearchEngineSync::new()?;

let results = engine.search(SearchRequest {
    q: "running shoes".into(),
    ..Default::default()
})?;
```

`SearchEngineBuilder` accepts optional overrides for storage backend, vector index, embedding provider, ranking weights, query cache TTL/size, and cross-encoder reranking. All default to in-memory storage and the local `multilingual-e5-small` ONNX model.

**Preloading an existing database** — pass a persistent backend pointing to an existing file, then call `reindex_all()` to rebuild the BM25 index and spell corrector from stored products:

```rust
use std::{path::Path, sync::Arc};
use vectoria_core::{SearchEngineBuilder, storage::sqlite::SqliteStorage};

let engine = SearchEngineBuilder::new()
    .storage(Arc::new(SqliteStorage::open(Path::new("./vectoria.db"))?))
    .build()
    .await?;

engine.reindex_all().await?;  // rebuild BM25 + spell corrector
```

**Bulk indexing** — call `engine.index()` in a loop. If products already have vectors, set `product.vector` to skip the embedding step. Call `reindex_all()` once after bulk loading to flush the HNSW graph.

Publish target: `make publish` (requires `cargo login` or `CARGO_REGISTRY_TOKEN`). See [crates.io/crates/vectoria-core](https://crates.io/crates/vectoria-core).

## Demo webstore

The fastest way to try Vectoria with real data is the Make-based demo. It uses the
Amazon ESCI product catalog — **a separate license agreement is required**:
<https://github.com/amazon-science/esci-data>

```sh
make server-bg      # start server in background (waits until healthy)
make esci-import    # download ESCI data and import 5000 products (~5 min first run)
make webstore       # serve demo store at http://localhost:8080
```

To benchmark after importing:

```sh
make esci-judges    # build judged query dataset
make bench          # Recall@K / NDCG@K / MRR across bm25 / semantic / hybrid
```

Run `make help` for all targets and overridable variables. See
[docs/quickstart.md](docs/quickstart.md) for a full walkthrough.

## Benchmark

Amazon ESCI dataset, 5000 US products, `multilingual-e5-small` embedding. Results by label hardness:

| Label set | Queries | BM25 MRR | Hybrid MRR | Semantic MRR | Coverage (all modes) |
|-----------|---------|----------|------------|--------------|----------------------|
| E (exact) | 107     | 0.5842   | 0.5882     | 0.4922       | **100%**             |
| E+S       | 117     | 0.6595   | 0.6576     | 0.5690       | **100%**             |
| E+S+C     | 119     | 0.6835   | 0.6803     | 0.5797       | **100%**             |

ESCI label meanings: E = exact product name match (BM25-optimal), S = substitute/concept (keyword overlap low), C = complement (e.g. query "camera" → relevant product "camera bag").

Key takeaways:
- **100% coverage across all modes and label sets** — zero-result queries handled by spell-correction fallback (compound split + typo correction applied only when BM25 returns no results)
- **Hybrid ≈ BM25 on exact queries**, with coverage maintained by semantic fallback
- **Semantic covers zero-keyword queries** that BM25 would miss entirely (S and C labels)
- Semantic p50 latency: **2ms** (cached embeddings); BM25/hybrid: sub-ms to ~3ms

Reproduce with custom label sets:
```sh
make esci-import                                  # import 5000 US products
make esci-judges                                  # default: E+S labels
# Or choose label set:
cargo run --example esci_import -p vectoria-cli -- \
  data/esci/shopping_queries_dataset_products.parquet \
  data/esci/shopping_queries_dataset_examples.parquet \
  --judges data/esci/judges.ndjson --labels E,S,C --locale us \
  --max-products 5000 --server http://localhost:7700 --api-key <key>
make bench
```

**Next benchmark target**: [WANDS (Wayfair)](https://github.com/wayfair/WANDS) — 42K furniture/home goods products, complex descriptive concept queries ("mid century modern floor lamp"). Expected to show larger hybrid advantage since Wayfair queries are more concept-driven than ESCI exact matches.

## API reference

See [docs/api.md](docs/api.md).

## Docker

**Quickest start** — Docker Compose (builds image, mounts volumes, sets API key):

```sh
VECTORIA_API_KEY=my-secret-key docker compose up
```

Or build and run manually:

```sh
# Full image — ONNX model downloaded on first start (~400 MB image, ~40 MB model cache)
docker build --target vectoria-full -t vectoria:full .
docker run -p 7700:7700 \
  -v vectoria-data:/data \
  -v fastembed-cache:/root/.cache/fastembed \
  -e VECTORIA_API_KEY=my-secret-key \
  vectoria:full

# Slim image — requires OpenAI-compatible embedding provider (~50 MB)
docker build --target vectoria-slim -t vectoria:slim .
docker run -p 7700:7700 \
  -v vectoria-data:/data \
  -e VECTORIA_API_KEY=my-secret-key \
  -e VECTORIA_EMBEDDING_BASE_URL=https://api.openai.com/v1 \
  -e VECTORIA_EMBEDDING_MODEL=text-embedding-3-small \
  vectoria:slim
```

Both images include the `vectoria` CLI. Run it against the container:

```sh
docker exec vectoria-vectoria-1 vectoria --server http://localhost:7700 --api-key my-secret-key stats
```

Volumes:
- `/data` — persistent index and SQLite storage
- `/root/.cache/fastembed` — ONNX model cache (full image only; mount to avoid re-downloading)

## Building from source

```sh
cargo build --release               # server + CLI
cargo test                          # integration tests (no mocks)
cargo build --release -p vectoria-server
cargo build --release -p vectoria-cli
```
