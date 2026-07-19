/**
 * VectoriaClient — thin wrapper over the Vectoria HTTP API.
 *
 * Override methods accept an optional `indexName`. When null/undefined they
 * target the default index via /admin/*. When set they target the named index
 * via /indexes/{name}/admin/*, which is also accessible with tenant API keys.
 */
export class VectoriaClient {
  constructor(baseUrl, apiKey) {
    this.baseUrl = baseUrl.replace(/\/$/, '');
    this.apiKey  = apiKey;
  }

  _headers() {
    return { 'Content-Type': 'application/json', 'Authorization': `Bearer ${this.apiKey}` };
  }

  async _req(method, path, body) {
    const opts = { method, headers: this._headers() };
    if (body !== undefined) opts.body = JSON.stringify(body);
    const r = await fetch(this.baseUrl + path, opts);
    const text = await r.text();
    const json = text ? JSON.parse(text) : {};
    if (!r.ok) throw Object.assign(new Error(json.error || `HTTP ${r.status}`), { status: r.status, body: json });
    return json;
  }

  // ── Identity ────────────────────────────────────────────────────────────────

  async health()      { return this._req('GET', '/health'); }
  async stats()       { return this._req('GET', '/stats'); }

  /**
   * Detect caller role. Returns { role: 'admin' | 'tenant', index: string | null }.
   * Admin if GET /admin/tenants returns 200. Tenant otherwise.
   * Tenant index name must be supplied externally (the API does not expose it on auth).
   */
  async detectRole(knownIndex = null) {
    try {
      await this._req('GET', '/admin/tenants');
      return { role: 'admin', index: null };
    } catch (e) {
      if (e.status === 403) return { role: 'tenant', index: knownIndex };
      throw e;
    }
  }

  // ── Tenants (admin only) ────────────────────────────────────────────────────

  async listTenants()              { return this._req('GET',    '/admin/tenants'); }
  async createTenant(name)         { return this._req('POST',   '/admin/tenants', { name }); }
  async deleteTenant(name)         { return this._req('DELETE', `/admin/tenants/${encodeURIComponent(name)}`); }
  async listTenantIndexes(name)    { return this._req('GET',    `/admin/tenants/${encodeURIComponent(name)}/indexes`); }
  async rotateKey(name)            { return this._req('POST',   `/admin/tenants/${encodeURIComponent(name)}/rotate-key`); }

  // ── Indexes ─────────────────────────────────────────────────────────────────

  async listIndexes()         { return this._req('GET',    '/indexes'); }
  async createIndex(name)     { return this._req('POST',   '/indexes', { name }); }
  async deleteIndex(name)     { return this._req('DELETE', `/indexes/${encodeURIComponent(name)}`); }

  // ── Products ─────────────────────────────────────────────────────────────────

  /** Index a single product. indexName = null → default index. */
  async indexProduct(product, indexName = null) {
    const path = indexName ? `/indexes/${encodeURIComponent(indexName)}/products` : '/products';
    return this._req('POST', path, product);
  }

  /**
   * Batch index products. Sends up to `concurrency` requests in parallel.
   * onProgress(done, total) called after each product.
   */
  async indexProductBatch(products, indexName = null, { concurrency = 5, onProgress } = {}) {
    let done = 0; const total = products.length; const errors = [];
    const queue = [...products];
    const worker = async () => {
      while (queue.length) {
        const p = queue.shift();
        try { await this.indexProduct(p, indexName); }
        catch (e) { errors.push({ product: p, error: e.message }); }
        onProgress?.(++done, total);
      }
    };
    await Promise.all(Array.from({ length: Math.min(concurrency, products.length) }, worker));
    return { indexed: done - errors.length, errors };
  }

  // ── Search ──────────────────────────────────────────────────────────────────

  async search(q, { limit = 15, mode = 'hybrid' } = {}, indexName = null) {
    const path = indexName ? `/indexes/${encodeURIComponent(indexName)}/search` : '/search';
    return this._req('POST', path, { q, limit, mode });
  }

  // ── Overrides ────────────────────────────────────────────────────────────────

  _adminBase(indexName) {
    return indexName ? `/indexes/${encodeURIComponent(indexName)}/admin` : '/admin';
  }

  async indexStats(indexName)  { return this._req('GET', `${this._adminBase(indexName)}/stats`); }

  async getOverrides(indexName = null, q = null) {
    const qs = q ? `?q=${encodeURIComponent(q)}` : '';
    return this._req('GET', `${this._adminBase(indexName)}/overrides${qs}`);
  }

  async listPins(indexName = null)                       { return this._req('GET',    `${this._adminBase(indexName)}/pins`); }
  async createPin(query, productId, position, indexName) { return this._req('POST',   `${this._adminBase(indexName)}/pins`, { query, product_id: productId, position }); }
  async deletePin(id, indexName = null)                  { return this._req('DELETE', `${this._adminBase(indexName)}/pins/${id}`); }

  async listSponsored(indexName = null)                               { return this._req('GET',    `${this._adminBase(indexName)}/sponsored`); }
  async createSponsored(data, indexName = null)                       { return this._req('POST',   `${this._adminBase(indexName)}/sponsored`, data); }
  async deleteSponsored(id, indexName = null)                         { return this._req('DELETE', `${this._adminBase(indexName)}/sponsored/${id}`); }

  async listSuppressions(indexName = null)                            { return this._req('GET',    `${this._adminBase(indexName)}/suppressions`); }
  async createSuppression(query, productId, indexName = null)         { return this._req('POST',   `${this._adminBase(indexName)}/suppressions`, { query, product_id: productId }); }
  async deleteSuppression(id, indexName = null)                       { return this._req('DELETE', `${this._adminBase(indexName)}/suppressions/${id}`); }

  async aggregate(indexName = null)                                   { return this._req('POST',   `${this._adminBase(indexName)}/aggregate`); }
  async exportOverrides(indexName = null)                             { return this._req('GET',    `${this._adminBase(indexName)}/training-export`); }
  async importOverrides(data, indexName = null)                       { return this._req('POST',   `${this._adminBase(indexName)}/training-import`, data); }
}

// ── Session storage helpers ──────────────────────────────────────────────────

export const session = {
  save(url, key, role, index) {
    sessionStorage.setItem('vt_url',   url);
    sessionStorage.setItem('vt_key',   key);
    sessionStorage.setItem('vt_role',  role);
    sessionStorage.setItem('vt_index', index || '');
  },
  load() {
    return {
      url:   sessionStorage.getItem('vt_url')   || '',
      key:   sessionStorage.getItem('vt_key')   || '',
      role:  sessionStorage.getItem('vt_role')  || '',
      index: sessionStorage.getItem('vt_index') || '',
    };
  },
  clear() { ['vt_url','vt_key','vt_role','vt_index'].forEach(k => sessionStorage.removeItem(k)); },
  client() {
    const s = session.load();
    return s.url && s.key ? new VectoriaClient(s.url, s.key) : null;
  },
};
