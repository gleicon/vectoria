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
#   - DNS: vectoriasearch.com, www, demo, algolia all pointing to the server
#   - Create /opt/apps/vectoria/.env on the server before running:
#       VECTORIA_API_KEY=<strong-random-key>
#       CERTBOT_EMAIL=<your-email>

set -euo pipefail

REMOTE_HOST="169.150.1.130"
REMOTE_USER="ubuntu"
SSH_KEY="$HOME/.ssh/id_rsa_mgc_saas_apps"
APP_DIR="/opt/apps/vectoria"
ALGOLIA_REPO="https://github.com/gleicon/vectoria-algolia.git"

SSH="ssh -i $SSH_KEY -o StrictHostKeyChecking=no $REMOTE_USER@$REMOTE_HOST"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "==> Vectoria first-time setup on $REMOTE_HOST"
echo ""

# ── 1. Create directory structure ──────────────────────────────────────────
echo "[1/7] Creating directories..."
$SSH "sudo mkdir -p $APP_DIR/{website,webstore,platform,deploy/nginx,data,logs} && \
      sudo chown -R ubuntu:ubuntu $APP_DIR"

# ── 2. Sync source + deploy artifacts ─────────────────────────────────────
echo "[2/7] Syncing source..."
rsync -az --checksum --delete \
  --exclude='.git/' \
  --exclude='target/' \
  --exclude='.claude/' \
  --exclude='*.env' \
  --exclude='webstore/' \
  --exclude='website/' \
  --exclude='data/' \
  --exclude='logs/' \
  --exclude='vectoria-algolia/' \
  -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/"

echo "[2b/7] Syncing website..."
rsync -az --checksum --delete \
  -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/website/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/website/"

echo "[2c/7] Syncing webstore..."
rsync -az --checksum --delete \
  -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/examples/webstore/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/webstore/"

echo "[2d/7] Syncing platform console..."
rsync -az --checksum --delete \
  -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
  "$REPO_ROOT/examples/saas-console/" \
  "$REMOTE_USER@$REMOTE_HOST:$APP_DIR/platform/"

# ── Preflight: .env required before docker + certbot steps ────────────────
if ! $SSH "test -f $APP_DIR/.env" 2>/dev/null; then
  echo ""
  echo "ERROR: $APP_DIR/.env not found."
  echo "  ssh -i $SSH_KEY $REMOTE_USER@$REMOTE_HOST"
  echo "  nano $APP_DIR/.env"
  echo "  # add: VECTORIA_API_KEY=<key>  CERTBOT_EMAIL=<email>"
  echo ""
  echo "Re-run: ./deploy/setup.sh"
  exit 1
fi

# ── 3. Clone vectoria-algolia ──────────────────────────────────────────────
echo "[3/7] Setting up vectoria-algolia..."
$SSH "
  if [ -d $APP_DIR/vectoria-algolia/.git ]; then
    echo '  pulling latest...'
    git -C $APP_DIR/vectoria-algolia pull --ff-only
  else
    echo '  cloning...'
    git clone $ALGOLIA_REPO $APP_DIR/vectoria-algolia
  fi
"

# ── 4. nginx — HTTP-only config first (needed for certbot ACME challenge) ──
echo "[4/7] Installing nginx config (HTTP phase)..."
$SSH "
  # Install HTTP-only config so nginx can serve the ACME challenge.
  # The full HTTPS config is installed after the cert exists.
  sudo tee /etc/nginx/sites-available/vectoriasearch.com > /dev/null <<'NGINX'
server {
    listen 80;
    listen [::]:80;
    server_name vectoriasearch.com www.vectoriasearch.com
                demo.vectoriasearch.com a.vectoriasearch.com;
    location /.well-known/acme-challenge/ { root /var/www/html; }
    location / { return 301 https://\$host\$request_uri; }
}
NGINX
  sudo ln -sf /etc/nginx/sites-available/vectoriasearch.com \
              /etc/nginx/sites-enabled/vectoriasearch.com
  sudo nginx -t && sudo nginx -s reload
  echo '  HTTP config loaded.'
"

# ── 5. TLS certificate (Let's Encrypt) ────────────────────────────────────
echo "[5/7] Issuing TLS certificate..."
$SSH "
  if sudo test -f /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem; then
    echo '  certificate exists, skipping certbot...'
  else
    CERTBOT_EMAIL=\$(grep ^CERTBOT_EMAIL $APP_DIR/.env | cut -d= -f2 | tr -d '\"')
    if [ -z \"\$CERTBOT_EMAIL\" ]; then
      echo 'ERROR: CERTBOT_EMAIL not set in $APP_DIR/.env'
      exit 1
    fi
    sudo certbot certonly --webroot -w /var/www/html \
      --non-interactive --agree-tos --email \"\$CERTBOT_EMAIL\" \
      -d vectoriasearch.com \
      -d www.vectoriasearch.com \
      -d demo.vectoriasearch.com \
      -d platform.vectoriasearch.com \
      -d a.vectoriasearch.com
    echo '  certificate issued.'
  fi

  # Now install the full HTTPS config (cert exists)
  sudo cp $APP_DIR/deploy/nginx/vectoriasearch.com \
          /etc/nginx/sites-available/vectoriasearch.com
  sudo nginx -t && sudo nginx -s reload
  echo '  HTTPS config loaded.'
"

# ── 6. Start services ──────────────────────────────────────────────────────
echo "[6/7] Starting Docker services..."

$SSH "
  cd $APP_DIR
  sudo docker compose -f deploy/docker-compose.prod.yml --env-file .env up -d --build
  echo '  vectoria-server started.'
"

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
  echo \"window.VECTORIA_DEFAULT_URL = 'https://demo.vectoriasearch.com';\" > $APP_DIR/platform/js/config.js
  echo '  platform config.js written.'
  sudo nginx -s reload
  echo '  nginx reloaded.'

  # Load demo products into vectoria-server (wait for it to be ready first)
  echo '  loading demo products into vectoria-server...'
  for i in \$(seq 1 12); do
    curl -sf http://127.0.0.1:7700/health >/dev/null 2>&1 && break
    sleep 5
  done
  python3 - <<'PYEOF'
import json, urllib.request, urllib.error
with open('$APP_DIR/vectoria-algolia/scripts/batch.json') as f:
    data = json.load(f)
SERVER = 'http://127.0.0.1:7700'
API_KEY = open('$APP_DIR/.env').read()
API_KEY = next((l.split('=',1)[1].strip().strip('\"') for l in API_KEY.splitlines() if l.startswith('VECTORIA_API_KEY')), 'vectoria-demo')
ok = 0
for req in data['requests']:
    body = req['body']
    text = ' '.join(filter(None, [body.get('title',''), body.get('brand',''), body.get('category',''), body.get('description','')]))
    product = json.dumps({'id': body['objectID'], 'text': text, 'metadata': body}).encode()
    r = urllib.request.Request(f'{SERVER}/products', data=product, headers={'Content-Type': 'application/json', 'Authorization': f'Bearer {API_KEY}'}, method='POST')
    try:
        urllib.request.urlopen(r); ok += 1
    except Exception as e:
        print(f'  error indexing {body.get("objectID","?")}: {e}')
print(f'  {ok} products indexed into vectoria-server.')
PYEOF
"

echo ""
echo "Setup complete."
echo ""
echo "  Website:  https://vectoriasearch.com"
echo "  Demo:     https://demo.vectoriasearch.com"
echo "  Platform: https://platform.vectoriasearch.com"
echo "  Algolia:  https://a.vectoriasearch.com"
echo ""
echo "Next: ensure DNS for www, demo, platform, algolia subdomains all point to $REMOTE_HOST"
