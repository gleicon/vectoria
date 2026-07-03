#!/usr/bin/env bash
# deploy/deploy.sh — Incremental deploy for vectoriasearch.com
#
# Run on every update: syncs files, rebuilds images, restarts services.
# Does NOT touch the existing minimidia app (port 3000).
#
# Usage:
#   ./deploy/deploy.sh               # full deploy
#   ./deploy/deploy.sh --site-only   # sync website + webstore, reload nginx only
#   ./deploy/deploy.sh --algolia     # redeploy vectoria-algolia only

set -euo pipefail

REMOTE_HOST="169.150.1.130"
REMOTE_USER="ubuntu"
SSH_KEY="$HOME/.ssh/id_rsa_mgc_saas_apps"
APP_DIR="/opt/apps/vectoria"

SSH="ssh -i $SSH_KEY -o StrictHostKeyChecking=no $REMOTE_USER@$REMOTE_HOST"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

MODE="full"
[[ "${1:-}" == "--site-only"  ]] && MODE="site"
[[ "${1:-}" == "--algolia"    ]] && MODE="algolia"

echo "==> Deploying vectoria ($MODE) to $REMOTE_HOST"

# ── Sync static files ──────────────────────────────────────────────────────
if [[ "$MODE" == "full" || "$MODE" == "site" ]]; then
  echo "[sync] website..."
  rsync -az --checksum --delete \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
    "$REPO_ROOT/website/" \
    "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/website/"

  echo "[sync] webstore..."
  rsync -az --checksum --delete \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
    "$REPO_ROOT/examples/webstore/" \
    "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/webstore/"

  echo "[sync] deploy config..."
  rsync -az --checksum \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
    "$REPO_ROOT/deploy/" \
    "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/deploy/"

  # Regenerate webstore config.js from server's .env
  $SSH "
    VECTORIA_API_KEY=\$(grep ^VECTORIA_API_KEY $APP_DIR/.env | cut -d= -f2 | tr -d '\"')
    echo \"window.VECTORIA_API_KEY = '\${VECTORIA_API_KEY}';\" > $APP_DIR/webstore/config.js
  "
fi

# ── Sync Rust source (needed for docker build) ─────────────────────────────
if [[ "$MODE" == "full" ]]; then
  echo "[sync] rust source..."
  rsync -az --checksum --delete \
    --exclude='.git/' \
    --exclude='target/' \
    --exclude='.claude/' \
    --exclude='deploy/.env' \
    --exclude='*.env' \
    --exclude='webstore/' \
    --exclude='website/' \
    --exclude='data/' \
    --exclude='logs/' \
    --exclude='vectoria-algolia/' \
    -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
    "$REPO_ROOT/" \
    "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/"
fi

# ── Update nginx config if changed ────────────────────────────────────────
if [[ "$MODE" == "full" || "$MODE" == "site" ]]; then
  echo "[nginx] checking config..."
  $SSH "
    if ! diff -q $APP_DIR/deploy/nginx/vectoriasearch.com \
                 /etc/nginx/sites-available/vectoriasearch.com >/dev/null 2>&1; then
      echo '  updating nginx config...'
      sudo cp $APP_DIR/deploy/nginx/vectoriasearch.com /etc/nginx/sites-available/vectoriasearch.com
      sudo nginx -t && sudo nginx -s reload
      echo '  nginx reloaded.'
    else
      echo '  nginx config unchanged.'
    fi
  "
fi

# ── Rebuild + restart vectoria-server ─────────────────────────────────────
if [[ "$MODE" == "full" ]]; then
  echo "[docker] rebuilding vectoria-server..."
  $SSH "
    cd $APP_DIR
    sudo docker compose -f deploy/docker-compose.prod.yml --env-file .env \
      up -d --build --remove-orphans
    echo '  vectoria-server restarted.'
  "
fi

# ── Redeploy vectoria-algolia ──────────────────────────────────────────────
if [[ "$MODE" == "full" || "$MODE" == "algolia" ]]; then
  echo "[algolia] updating..."
  $SSH "
    git -C $APP_DIR/vectoria-algolia pull --ff-only
    cd $APP_DIR/vectoria-algolia
    sudo docker compose \
      -f docker-compose.yml \
      -f $APP_DIR/deploy/docker-compose.algolia-override.yml \
      up -d --build --remove-orphans search loader
    echo '  vectoria-algolia restarted.'
  "
fi

# ── Health checks ──────────────────────────────────────────────────────────
echo "[health] checking services..."
$SSH "
  # vectoria-server
  for i in \$(seq 1 12); do
    curl -sf http://127.0.0.1:7700/health >/dev/null 2>&1 && break
    sleep 5
  done
  STATUS=\$(curl -sf http://127.0.0.1:7700/health 2>/dev/null || echo 'DOWN')
  echo \"  vectoria-server: \$STATUS\"

  # vectoria-algolia
  for i in \$(seq 1 6); do
    curl -sf http://127.0.0.1:8108/health >/dev/null 2>&1 && break
    sleep 5
  done
  ALG_STATUS=\$(curl -sf http://127.0.0.1:8108/health 2>/dev/null || echo 'DOWN')
  echo \"  vectoria-algolia: \$ALG_STATUS\"
"

echo ""
echo "Deploy complete."
echo "  https://vectoriasearch.com"
echo "  https://demo.vectoriasearch.com"
echo "  https://a.vectoriasearch.com"
