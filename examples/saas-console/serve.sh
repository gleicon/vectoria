#!/usr/bin/env bash
# Serve the saas-console from this directory.
# ES module imports require HTTP (file:// won't work due to CORS).
set -euo pipefail
PORT="${PORT:-8889}"
DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Vectoria SaaS Console → http://localhost:${PORT}"
python3 -m http.server "$PORT" --directory "$DIR"
