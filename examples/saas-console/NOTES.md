# Vectoria SaaS Console

Self-contained browser admin UI for Vectoria's multi-tenant search API.
No build step, no framework dependencies — static files served by any HTTP server or nginx.

---

## File map

```
examples/saas-console/
├── index.html          Login + role detection → routes to admin.html or tenant.html
├── admin.html          Tenant CRUD, system index list (admin key only)
├── tenant.html         Search, overrides, upload (tenant key or admin-via-tenant-detail)
├── tenant-detail.html  Per-tenant index management, load-data help (admin key only)
├── api-guide.html      Interactive API reference with Shell/JS/Python/Go examples
├── js/
│   ├── api.js          VectoriaClient class + session helpers (sessionStorage)
│   ├── nav.js          Shared nav bar, role routing, tab helper (initTabs)
│   ├── config.js       Set by deploy script: window.VECTORIA_DEFAULT_URL
│   └── upload.js       CSV/JSONL parser → batch indexing
├── css/
│   └── console.css     Design tokens, layout, component library
├── serve.sh            python3 -m http.server 8889 (dev)
└── NOTES.md            This file
```

---

## Security model

### API key is the only security boundary

Authentication is purely key-based. The tenant name in the URL is **not** a security boundary.

```
Tenant key vtk_<uuid>  →  Principal::Tenant("shoestore")
Admin key              →  Principal::Admin
```

### Tenant namespace isolation

Every index name in the URL is internally prefixed with the tenant name derived from the API key:

```
Tenant key for "shoestore" + POST /indexes/catalog/products
                           ↓
                  internal key: "shoestore/catalog"
```

A shoestore tenant hitting `/indexes/food/products` reaches `shoestore/food`, never `food/food`.
There is no URL path manipulation that can escape the tenant's namespace.

Admin key: accesses the bare index key directly (`catalog`, not `shoestore/catalog`).
Tenant indexes created via the admin panel live under `{tenant}/{index}` and are
listed separately via `GET /admin/tenants/{name}/indexes`.

### Error codes

Cross-namespace access returns **404 Not Found**, not 403 Forbidden.
This is intentional: a 403 would confirm the namespace exists; 404 reveals nothing.

### API key format

`vtk_<uuid-without-hyphens>` — 128-bit random, unguessable.
Tenant names are human-readable labels (routing labels, not secrets).
Knowing a tenant name without its API key grants no access.

---

## Session management

Session stored in `sessionStorage` (tab-scoped, cleared on tab close):

| key         | value                              |
|-------------|------------------------------------|
| `vt_url`    | server URL                         |
| `vt_key`    | API key                            |
| `vt_role`   | `"admin"` or `"tenant"`           |
| `vt_index`  | index name (tenant sessions only)  |

### Platform mode vs dev mode

`js/config.js` is written by the deploy script:
```js
window.VECTORIA_DEFAULT_URL = 'https://demo.vectoriasearch.com';
```

When `VECTORIA_DEFAULT_URL` is set (production platform), `index.html` uses it as the
server URL and does **not** restore any saved session. This prevents stale `localhost` URLs
from a prior dev session from silently blocking HTTPS platform users.

In dev (no config.js or empty file), full session restore applies — URL, key, and index are
all pre-filled from the previous session.

### Admin-to-tenant navigation

`tenant-detail.html` switches to the tenant key when "Search & Overrides" is clicked,
storing the admin key under `vt_prev_key` / `vt_prev_role` so `tenant.html` can restore
it on Back navigation.

---

## Content Security Policy

Platform nginx serves:
```
Content-Security-Policy:
  default-src 'self';
  script-src 'self' 'unsafe-inline';
  style-src 'self' 'unsafe-inline';
  connect-src *;
  img-src 'self' data:;
  font-src 'self';
  frame-ancestors 'none'
```

**Why `'unsafe-inline'` for scripts:** All page logic is in inline `<script type="module">` blocks.
Static files can't use CSP nonces (requires server-side rendering per request).
Hashes would need recomputing on every HTML edit.
Long-term fix: move inline scripts to external `.js` files; then `'self'` covers them and
`'unsafe-inline'` can be removed.

**Why `connect-src *`:** The console connects to whatever server URL the user configures.
Restricting to a specific origin would break self-hosted deployments.

**Why `'unsafe-inline'` for styles:** Inline `style=` attributes are used on several elements
for one-off layout tweaks.

`frame-ancestors 'none'` prevents clickjacking — most important protection in this CSP.

---

## Deployment

### Current (monorepo deploy script)

```bash
./deploy/deploy.sh --platform   # sync console + overwrite config.js + reload nginx
./deploy/deploy.sh --site-only  # sync everything + reload nginx (no Rust rebuild)
./deploy/deploy.sh              # full: sync + rebuild Docker image + restart
```

The deploy script:
1. Rsyncs `examples/saas-console/` to `/opt/apps/vectoria/platform/` on the server
2. Overwrites `js/config.js` with the production default URL (rsync would restore the dev default)
3. Diffs the nginx config and reloads if changed

nginx vhost `platform.vectoriasearch.com`:
- Root: `/opt/apps/vectoria/platform/`
- JS/CSS: `Cache-Control: no-cache, must-revalidate` (admin console must reflect deploys immediately)
- Images/fonts: `Cache-Control: public, immutable` (7-day cache, content-stable)

### Splitting this directory into its own repo

When splitting `examples/saas-console/` out:

**Move:**
- All HTML pages, `js/`, `css/`, `serve.sh`
- The nginx vhost block for `platform.vectoriasearch.com`

**Keep in vectoria:**
- `deploy/deploy.sh` (or duplicate the platform-sync portion)
- `examples/webstore/` — demo store
- `examples/admin-panel/` — training panel (separate codebase)
- `search-widget/` — embeddable JS widget
- `website/` — marketing site

**Replicate in the new repo's deploy:**
```bash
# After rsync, overwrite config.js with production URL
ssh user@host "echo \"window.VECTORIA_DEFAULT_URL = 'https://demo.vectoriasearch.com';\" \
  > /opt/apps/vectoria/platform/js/config.js"
```

**CSP `'unsafe-inline'` debt:** To remove it, move each page's inline `<script type="module">` block
to a corresponding external file (`js/login.js`, `js/admin.js`, etc.) and reference it as
`<script type="module" src="js/login.js">`. Five files to convert.

---

## Production hardening checklist

These are not implemented — they're the gap between this prototype and a real hosted product.

### Auth layer

The prototype stores the raw `vtk_` key in `sessionStorage`. Production should:
- Put an auth proxy (Next.js, Axum middleware, Cloudflare Worker) in front of Vectoria
- Proxy injects the per-tenant key from a secrets manager; browser never touches `vtk_` keys
- Replace `sessionStorage` with an HttpOnly cookie set by the proxy

### Rate limiting on auth failures

Currently the global `VECTORIA_RATE_LIMIT_PER_SECOND` throttles all requests.
No specific lockout on repeated 401s — an attacker can probe at 100 req/sec.
Add a per-IP 401 counter with exponential backoff in nginx or the auth proxy.

### Admin key rotation

Rotating the admin key requires a server restart with a new env var.
Wire `POST /admin/rotate-key` or allow hot-reload from a secrets manager.

### Per-tenant rate limits

`VECTORIA_RATE_LIMIT_PER_SECOND` is global. Per-tenant rate limiting requires
a middleware shim that checks the tenant name from the resolved `Principal`
and applies per-key counters (Redis or in-memory with periodic reset).

### Per-tenant quotas

Add a `quotas` table alongside `tenants.json`:

| quota             | suggested default |
|-------------------|-------------------|
| `max_products`    | 50 000            |
| `max_queries/day` | 100 000           |
| `max_indexes`     | 5                 |

Enforce in the auth proxy before forwarding to Vectoria.
Wire Stripe webhook `customer.subscription.updated` → update quota row.

### Bulk product loading

No bulk endpoint exists. `POST /indexes/{name}/products` accepts one product at a time.
For large catalogs (100k+ products):
- Generate a presigned S3/R2 URL server-side
- Trigger an import worker on `ObjectCreated`
- Stream progress via SSE or polling

### Observability

Ship structured logs (JSON) from Vectoria → Loki / OpenSearch / Datadog.
Add `X-Tenant-Id` in the auth proxy so every log line is attributable.

Track per tenant: query latency, zero-result rate, upload errors, override hit rate.

### Deployment topology

```
Browser ──► Auth proxy (Node/Axum, ~50 MB)
                │  vtk_ key from Secrets Manager
            Vectoria (single binary, ~120 MB)
                │  EdgeStore HNSW
            /data/vectoria/indexes/{tenant}/{index}/
```

Mount `/data` on EFS (AWS) or a Fly volume for persistence across restarts.
ECS Fargate or Fly.io for zero-downtime deploys.

### Multi-region

Vectoria is single-node. For multi-region:
- One Vectoria per region; fan-out writes at the proxy
- Route reads to nearest region (Cloudflare Workers, Fly regions)
- Override state (pins, suppressions) in a single Postgres with read replicas
