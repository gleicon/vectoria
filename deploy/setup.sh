#!/usr/bin/env bash
# deploy/setup.sh — First-time server setup for vectoriasearch.com
#
# Run ONCE on initial deployment. Safe to re-run (idempotent).
# Must be executed locally — SSH commands run against the remote server.
#
# Usage:
#   ./deploy/setup.sh
#
# Prerequisites:
#   - Server: 169.150.1.130 (ubuntu, ~/.ssh/id_rsa_mgc_saas_apps)
#   - /opt/apps/vectoria/.env must exist on the server (copy from .env.example)
#   - DNS: vectoriasearch.com, www, demo, algolia all pointing to the server

set -euo pipefail

REMOTE_HOST="169.150.1.130"
REMOTE_USER="ubuntu"
SSH_KEY="$HOME/.ssh/id_rsa_mgc_saas_apps"
APP_DIR="/opt/apps/vectoria"
ALGOLIA_REPO="https://github.com/gleicon/vectoria-algolia.git"

SSH="ssh -i $SSH_KEY -o StrictHostKeyChecking=no $REMOTE_USER@$REMOTE_HOST"

echo "==> Vectoria first-time setup on $REMOTE_HOST"
echo ""

# ── 1. Create directory structure ──────────────────────────────────────────
echo "[1/7] Creating directories..."
$SSH "sudo mkdir -p $APP_DIR/{website,webstore,deploy/nginx,data,logs} && \
      sudo chown -R ubuntu:ubuntu $APP_DIR"

# ── 2. Sync deploy artifacts ───────────────────────────────────────────────
echo "[2/7] Syncing deploy files..."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

rsync -az --checksum -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/deploy/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/deploy/"

rsync -az --checksum --delete -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/website/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/website/"

rsync -az --checksum --delete -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/examples/webstore/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/webstore/"

# ── 3. Clone vectoria-algolia ──────────────────────────────────────────────
echo "[3/7] Setting up vectoria-algolia..."
$SSH "
  if [ -d $APP_DIR/vectoria-algolia/.git ]; then
    echo '  algolia repo exists, pulling latest...'
    git -C $APP_DIR/vectoria-algolia pull --ff-only
  else
    echo '  cloning vectoria-algolia...'
    git clone $ALGOLIA_REPO $APP_DIR/vectoria-algolia
  fi
"

# ── 4. nginx configuration ─────────────────────────────────────────────────
echo "[4/7] Installing nginx config..."
$SSH "
  sudo cp $APP_DIR/deploy/nginx/vectoriasearch.com /etc/nginx/sites-available/vectoriasearch.com
  sudo ln -sf /etc/nginx/sites-available/vectoriasearch.com /etc/nginx/sites-enabled/vectoriasearch.com
  sudo nginx -t
"

# ── 5. TLS certificate (Let's Encrypt) ────────────────────────────────────
echo "[5/7] Issuing TLS certificate..."
$SSH "
  # Check if cert already exists
  if sudo test -f /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem; then
    echo '  certificate exists, skipping certbot...'
  else
    # Load email from .env
    CERTBOT_EMAIL=\$(grep CERTBOT_EMAIL $APP_DIR/.env 2>/dev/null | cut -d= -f2 | tr -d '\"')
    if [ -z \"\$CERTBOT_EMAIL\" ]; then
      echo 'ERROR: set CERTBOT_EMAIL in $APP_DIR/.env before running setup.sh'
      exit 1
    fi

    # Temporarily serve HTTP for the ACME challenge via nginx
    sudo nginx -s reload 2>/dev/null || sudo systemctl start nginx

    sudo certbot certonly --webroot -w /var/www/html \
      --non-interactive --agree-tos --email \"\$CERTBOT_EMAIL\" \
      -d vectoriasearch.com \
      -d www.vectoriasearch.com \
      -d demo.vectoriasearch.com \
      -d algolia.vectoriasearch.com

    echo '  certificate issued.'
  fi
"

# ── 6. Start services ──────────────────────────────────────────────────────
echo "[6/7] Starting Docker services..."

# vectoria-server
$SSH "
  cd $APP_DIR
  if [ ! -f .env ]; then
    echo 'ERROR: $APP_DIR/.env not found. Copy deploy/.env.example and fill in values.'
    exit 1
  fi
  sudo docker compose -f deploy/docker-compose.prod.yml --env-file .env up -d --build
  echo '  vectoria-server started.'
"

# vectoria-algolia
$SSH "
  cd $APP_DIR/vectoria-algolia
  sudo docker compose \
    -f docker-compose.yml \
    -f $APP_DIR/deploy/docker-compose.algolia-override.yml \
    up -d --build search loader
  echo '  vectoria-algolia started.'
"

# ── 7. Generate webstore config.js + reload nginx ─────────────────────────
echo "[7/7] Finalizing..."
$SSH "
  VECTORIA_API_KEY=\$(grep ^VECTORIA_API_KEY $APP_DIR/.env | cut -d= -f2 | tr -d '\"')
  echo \"window.VECTORIA_API_KEY = '\${VECTORIA_API_KEY}';\" > $APP_DIR/webstore/config.js
  echo '  webstore config.js written.'

  sudo nginx -s reload
  echo '  nginx reloaded.'
"

echo ""
echo "Setup complete."
echo ""
echo "  Website:  https://vectoriasearch.com"
echo "  Demo:     https://demo.vectoriasearch.com"
echo "  Algolia:  https://algolia.vectoriasearch.com"
echo ""
echo "Next: ensure DNS for www, demo, algolia subdomains all point to $REMOTE_HOST"
