# Vectoria — Deployment Guide

## Infrastructure

| Component | Description |
|-----------|-------------|
| Server | `169.150.1.130` (ubuntu, MGC cloud) |
| SSH key | `~/.ssh/id_rsa_mgc_saas_apps` |
| App root | `/opt/apps/vectoria/` |
| vectoria-algolia repo | `/opt/apps/vectoria/vectoria-algolia/` |

## Domains

| Domain | Serves |
|--------|--------|
| `vectoriasearch.com` | Marketing website (static HTML) |
| `demo.vectoriasearch.com` | Demo store + vectoria-server API proxy |
| `a.vectoriasearch.com` | Algolia-compatible adapter (vectoria-algolia) |

## Services

| Service | Port | Container |
|---------|------|-----------|
| vectoria-server | `127.0.0.1:7700` | `deploy/docker-compose.prod.yml` |
| vectoria-algolia | `127.0.0.1:8108` | `vectoria-algolia/docker-compose.yml` + override |
| nginx | 80/443 | host |

vectoria-algolia uses `network_mode: host` so it can resolve DNS on MGC cloud.
The override (`deploy/docker-compose.algolia-override.yml`) enables this and binds to `127.0.0.1` only.

## First-time setup

Prerequisites:
- DNS A records for `vectoriasearch.com`, `www`, `demo`, `a` all pointing to `169.150.1.130`
- Create `/opt/apps/vectoria/.env` on the server:

```
VECTORIA_API_KEY=vectoria-demo
CERTBOT_EMAIL=your@email.com
```

Then run:

```bash
./deploy/setup.sh
```

This:
1. Creates directory structure on the server
2. Syncs source code (excludes `.git`, `target`, server-only dirs)
3. Syncs `website/` and `examples/webstore/` separately
4. Clones `vectoria-algolia` repo
5. Installs HTTP-only nginx config (for ACME challenge)
6. Runs certbot (Let's Encrypt TLS)
7. Installs full HTTPS nginx config
8. Starts all Docker services
9. Loads product data via the loader container
10. Generates `webstore/config.js` with API key

## Incremental deploy

```bash
./deploy/deploy.sh               # full deploy (rebuild + restart everything)
./deploy/deploy.sh --site-only   # website + webstore only, no Docker rebuild
./deploy/deploy.sh --algolia     # vectoria-algolia only
```

## Loading product data

Products are loaded once by the `loader` container at first deploy. To reload:

```bash
ssh -i ~/.ssh/id_rsa_mgc_saas_apps ubuntu@169.150.1.130 "
  cd /opt/apps/vectoria/vectoria-algolia
  sudo docker compose \
    -f docker-compose.yml \
    -f /opt/apps/vectoria/deploy/docker-compose.algolia-override.yml \
    run --rm loader
"
```

## API key

The demo site uses the key `vectoria-demo` (bundled in `webstore/config.js`).
The server accepts it because `/opt/apps/vectoria/.env` sets `VECTORIA_API_KEY=vectoria-demo`.

To use a private key, change both the `.env` file and redeploy with `--site-only`
(which regenerates `config.js`).

## Cert renewal

Certbot auto-renews via systemd timer. To renew manually:

```bash
ssh -i ~/.ssh/id_rsa_mgc_saas_apps ubuntu@169.150.1.130 \
  "sudo certbot renew && sudo nginx -s reload"
```

## Directory layout on server

```
/opt/apps/vectoria/
├── .env                        # secrets (not in repo)
├── Dockerfile
├── Cargo.toml / Cargo.lock
├── vectoria-core/
├── vectoria-server/
├── vectoria-cli/
├── deploy/
│   ├── docker-compose.prod.yml
│   ├── docker-compose.algolia-override.yml
│   ├── nginx/vectoriasearch.com
│   └── setup.sh / deploy.sh
├── website/                    # marketing site (synced from repo website/)
├── webstore/                   # demo store (synced from repo examples/webstore/)
│   └── config.js               # generated; not in repo
├── data/                       # vectoria-server SQLite DB
├── logs/
└── vectoria-algolia/           # cloned from github.com/gleicon/vectoria-algolia
```

## Rust version

Dockerfile uses `rust:1-slim-bookworm` (always latest stable 1.x).

The `edgestore-1.0.4` crate has a type error on Rust ≥ 1.88 (`as_raw_fd()` returns `i32`
not `Result`). The Dockerfile patches the crate source before building.

## Troubleshooting

**demo.vectoriasearch.com returns 404**
The webstore dir was deleted by a full rsync. Restore it:
```bash
rsync -az --checksum --delete \
  -e "ssh -i ~/.ssh/id_rsa_mgc_saas_apps -o StrictHostKeyChecking=no" \
  examples/webstore/ ubuntu@169.150.1.130:/opt/apps/vectoria/webstore/

ssh -i ~/.ssh/id_rsa_mgc_saas_apps ubuntu@169.150.1.130 \
  "echo \"window.VECTORIA_API_KEY = 'vectoria-demo';\" > /opt/apps/vectoria/webstore/config.js"
```

**vectoria-algolia can't reach DNS**
Already fixed via `network_mode: host`. If symptoms recur:
```bash
ssh ... "sudo docker network prune -f && sudo docker compose ... up -d search"
```

**Port 8108 already in use**
Stale network namespaces. Fix:
```bash
ssh ... "sudo docker network prune -f"
```
