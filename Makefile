SERVER        := http://localhost:7700
API_KEY       := vectoria-esci-demo
DATA_DIR      := data/esci
PRODUCTS      := $(DATA_DIR)/shopping_queries_dataset_products.parquet
EXAMPLES      := $(DATA_DIR)/shopping_queries_dataset_examples.parquet
JUDGES        := $(DATA_DIR)/judges.ndjson
PRODUCTS_URL  := https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_products.parquet
EXAMPLES_URL  := https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_examples.parquet
MAX_PRODUCTS  := 5000
LOCALE        := us
WEBSTORE_PORT := 8080

WANDS_DIR       := data/wands
WANDS_BASE_URL  := https://raw.githubusercontent.com/wayfair/WANDS/main/dataset
WANDS_PRODUCTS  := $(WANDS_DIR)/product.csv
WANDS_QUERIES   := $(WANDS_DIR)/query.csv
WANDS_LABELS    := $(WANDS_DIR)/label.csv
WANDS_JUDGES    := $(WANDS_DIR)/judges.ndjson
WANDS_MAX       := 42994

.PHONY: help build test server server-bg kill esci-download esci-import esci-judges bench webstore clean \
        publish publish-dry-run tag version \
        wands-download wands-import wands-judges wands-bench \
        wasm-build wasm-pack

VERSION       := $(shell cargo metadata --no-deps --format-version 1 | python3 -c "import sys,json; print(json.load(sys.stdin)['packages'][0]['version'])" 2>/dev/null || grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

help:
	@echo "Vectoria — demo targets"
	@echo ""
	@echo "  Quick start:"
	@echo "    make server-bg      start server in background"
	@echo "    make esci-import    download ESCI data and import ($(MAX_PRODUCTS) products)"
	@echo "    make webstore       serve demo store at http://localhost:$(WEBSTORE_PORT)"
	@echo ""
	@echo "  All targets:"
	@echo "    test                run full test suite (no server, no model download)"
	@echo "    build               cargo build --release (server + CLI)"
	@echo "    server              start server in foreground (own terminal)"
	@echo "    server-bg           start server in background, log → /tmp/vectoria.log"
	@echo "    kill                stop background server"
	@echo "    esci-download       download ESCI parquet files to $(DATA_DIR)/"
	@echo "    esci-import         import $(MAX_PRODUCTS) products into running server"
	@echo "    esci-judges         build judged query file for benchmarking → $(JUDGES)"
	@echo "    bench               run Recall@K / NDCG@K / MRR benchmark (all modes)"
	@echo "    webstore            serve examples/webstore/ at :$(WEBSTORE_PORT)"
	@echo "    clean               remove downloaded data files"
	@echo ""
	@echo "  Release (run in order):"
	@echo "    make test                    confirm all tests pass"
	@echo "    make publish-dry-run         preflight check (no upload)"
	@echo "    make tag NEW_VERSION=x.y.z   bump versions, commit, push branch + signed tag"
	@echo "    make publish                 upload vectoria-core to crates.io"
	@echo ""
	@echo "  Release targets:"
	@echo "    version             print current version from Cargo.toml"
	@echo "    publish-dry-run     verify crate passes crates.io preflight (no upload)"
	@echo "    publish             publish vectoria-core to crates.io (cargo login required)"
	@echo "    tag NEW_VERSION=x.y.z  bump Cargo.toml + 6 doc/web files, commit, push, sign tag"
	@echo ""
	@echo "  Variables (override on command line):"
	@echo "    MAX_PRODUCTS=$(MAX_PRODUCTS)   LOCALE=$(LOCALE)   WEBSTORE_PORT=$(WEBSTORE_PORT)"
	@echo "    SERVER=$(SERVER)   API_KEY=$(API_KEY)"
	@echo ""
	@echo "  ESCI dataset: Amazon license required — https://github.com/amazon-science/esci-data"

# ── Build ──────────────────────────────────────────────────────────────────
#
# Requires: Rust 1.80+ (rustup.rs). No external services or model downloads needed.
#
# Outputs:
#   ./target/release/vectoria-server   — HTTP search server (port 7700)
#   ./target/release/vectoria-cli      — import / benchmark / reindex CLI
#
# Typical dev flow:
#   make test     → confirm all tests pass (fast, no model needed)
#   make build    → compile release binaries
#   make server   → start server in foreground to verify it runs

# Run the full workspace test suite.
# Uses a hash-based stub embedder — no model download, no running server required.
# 57 tests covering search, persistence, behavioral signals, caching, and spell correction.
test:
	cargo test --workspace --exclude vectoria-wasm

# Compile release binaries for vectoria-server and vectoria-cli.
# Skips vectoria-core (library only, no binary).
build:
	cargo build --release -p vectoria-server -p vectoria-cli

# Build the WASM package using wasm-pack (install: cargo install wasm-pack).
# Outputs to vectoria-wasm/pkg/ — ready to publish to npm or load in a Worker.
wasm-pack:
	wasm-pack build vectoria-wasm --target web --out-dir pkg

# Compile-check the WASM crate without wasm-pack (faster, no npm artifact).
wasm-build:
	cargo check -p vectoria-wasm --target wasm32-unknown-unknown

# ── Server ─────────────────────────────────────────────────────────────────

server:
	cargo run --release -p vectoria-server

server-bg:
	@nohup cargo run --release -p vectoria-server > /tmp/vectoria.log 2>&1 & echo $$! > /tmp/vectoria.pid
	@echo "Server PID $$(cat /tmp/vectoria.pid) started. Waiting for it to be ready..."
	@for i in $$(seq 1 20); do \
		curl -sf $(SERVER)/health >/dev/null 2>&1 && echo "  Server ready at $(SERVER)" && exit 0; \
		sleep 1; \
	done; \
	echo "  Server did not start within 20s. Check: tail -f /tmp/vectoria.log"; exit 1

kill:
	@if [ -f /tmp/vectoria.pid ]; then \
		kill $$(cat /tmp/vectoria.pid) 2>/dev/null && echo "Server stopped" || true; \
		rm -f /tmp/vectoria.pid; \
	else \
		pkill -f vectoria-server 2>/dev/null && echo "Server stopped" || echo "No server running"; \
	fi

# ── ESCI dataset ───────────────────────────────────────────────────────────
# Amazon ESCI dataset requires a separate license.
# https://github.com/amazon-science/esci-data

$(DATA_DIR):
	mkdir -p $(DATA_DIR)

$(PRODUCTS): | $(DATA_DIR)
	@echo "Downloading ESCI products (~1.1 GB)..."
	@echo "Amazon ESCI license required: https://github.com/amazon-science/esci-data"
	curl -L --progress-bar -o $@ $(PRODUCTS_URL)

$(EXAMPLES): | $(DATA_DIR)
	@echo "Downloading ESCI examples (~68 MB)..."
	curl -L --progress-bar -o $@ $(EXAMPLES_URL)

esci-download: $(PRODUCTS) $(EXAMPLES)

esci-import: esci-download
	@curl -sf $(SERVER)/health >/dev/null 2>&1 || \
		(echo "Error: server not running at $(SERVER). Run 'make server-bg' first."; exit 1)
	cargo run --example esci_import -p vectoria-cli -- \
		$(PRODUCTS) $(EXAMPLES) \
		--import \
		--locale $(LOCALE) \
		--max-products $(MAX_PRODUCTS) \
		--server $(SERVER) \
		--api-key $(API_KEY)

esci-judges: esci-download
	@curl -sf $(SERVER)/health >/dev/null 2>&1 || \
		(echo "Error: server not running at $(SERVER). Run 'make server-bg' first."; exit 1)
	cargo run --example esci_import -p vectoria-cli -- \
		$(PRODUCTS) $(EXAMPLES) \
		--judges $(JUDGES) \
		--locale $(LOCALE) \
		--max-products $(MAX_PRODUCTS) \
		--server $(SERVER) \
		--api-key $(API_KEY)

# ── Benchmark ──────────────────────────────────────────────────────────────

bench: $(JUDGES)
	@curl -sf $(SERVER)/health >/dev/null 2>&1 || \
		(echo "Error: server not running at $(SERVER). Run 'make server-bg' first."; exit 1)
	cargo run --release -p vectoria-cli -- \
		--server $(SERVER) --api-key $(API_KEY) \
		bench $(JUDGES) --mode all

$(JUDGES): esci-judges

# ── Webstore ───────────────────────────────────────────────────────────────

webstore:
	@echo "Webstore: http://localhost:$(WEBSTORE_PORT)"
	@echo "Vectoria: $(SERVER)  |  API key: $(API_KEY)"
	python3 -m http.server $(WEBSTORE_PORT) --directory examples/webstore

# ── Cleanup ────────────────────────────────────────────────────────────────

clean:
	rm -f $(PRODUCTS) $(EXAMPLES) $(JUDGES)
	rm -rf $(WANDS_DIR)
	@echo "Data files removed. Server still running if started with 'make server-bg'."

# ── WANDS dataset ──────────────────────────────────────────────────────────
# Wayfair WANDS dataset — open license (CC BY-SA 4.0)
# https://github.com/wayfair/WANDS

$(WANDS_DIR):
	mkdir -p $(WANDS_DIR)

$(WANDS_PRODUCTS): | $(WANDS_DIR)
	@echo "Downloading WANDS products..."
	curl -L --progress-bar -o $@ "$(WANDS_BASE_URL)/product.csv"

$(WANDS_QUERIES): | $(WANDS_DIR)
	@echo "Downloading WANDS queries..."
	curl -L --progress-bar -o $@ "$(WANDS_BASE_URL)/query.csv"

$(WANDS_LABELS): | $(WANDS_DIR)
	@echo "Downloading WANDS labels..."
	curl -L --progress-bar -o $@ "$(WANDS_BASE_URL)/label.csv"

wands-download: $(WANDS_PRODUCTS) $(WANDS_QUERIES) $(WANDS_LABELS)

wands-import: wands-download
	@curl -sf $(SERVER)/health >/dev/null 2>&1 || \
		(echo "Error: server not running at $(SERVER). Run 'make server-bg' first."; exit 1)
	python3 scripts/wands_import.py \
		--products $(WANDS_PRODUCTS) \
		--server $(SERVER) \
		--api-key $(API_KEY) \
		--max-products $(WANDS_MAX)

wands-judges: wands-download
	python3 scripts/wands_judges.py \
		--queries $(WANDS_QUERIES) \
		--labels $(WANDS_LABELS) \
		--output $(WANDS_JUDGES)
	@echo "Judges written to $(WANDS_JUDGES)"

wands-bench: $(WANDS_JUDGES)
	@curl -sf $(SERVER)/health >/dev/null 2>&1 || \
		(echo "Error: server not running at $(SERVER). Run 'make server-bg' first."; exit 1)
	cargo run --release -p vectoria-cli -- \
		--server $(SERVER) --api-key $(API_KEY) \
		bench $(WANDS_JUDGES) --mode all

$(WANDS_JUDGES): wands-judges

# ── Release / publish ──────────────────────────────────────────────────────
#
# Standard release sequence:
#
#   1. make test                     confirm all 57 tests pass
#   2. make publish-dry-run          verify vectoria-core passes crates.io preflight
#   3. make tag NEW_VERSION=x.y.z    bump versions, commit, push branch + signed tag
#   4. make publish                  upload vectoria-core to crates.io
#
# Prerequisites:
#   - git remote 'origin' must be set and you must have push access
#   - GPG signing key configured for git (used by 'tag' target: git tag -s)
#   - cargo login, or CARGO_REGISTRY_TOKEN set (used by 'publish' target)
#
# What gets version-bumped by 'make tag':
#   Cargo.toml  README.md  docs/api.md  docs/quickstart.md
#   website/index.html  website/api.html  website/quickstart.html
#
# Only vectoria-core is published to crates.io (it is the embeddable library).
# vectoria-server and vectoria-cli are distributed as binaries / Docker images only.

# Print the current version from Cargo.toml (used internally by other targets).
version:
	@echo "$(VERSION)"

# Preflight check: verifies the crate package is valid without uploading.
# Catches missing files, bad metadata, or dependency issues before the real publish.
#
# --allow-dirty: cargo's git2 library reports false-positive dirty state due to
# stale index mtimes after a commit even when 'git status' shows a clean tree.
# This flag is safe here — actual file changes are caught by 'make test' first.
publish-dry-run:
	cargo publish -p vectoria-core --dry-run --allow-dirty

# Upload vectoria-core to crates.io.
#
# Prerequisites:
#   Run 'cargo login' once, or set CARGO_REGISTRY_TOKEN in the environment.
#   Run 'make publish-dry-run' and 'make tag' first.
#
# Only vectoria-core is published. The server and CLI are not on crates.io.
publish:
	@echo "Publishing vectoria-core v$(VERSION) to crates.io..."
	cargo publish -p vectoria-core --allow-dirty
	@echo "Published. https://crates.io/crates/vectoria-core"

# Bump version strings, commit, push branch, create a signed tag, push tag.
#
# Usage:
#   make tag NEW_VERSION=0.1.8
#
# Steps performed:
#   1. Reads current version from website/index.html (regex: first semver found)
#   2. Replaces OLD → NEW in Cargo.toml + 6 doc/web files (sed in-place)
#   3. Stages all changes with 'git add -A' and commits "chore: release vX.Y.Z"
#   4. Pushes current branch to origin
#   5. Creates a GPG-signed tag vX.Y.Z and pushes it to origin
#
# After this: run 'make publish' to upload vectoria-core to crates.io.
tag:
	@test -n "$(NEW_VERSION)" || { echo "ERROR: specify version: make tag NEW_VERSION=x.y.z"; exit 1; }
	@PREV=$$(grep -ohE '[0-9]+\.[0-9]+\.[0-9]+' website/index.html | head -1); \
	echo "Bumping $$PREV → $(NEW_VERSION) in Cargo.toml and docs..."; \
	sed -i '' "s/$$PREV/$(NEW_VERSION)/g" \
		Cargo.toml \
		README.md \
		docs/api.md \
		docs/quickstart.md \
		website/api.html \
		website/quickstart.html \
		website/index.html
	@echo "Committing release v$(NEW_VERSION)..."
	git add -A
	git commit -m "chore: release v$(NEW_VERSION)"
	@echo "Pushing branch to origin..."
	git push origin HEAD
	@echo "Tagging v$(NEW_VERSION)..."
	git tag -s "v$(NEW_VERSION)" -m "vectoria v$(NEW_VERSION)"
	git push origin "v$(NEW_VERSION)"
	@echo "Done. Branch and tag v$(NEW_VERSION) pushed."
