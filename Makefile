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

.PHONY: help build server server-bg kill esci-download esci-import esci-judges bench webstore clean \
        publish publish-dry-run tag version \
        wands-download wands-import wands-judges wands-bench

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
	@echo "  Release:"
	@echo "    version             print current version from Cargo.toml"
	@echo "    publish-dry-run     verify vectoria-core is ready for crates.io"
	@echo "    publish             publish vectoria-core to crates.io"
	@echo "    tag NEW_VERSION=x.y.z  bump Cargo.toml + docs, commit, push branch + tag"
	@echo ""
	@echo "  Variables (override on command line):"
	@echo "    MAX_PRODUCTS=$(MAX_PRODUCTS)   LOCALE=$(LOCALE)   WEBSTORE_PORT=$(WEBSTORE_PORT)"
	@echo "    SERVER=$(SERVER)   API_KEY=$(API_KEY)"
	@echo ""
	@echo "  ESCI dataset: Amazon license required — https://github.com/amazon-science/esci-data"

# ── Build ──────────────────────────────────────────────────────────────────

build:
	cargo build --release -p vectoria-server -p vectoria-cli

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

version:
	@echo "$(VERSION)"

# Dry-run publish: verifies the crate is ready without uploading.
# --allow-dirty: cargo's git2 library can report false-positive dirty state
# due to stale index mtimes after a commit; git status shows a clean tree.
publish-dry-run:
	cargo publish -p vectoria-core --dry-run --allow-dirty

# Publish vectoria-core to crates.io.
# Requires: cargo login (or CARGO_REGISTRY_TOKEN env var).
publish:
	@echo "Publishing vectoria-core v$(VERSION) to crates.io..."
	cargo publish -p vectoria-core --allow-dirty
	@echo "Published. https://crates.io/crates/vectoria-core"

# Create and push a release tag. Triggers the GitHub Actions release workflow.
# Requires NEW_VERSION argument. Updates Cargo.toml, all doc/web version strings,
# commits everything, pushes branch, then creates and pushes the tag.
# Usage: make tag NEW_VERSION=0.1.5
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
