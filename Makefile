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

.PHONY: help build server server-bg kill esci-download esci-import esci-judges bench webstore clean

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
	@echo "Data files removed. Server still running if started with 'make server-bg'."
