
## Multi-tenant SaaS schema

**Q: 1:1 or 1:N tenant:index?**
Decision: 1:N — one tenant can own multiple indexes.
Rationale: tenants need separate indexes for catalog, returns, recommendations etc. without namespace collision.

**Q: How is the index namespace exposed in the API?**
Decision: Implicit — tenant key carries the namespace; API surface stays flat (`/indexes/{index-name}`). Server prepends `{tenant}/` internally to form the registry key.
Rationale: tenant keys are embedded in browser-side ecommerce JS. The URL must not expose multi-tenant structure to end users or leak tenant names across tenants.

**Q: Where do admin-created named indexes live vs tenant indexes?**
Decision: Two namespaces. Any registry key without `/` is a system index (admin-only, created via `POST /indexes`). Any key with `/` is `{tenant}/{index-name}` (tenant-owned). `POST /indexes` with a tenant key creates `{tenant}/{given-name}` implicitly. Admin UI shows System Indexes and a Tenant→Indexes tree separately — the current flat "ALL NAMED INDEXES" table is removed.
Rationale: prevents accidental cross-namespace access and makes the admin view match the actual ownership model.

**Q: Does TenantStore track which indexes a tenant owns?**
Decision: No — derived from IndexRegistry by prefix scan. TenantStore is auth-only (name → vtk_key). IndexRegistry is the single source of truth. `delete_tenant` cascades via `registry.delete_all_for_tenant(name)` (prefix scan). No sync bugs possible.
Rationale: two stores tracking the same membership list will diverge; derive from the authoritative source instead.

**Q: Where does the {tenant}/ naming convention live in code?**
Decision: Encapsulated in IndexRegistry via explicit methods: `list_for_prefix(tenant)`, `delete_by_prefix(tenant)`, and `get_for_tenant(tenant, index_name)`. Routes never manipulate the key format directly. vectoria-core has zero tenant awareness — IndexRegistry is in vectoria-server only.
Rationale: convention change (e.g. separator character) stays in one file; callers are decoupled from the naming scheme.

**Q: Can tenants create their own indexes (self-service)?**
Decision: Yes — `POST /indexes/{name}` with a tenant key creates `{tenant}/{name}`. Admin controls the account (create/delete tenant); tenant controls their own indexes. Admin provisioning the first index at tenant-creation time becomes optional.
Rationale: self-service from day one keeps the API stable when moving from manual to automated onboarding; no breaking change needed later.

**Q: Navigation model for tenant login and admin drill-down?**
Decision: Tenant login — skip to search/overrides if exactly one index exists; show index picker if multiple. Admin drill-down — `tenant-detail.html` page (separate from admin.html) shows tenant's index list; clicking an index enters `tenant.html` for search/overrides. The flat "ALL NAMED INDEXES" table on admin.html is replaced by the tenant-detail page.
Rationale: most tenants at launch have one index (zero friction); the picker is ready for multi-index without a new page; admin gets a clean per-tenant view.

**Q: Auto-create first index at tenant creation?**
Decision: No — `POST /admin/tenants` creates auth entry only (name + vtk_key). Zero indexes provisioned. Tenant creates their first index via self-service (`POST /indexes/{name}` with their vtk_ key). UI and inline help text guides them through this bootstrap step.
Rationale: avoids auto-naming confusion (`acme/acme`); keeps the provisioning payload minimal; self-service indexes already decided.

**Cross-cutting: inline guidance**
Decision: add terse one-liner help text throughout the console (empty states, zero-index tenant rows, create-index forms). No external docs needed for the happy path.
Rationale: sparse external documentation; in-app text is the only guide most tenants will see.
