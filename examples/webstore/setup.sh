#!/usr/bin/env bash
# Download Amazon ESCI data and import into a running Vectoria server.
#
# Usage:
#   ./setup.sh [--server URL] [--api-key KEY] [--max-products N] [--locale LOCALE]
#
# Defaults:
#   server       http://localhost:7700
#   api-key      read from vectoria.toml in current directory, or prompt
#   max-products 5000  (set 0 for all ~1.8M products — takes hours)
#   locale       us

set -euo pipefail

SERVER="http://localhost:7700"
API_KEY=""
MAX_PRODUCTS=5000
LOCALE="us"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/vectoria/esci"

PRODUCTS_URL="https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_products.parquet"
EXAMPLES_URL="https://media.githubusercontent.com/media/amazon-science/esci-data/main/shopping_queries_dataset/shopping_queries_dataset_examples.parquet"

# Parse args
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

# Resolve API key from vectoria.toml if not provided
if [[ -z "$API_KEY" ]]; then
  if [[ -f vectoria.toml ]]; then
    API_KEY=$(grep -E '^api_key\s*=' vectoria.toml | head -1 | sed 's/.*=\s*"\?\([^"]*\)"\?.*/\1/' | tr -d '[:space:]')
  fi
fi
if [[ -z "$API_KEY" ]]; then
  read -rp "Vectoria API key (from server startup log): " API_KEY
fi

# Require the vectoria CLI
if ! command -v vectoria &>/dev/null; then
  echo "Error: 'vectoria' CLI not found. Build it first:"
  echo "  cargo build --release -p vectoria-cli"
  echo "  export PATH=\"\$PATH:\$(pwd)/target/release\""
  exit 1
fi

# Check server is up
echo "Checking server at $SERVER..."
if ! curl -sf "$SERVER/health" >/dev/null; then
  echo "Error: server not responding at $SERVER"
  echo "Start it with: vectoria-server"
  exit 1
fi
echo "  Server OK"

# Download data files
mkdir -p "$DATA_DIR"
PRODUCTS_FILE="$DATA_DIR/shopping_queries_dataset_products.parquet"
EXAMPLES_FILE="$DATA_DIR/shopping_queries_dataset_examples.parquet"

download() {
  local url="$1" dest="$2" label="$3"
  if [[ -f "$dest" ]]; then
    echo "  $label already downloaded at $dest"
    return
  fi
  echo "  Downloading $label (~$([ "$label" = "products" ] && echo "1.1 GB" || echo "68 MB"))..."
  if command -v curl &>/dev/null; then
    curl -L --progress-bar -o "$dest" "$url"
  elif command -v wget &>/dev/null; then
    wget -q --show-progress -O "$dest" "$url"
  else
    echo "Error: need curl or wget"
    exit 1
  fi
  echo "  $label saved to $dest"
}

echo ""
echo "==> Downloading ESCI dataset to $DATA_DIR"
download "$PRODUCTS_URL" "$PRODUCTS_FILE" "products"
download "$EXAMPLES_URL" "$EXAMPLES_FILE" "examples"

# Import
echo ""
echo "==> Importing products (locale=$LOCALE, max=$MAX_PRODUCTS)..."
MAX_FLAG=""
[[ "$MAX_PRODUCTS" -gt 0 ]] && MAX_FLAG="--max-products $MAX_PRODUCTS"

# shellcheck disable=SC2086
vectoria --server "$SERVER" --api-key "$API_KEY" \
  esci "$PRODUCTS_FILE" "$EXAMPLES_FILE" \
  --import \
  --locale "$LOCALE" \
  $MAX_FLAG

echo ""
echo "==> Done. Open the webstore:"
echo "    python3 -m http.server 8080 --directory examples/webstore"
echo "    open http://localhost:8080"
echo ""
echo "    API key: $API_KEY"
