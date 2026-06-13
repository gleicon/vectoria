# Vectoria

Embedded semantic search engine for ecommerce. Single binary, no external services required.

Vectoria runs inside your application or as a standalone HTTP server. It combines BM25 full-text search with vector similarity (HNSW or flat index) and re-ranks results using behavioral signals — clicks, purchases, views. The embedding model runs locally via ONNX; no API calls to external services unless you configure an OpenAI-compatible provider.

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
INFO vectoria v0.1.0
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

[storage]
path = "./vectoria.db"      # path for persistent index files

[embedding]
provider = "local"          # "local" | "openai-compatible"
model = "multilingual-e5-small"

[index]
vector_backend = "edgestore-hnsw"   # see below

[ranking]
semantic    = 0.6
popularity  = 0.2
availability = 0.1
margin      = 0.1
```

**vector_backend options:**

- `edgestore-hnsw` — persistent HNSW index, recommended for production
- `sqlite` — SQLite metadata + TurboVec flat vector file
- `turbovec` — in-memory metadata + TurboVec flat vector file
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
VECTORIA_SKIP_CONSENT=1      # skip model download prompt
VECTORIA_ENABLE_RERANKER=1   # enable cross-encoder reranking (slower, higher quality)
```

## CLI

The `vectoria` CLI handles bulk operations against a running server.

```sh
# Bulk import from NDJSON, CSV, or Parquet
vectoria import products.ndjson --server http://localhost:7700 --api-key <key>

# Load Amazon ESCI dataset and import
vectoria esci products.parquet examples.parquet --import --locale us --max-products 5000

# Re-embed all products after a model change
vectoria reindex --server http://localhost:7700 --api-key <key>

# Run evaluation against judged queries
vectoria eval judges.ndjson --server http://localhost:7700
```

## Demo webstore

To run the demo against the Amazon ESCI product catalog:

```sh
# Start the server first, then:
examples/webstore/setup.sh --api-key <key>

# Serve the frontend
python3 -m http.server 8080 --directory examples/webstore
```

Open `http://localhost:8080`. The setup script downloads ~1.2 GB of ESCI data to
`~/.local/share/vectoria/esci/` on first run and imports 5000 US-locale products.

## API reference

See [docs/api.md](docs/api.md).

## Docker

```sh
# Full image (includes ONNX model, ~400 MB)
docker build --target vectoria-full -t vectoria:full .

# Slim image (OpenAI-compatible embedding only, ~50 MB)
docker build --target vectoria-slim -t vectoria:slim .

docker run -p 7700:7700 -v $(pwd)/data:/data vectoria:full
```

## Building from source

```sh
cargo build --release               # server + CLI
cargo test                          # integration tests (no mocks)
cargo build --release -p vectoria-server
cargo build --release -p vectoria-cli
```
