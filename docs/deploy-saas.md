# Vectoria SaaS Deployment Guide

Deploys Vectoria as a micro-SaaS on your VPS at `169.150.1.130`, serving the admin console at `platform.vectoriasearch.com` and the API at `demo.vectoriasearch.com`.

## Architecture

```
Browser
  │
  ├── platform.vectoriasearch.com  → nginx → /opt/apps/vectoria/platform/ (static)
  │                                   saas-console JS calls demo directly
  │
  └── demo.vectoriasearch.com      → nginx → 127.0.0.1:7700 (vectoria-server)
                                      handles admin + all tenant API keys
```

- **Single vectoria-server** handles all tenants. No mode flag needed.
- **Tenant isolation** is by named index (`/indexes/{tenant-name}/`) scoped to their API key.
- **Data persistence** is automatic: the `vectoria-data` Docker volume stores main store, per-tenant HNSW indexes, and the tenant registry all under `/data/`.
- **Platform console** is a static HTML/JS app; no backend for the console itself.

## Prerequisites

- DNS A records for `vectoriasearch.com`, `www`, `demo`, `platform`, `a` all pointing to `169.150.1.130`.
- SSH key at `~/.ssh/id_rsa_mgc_saas_apps`.
- `/opt/apps/vectoria/.env` on the server with:
  ```
  VECTORIA_API_KEY=<strong-random-key>
  CERTBOT_EMAIL=<your-email>
  VECTORIA_RATE_LIMIT_PER_SECOND=30
  ```

## First-Time Setup

If this is a fresh server (no existing cert):

```bash
./deploy/setup.sh
```

This creates directories, syncs files, issues the TLS cert for all subdomains including `platform`, starts Docker services, and writes both `config.js` files.

### Adding `platform` to an existing cert

If the server already has a cert for the other domains:

```bash
ssh -i ~/.ssh/id_rsa_mgc_saas_apps ubuntu@169.150.1.130
sudo certbot certonly --webroot -w /var/www/html \
  --expand --non-interactive --agree-tos \
  -d vectoriasearch.com \
  -d www.vectoriasearch.com \
  -d demo.vectoriasearch.com \
  -d platform.vectoriasearch.com \
  -d a.vectoriasearch.com
sudo nginx -t && sudo nginx -s reload
```

Then sync the updated nginx config and platform files:

```bash
./deploy/deploy.sh --platform
```

## Deploying Updates

| Command | What it does |
|---------|--------------|
| `./deploy/deploy.sh` | Full deploy: sync everything, rebuild server image, restart |
| `./deploy/deploy.sh --platform` | Sync platform console only, reload nginx |
| `./deploy/deploy.sh --site-only` | Sync website + webstore + platform, reload nginx |
| `./deploy/deploy.sh --algolia` | Redeploy algolia adapter only |

## SaaS Stack (docker-compose)

The SaaS deploy uses `docker-compose.prod.yml` plus the overlay:

```bash
cd /opt/apps/vectoria
sudo docker compose \
  -f deploy/docker-compose.prod.yml \
  -f deploy/docker-compose.saas.yml \
  --env-file .env \
  up -d --build
```

The overlay adds `VECTORIA_RATE_LIMIT_PER_SECOND` (default 30 req/sec per IP). Everything else is inherited from the prod compose.

The `deploy.sh --platform` and `setup.sh` scripts still use `docker-compose.prod.yml` alone for simplicity. Switch to the overlay command if you need the rate limit active.

## Tenant Provisioning (Manual Flow)

This is the manual micro-SaaS workflow — no payment integration yet.

### 1. Create a tenant

```bash
curl -X POST https://demo.vectoriasearch.com/admin/tenants \
  -H "Authorization: Bearer $VECTORIA_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "acme-store"}'
```

Response (save the key — it's shown once):
```json
{
  "name": "acme-store",
  "api_key": "vtk_...",
  "created_at": "2026-07-16T00:00:00Z",
  "index": "acme-store"
}
```

### 2. Send credentials to the customer

Give them:
- **Console URL:** `https://platform.vectoriasearch.com`
- **Server URL:** `https://demo.vectoriasearch.com` (pre-filled in the console)
- **API key:** the `vtk_…` key from step 1
- **Index name:** their tenant name (e.g., `acme-store`)

### 3. Customer logs in

The customer opens `https://platform.vectoriasearch.com`, enters their API key, and their index name. The server URL is already pre-filled. They land on the tenant dashboard and can start indexing products.

### 4. Rotate a key if needed

```bash
curl -X POST https://demo.vectoriasearch.com/admin/tenants/acme-store/rotate-key \
  -H "Authorization: Bearer $VECTORIA_API_KEY"
```

### 5. Delete a tenant

```bash
curl -X DELETE https://demo.vectoriasearch.com/admin/tenants/acme-store \
  -H "Authorization: Bearer $VECTORIA_API_KEY"
```

Returns `204 No Content`. The named index is deleted; the data is gone.

## Plan Limits (coming soon)

The `TenantQuota` struct is planned but not yet implemented. Current state:

- All tenants have unlimited products and no per-tenant rate limit.
- Global rate limit applies to all traffic per IP (`VECTORIA_RATE_LIMIT_PER_SECOND`).
- When quota is implemented, `PUT /admin/tenants/{name}/quota` will set `max_products` and `rate_limit_per_second`. `null` fields mean unlimited (safe default for single-tenant operators).

In the meantime, enforce limits manually: delete the tenant if they exceed the agreed plan, or block their key.

## Payment (manual, no integration yet)

1. Send a PayPal request or invoice link.
2. Confirm payment received.
3. Run the `POST /admin/tenants` command above.
4. Send the customer their credentials.

Stripe integration can be added later as a webhook that calls `POST /admin/tenants` automatically.

## Health Checks

```bash
# vectoria-server
curl https://demo.vectoriasearch.com/health

# platform console (nginx serving static files)
curl -I https://platform.vectoriasearch.com

# list tenants (admin)
curl https://demo.vectoriasearch.com/admin/tenants \
  -H "Authorization: Bearer $VECTORIA_API_KEY"
```

## Data Backup

The `vectoria-data` Docker volume contains everything:

```bash
# On the server
docker run --rm \
  -v vectoria-data:/data \
  -v /opt/apps/vectoria/backups:/backup \
  alpine tar czf /backup/vectoria-data-$(date +%Y%m%d).tar.gz /data
```

Schedule this with cron. Keep at least 7 days of backups before deploying major changes.
