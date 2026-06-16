#!/usr/bin/env bash
# Download Amazon ESCI data and import into a running Vectoria server.
#
# IMPORTANT: The Amazon ESCI dataset requires a separate license agreement.
# See: https://github.com/amazon-science/esci-data
#
# Usage:
#   ./setup.sh [--server URL] [--api-key KEY] [--max-products N] [--locale LOCALE]
#
# Defaults:
#   server       http://localhost:7700
#   api-key      read from vectoria.toml, or prompted
#   max-products 5000  (0 = all ~1.8M — takes hours)
#   locale       us

set -euo pipefail

SERVER="http://localhost:7700"
API_KEY=""
MAX_PRODUCTS=5000
LOCALE="us"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/vectoria/esci"

PRODUCTS_URL="https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_products.parquet"
EXAMPLES_URL="https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_examples.parquet"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --server)       SERVER="$2";       shift 2 ;;
    --api-key)      API_KEY="$2";      shift 2 ;;
    --max-products) MAX_PRODUCTS="$2"; shift 2 ;;
    --locale)       LOCALE="$2";       shift 2 ;;
    --data-dir)     DATA_DIR="$2";     shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

if [[ -z "$API_KEY" ]]; then
  if [[ -f vectoria.toml ]]; then
    API_KEY=$(grep -E '^api_key\s*=' vectoria.toml | head -1 | sed 's/.*=\s*"\?\([^"]*\)"\?.*/\1/' | tr -d '[:space:]') || true
  fi
fi
if [[ -z "$API_KEY" ]]; then
  read -rp "Vectoria API key (from server startup log): " API_KEY
fi

echo "Checking server at $SERVER..."
if ! curl -sf "$SERVER/health" >/dev/null; then
  echo "Error: server not responding at $SERVER"
  echo "Start it with: cargo run -p vectoria-server"
  exit 1
fi
echo "  Server OK"

mkdir -p "$DATA_DIR"
PRODUCTS_FILE="$DATA_DIR/shopping_queries_dataset_products.parquet"
EXAMPLES_FILE="$DATA_DIR/shopping_queries_dataset_examples.parquet"

download() {
  local url="$1" dest="$2" label="$3"
  if [[ -f "$dest" ]]; then
    echo "  $label already at $dest"
    return
  fi
  echo "  Downloading $label..."
  if command -v curl &>/dev/null; then
    curl -L --progress-bar -o "$dest" "$url"
  elif command -v wget &>/dev/null; then
    wget -q --show-progress -O "$dest" "$url"
  else
    echo "Error: need curl or wget"; exit 1
  fi
}

echo ""
echo "==> Downloading ESCI dataset to $DATA_DIR"
download "$PRODUCTS_URL" "$PRODUCTS_FILE" "products (~1.1 GB)"
download "$EXAMPLES_URL" "$EXAMPLES_FILE" "examples (~68 MB)"

echo ""
echo "==> Importing products (locale=$LOCALE, max=$MAX_PRODUCTS)..."
MAX_FLAG=""
[[ "$MAX_PRODUCTS" -gt 0 ]] && MAX_FLAG="--max-products $MAX_PRODUCTS"

# shellcheck disable=SC2086
cargo run --example esci_import -p vectoria-cli -- \
  "$PRODUCTS_FILE" "$EXAMPLES_FILE" \
  --import \
  --locale "$LOCALE" \
  --server "$SERVER" \
  --api-key "$API_KEY" \
  $MAX_FLAG

echo ""
echo "==> Done. Open the webstore:"
echo "    python3 -m http.server 8080 --directory examples/webstore"
echo "    open http://localhost:8080"
echo "    API key: $API_KEY"
