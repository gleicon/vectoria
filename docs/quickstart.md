# Quickstart

This guide takes you from zero to a running Vectoria server with real product data and a searchable demo storefront.

**Time:** ~10 minutes (plus ~5 minutes on first run to download the embedding model and ESCI data).

---

## Prerequisites

- Rust 1.80+ — install at <https://rustup.rs>
- `curl` or `wget`
- `python3` (for the webstore frontend)
- ~1.5 GB free disk space (ESCI data + embedding model)

---

## 1. Clone and build

```sh
git clone https://github.com/gleicon/vectoria
cd vectoria
cargo build --release -p vectoria-server -p vectoria-cli
```

Or use Make (does the same thing):

```sh
make build
```

---

## 2. Configure

A ready-to-use config is included at `vectoria.toml`. It sets:

- Port `7700`
- API key `vectoria-esci-demo`
- In-memory index (data resets on restart — change to `edgestore-hnsw` for persistence)

For a persistent index, edit `vectoria.toml`:

```toml
[index]
vector_backend = "edgestore-hnsw"

[storage]
path = "./data/vectoria.db"
```

---

## 3. Start the server

**Option A — Docker Compose (recommended, no Rust required):**

```sh
# Full image — ONNX model downloaded on first start
VECTORIA_API_KEY=my-secret-key docker compose up

# Run in background
VECTORIA_API_KEY=my-secret-key docker compose up -d
```

The compose file mounts two named volumes: `/data` for the index and `/root/.cache/fastembed` for the model cache. The model (~40 MB) downloads once and is reused on subsequent starts.

**Option B — Docker run:**

```sh
docker build --target vectoria-full -t vectoria:full .
docker run -p 7700:7700 \
  -v vectoria-data:/data \
  -v fastembed-cache:/root/.cache/fastembed \
  -e VECTORIA_API_KEY=my-secret-key \
  vectoria:full
```

**Option C — from source (foreground):**

```sh
cargo run --release -p vectoria-server
# or: make server
```

**Option D — from source (background):**

```sh
make server-bg
# Waits until the server responds at http://localhost:7700/health
# Logs → /tmp/vectoria.log
```

On first run (any option) the server downloads the `multilingual-e5-small` embedding model (~40 MB). It prints:

```
api_key: vectoria-esci-demo
INFO listening on http://0.0.0.0:7700
```

Verify it is up:

```sh
curl http://localhost:7700/health
# {"status":"ok","version":"0.1.10"}
```

---

## 4. Load the ESCI demo dataset

> **License required.** The Amazon Shopping Queries Dataset (ESCI) is proprietary.
> Read and accept the terms at <https://github.com/amazon-science/esci-data> before downloading.

```sh
make esci-import
```

This:
1. Downloads `shopping_queries_dataset_products.parquet` (~1.1 GB) to `data/esci/` — skipped on subsequent runs
2. Downloads `shopping_queries_dataset_examples.parquet` (~68 MB)
3. Imports 5 000 US-locale products into the running server

To import more products or a different locale:

```sh
make esci-import MAX_PRODUCTS=50000 LOCALE=es
```

To import the full catalog (~1.8 M products — takes hours):

```sh
make esci-import MAX_PRODUCTS=0
```

---

## 5. Open the demo storefront

```sh
make webstore
```

Open <http://localhost:8080>. The demo store lets you search in three modes — BM25, semantic, and hybrid — and shows per-result score breakdowns.

---

## 6. Benchmark search quality

Generate a judged query file from the ESCI relevance labels, then run the benchmark:

```sh
make esci-judges   # writes data/esci/judges.ndjson
make bench         # Recall@K, NDCG@K, MRR across bm25 / semantic / hybrid
```

Sample output (ESCI, 5000 US products, E+S labels, 117 judged queries):

```
── Mode: bm25 ──────────────────────────────
  Coverage:     100.0%
  Recall@10:    0.5347
  NDCG@10:      0.5796
  MRR:          0.6595
  Latency p50:  0.4ms

── Mode: semantic ──────────────────────────
  Coverage:     100.0%
  Recall@10:    0.4407
  NDCG@10:      0.4765
  MRR:          0.5690
  Latency p50:  2.0ms

── Mode: hybrid ────────────────────────────
  Coverage:     100.0%
  Recall@10:    0.4914
  NDCG@10:      0.5635
  MRR:          0.6576
  Latency p50:  2.2ms
```

All modes reach 100% coverage. Zero-result queries are retried with spell correction (compound splits and typo correction) before falling back to semantic results.

---

## 7. Import your own data

**NDJSON** (one product per line):

```jsonl
{"id":"sku-001","text":"Blue trail running shoe waterproof","metadata":{"title":"Trail X","brand":"Merrell","price":149.99,"in_stock":true}}
{"id":"sku-002","text":"Yoga mat non-slip extra thick 6mm","metadata":{"title":"ProMat","brand":"Manduka","price":89.00,"in_stock":true}}
```

```sh
vectoria import products.ndjson \
  --server http://localhost:7700 \
  --api-key vectoria-esci-demo
```

**CSV** (any columns; `id`, `sku`, or `product_id` used as the product ID):

```sh
vectoria import catalog.csv --server http://localhost:7700 --api-key vectoria-esci-demo
```

**Parquet** (all string and numeric columns mapped to metadata automatically):

```sh
vectoria import catalog.parquet --server http://localhost:7700 --api-key vectoria-esci-demo
```

---

## 8. Query the API directly

```sh
API_KEY=vectoria-esci-demo

# Hybrid search
curl -X POST http://localhost:7700/search \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"q":"waterproof trail shoes","limit":5,"mode":"hybrid"}'

# Similar products by ID
curl http://localhost:7700/products/sku-001/similar \
  -H "Authorization: Bearer $API_KEY"

# Record a click event (feeds behavioral ranking)
curl -X POST http://localhost:7700/events \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"type":"click","product_id":"sku-001","query":"trail shoes"}'

# Stats
curl http://localhost:7700/stats -H "Authorization: Bearer $API_KEY"
```

---

## 9. Run the test suite

No running server or model download needed — all tests use a stub embedder.

```sh
cargo test --workspace
```

Expected output:

```
test result: ok. 57 passed; 0 failed; 0 ignored; 0 measured
```

Scope to a specific area:

```sh
cargo test -p vectoria-core edgestore   # persistence tests
cargo test -p vectoria-core spell       # spell correction
cargo test -p vectoria-server           # index registry
```

---

## 10. Stop and clean up

```sh
make kill       # stop background server
make clean      # delete downloaded ESCI parquet files (not the model cache)
```

The embedding model cache stays at `~/.cache/fastembed/`. Delete it manually if you want to reclaim the ~40 MB.

---

## 11. Use as an embedded Rust library

No HTTP server required. See [API reference — Embedded library](api.md#embedded-library-rust) for the full builder API, sync wrapper, persistence, and bulk indexing examples.

```toml
[dependencies]
vectoria-core = "0.1.10"
```

---

## Make target reference

| Target | What it does |
|---|---|
| `make test` | Run full test suite (57 tests, no server, no model download) |
| `make build` | `cargo build --release` for server + CLI |
| `make server` | Start server in foreground |
| `make server-bg` | Start server in background, wait until healthy |
| `make kill` | Stop background server |
| `make esci-download` | Download ESCI parquet files only |
| `make esci-import` | Download + import ESCI products |
| `make esci-judges` | Build judged query file from ESCI labels |
| `make bench` | Run ESCI benchmark against running server |
| `make wands-download` | Download WANDS dataset (CC BY-SA 4.0, no license required) |
| `make wands-import` | Download + import 42 994 WANDS products |
| `make wands-judges` | Build judged query file from WANDS labels |
| `make wands-bench` | Run WANDS benchmark against running server |
| `make webstore` | Serve demo store at `:8080` |
| `make clean` | Delete downloaded data files |
| `make version` | Print current version from `Cargo.toml` |
| `make publish-dry-run` | Verify `vectoria-core` is ready for crates.io |
| `make publish` | Publish `vectoria-core` to crates.io |
| `make tag NEW_VERSION=x.y.z` | Bump version, commit, push branch + tag |

Override variables on the command line:

```sh
make esci-import MAX_PRODUCTS=10000 LOCALE=es SERVER=http://myserver:7700 API_KEY=mykey
```

---

## Next steps

- [API reference](api.md) — HTTP endpoints and embedded library usage
- [Configuration reference](../vectoria.toml) — all fields documented inline
- [Production deployment](prod.md)
- [crates.io — vectoria-core](https://crates.io/crates/vectoria-core) — embed in your Rust app
