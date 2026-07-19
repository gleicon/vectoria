#!/usr/bin/env bash
# Start Vectoria, import ESCI data, and serve the admin panel — one command.
# Usage: ./scripts/admin-demo.sh [api-key]
# Requires: cargo, python3, curl

set -e

REPO="$(cd "$(dirname "$0")/.." && pwd)"
API_KEY="${1:-vectoria-demo}"
SERVER="http://localhost:7700"
PANEL_PORT=8888

cd "$REPO"

# ── 1. kill any old server ────────────────────────────────────────────────
echo "→ Stopping any running server..."
lsof -ti:7700 | xargs kill -9 2>/dev/null || true
rm -f /tmp/vectoria.pid

# ── 2. build ──────────────────────────────────────────────────────────────
echo "→ Building server..."
cargo build --release -p vectoria-server -p vectoria-cli 2>&1 | grep -E "^(error|warning\[|Compiling|Finished)" || true

# ── 3. start server ───────────────────────────────────────────────────────
echo "→ Starting server (key: $API_KEY)..."
VECTORIA_API_KEY="$API_KEY" \
  nohup ./target/release/vectoria-server > /tmp/vectoria.log 2>&1 &
echo $! > /tmp/vectoria.pid

echo -n "   Waiting for server"
for i in $(seq 1 30); do
  curl -sf "$SERVER/health" >/dev/null 2>&1 && echo " ready." && break
  echo -n "."
  sleep 1
done
curl -sf "$SERVER/health" >/dev/null 2>&1 || { echo; echo "ERROR: server failed to start. Check: tail -f /tmp/vectoria.log"; exit 1; }

# ── 4. import ESCI data ───────────────────────────────────────────────────
echo "→ Importing ESCI data (downloads ~1.1 GB on first run, then cached)..."
echo "  Amazon ESCI license: https://github.com/amazon-science/esci-data"
make esci-import API_KEY="$API_KEY" SERVER="$SERVER"

# ── 5. serve admin panel ──────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Admin panel : http://localhost:$PANEL_PORT/vectoria-admin.html"
echo "  Server      : $SERVER"
echo "  API key     : $API_KEY"
echo "  Server log  : tail -f /tmp/vectoria.log"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  Try searching: 'running shoes', 'laptop', 'coffee maker'"
echo ""
echo "  Ctrl+C to stop the panel (server keeps running)"
echo "  kill \$(cat /tmp/vectoria.pid) to stop the server"
echo ""

python3 -m http.server "$PANEL_PORT" --directory examples/admin-panel
