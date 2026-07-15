/*!
 * vectoria-search v0.1.0
 * Smart search widget for Vectoria — client-side query enhancement, zero extra roundtrips.
 * Supports en-US and pt-BR with built-in synonym dictionaries.
 * https://vectoriasearch.com
 * MIT License
 */
(function (global, factory) {
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = factory();
  } else if (typeof define === 'function' && define.amd) {
    define(factory);
  } else {
    global.VectoriaSearch = factory();
  }
}(typeof globalThis !== 'undefined' ? globalThis : typeof window !== 'undefined' ? window : this, function () {
  'use strict';

  // ── Built-in UI strings ──────────────────────────────────────────────────────
  const STRINGS = {
    'en-US': {
      placeholder: 'Search products…',
      noResults: 'No results found.',
      error: 'Search is unavailable right now.',
      enhanced: 'Query enhanced:',
      searching: 'Searching…',
    },
    'pt-BR': {
      placeholder: 'Buscar produtos…',
      noResults: 'Nenhum resultado encontrado.',
      error: 'A busca está indisponível agora.',
      enhanced: 'Consulta expandida:',
      searching: 'Buscando…',
    },
  };

  // ── Built-in synonym dictionaries ────────────────────────────────────────────
  // Each entry: term → additional terms to append to the query (space-joined).
  // These are retail-oriented: the most common cross-vocabulary mismatches.
  const SYNONYMS = {
    'en-US': {
      sneaker:    ['shoe', 'trainer', 'footwear', 'running shoe'],
      sneakers:   ['shoes', 'trainers', 'footwear'],
      shoe:       ['sneaker', 'trainer', 'footwear', 'boot'],
      shoes:      ['sneakers', 'trainers', 'footwear', 'boots'],
      trainer:    ['sneaker', 'shoe', 'running shoe'],
      boot:       ['shoe', 'footwear', 'leather boot'],
      boots:      ['shoes', 'footwear'],
      sandal:     ['shoe', 'footwear', 'slipper', 'slide'],
      sandals:    ['shoes', 'footwear'],
      headphone:  ['earphone', 'earbud', 'headset', 'audio', 'wireless'],
      headphones: ['earphones', 'earbuds', 'headset', 'audio'],
      earbud:     ['earphone', 'headphone', 'wireless', 'bluetooth'],
      earbuds:    ['earphones', 'headphones', 'wireless'],
      speaker:    ['audio', 'bluetooth speaker', 'sound system'],
      speakers:   ['audio', 'bluetooth'],
      laptop:     ['notebook', 'computer', 'pc', 'macbook'],
      notebook:   ['laptop', 'computer', 'pc'],
      computer:   ['laptop', 'notebook', 'pc'],
      phone:      ['smartphone', 'mobile', 'cell'],
      smartphone: ['phone', 'mobile', 'cell'],
      sofa:       ['couch', 'loveseat', 'sectional'],
      couch:      ['sofa', 'loveseat'],
      chair:      ['seat', 'seating', 'stool', 'office chair'],
      desk:       ['table', 'workstation', 'standing desk'],
      mattress:   ['bed', 'sleep', 'foam', 'spring'],
      jacket:     ['coat', 'parka', 'windbreaker', 'outerwear'],
      coat:       ['jacket', 'parka', 'outerwear'],
      hoodie:     ['sweatshirt', 'fleece', 'pullover'],
      jeans:      ['pants', 'denim', 'trousers'],
      pants:      ['jeans', 'trousers', 'bottoms'],
      shirt:      ['top', 'tee', 'polo', 'blouse'],
      watch:      ['smartwatch', 'wristwatch', 'timepiece'],
      smartwatch: ['watch', 'fitness tracker', 'gps watch'],
      camera:     ['dslr', 'mirrorless', 'photography', 'lens'],
      tv:         ['television', 'screen', 'display', 'smart tv'],
      television: ['tv', 'screen', 'display'],
      gym:        ['fitness', 'workout', 'exercise', 'training'],
      fitness:    ['gym', 'workout', 'exercise', 'sport'],
      running:    ['jogging', 'sport', 'cardio', 'marathon'],
      yoga:       ['pilates', 'stretching', 'mindfulness', 'mat'],
      wireless:   ['bluetooth', 'cordless', 'wifi'],
      bluetooth:  ['wireless', 'cordless'],
      ergonomic:  ['comfortable', 'adjustable', 'lumbar', 'posture'],
      standing:   ['adjustable', 'ergonomic', 'height adjustable'],
      waterproof: ['water resistant', 'weatherproof', 'rain', 'outdoor'],
      outdoor:    ['hiking', 'trail', 'camping', 'adventure'],
      hiking:     ['outdoor', 'trail', 'trekking', 'backpacking'],
    },
    'pt-BR': {
      tênis:        ['sapato', 'calçado', 'sneaker', 'shoes', 'trainer'],
      sapato:       ['tênis', 'calçado', 'shoe', 'boot'],
      sapatos:      ['tênis', 'calçados', 'shoes', 'boots'],
      calçado:      ['tênis', 'sapato', 'footwear'],
      calçados:     ['tênis', 'sapatos', 'footwear'],
      sandália:     ['chinelo', 'sapato', 'sandal'],
      sandálias:    ['chinelos', 'calçados', 'sandals'],
      bota:         ['sapato', 'coturno', 'boot'],
      botas:        ['sapatos', 'calçados', 'boots'],
      fone:         ['headphone', 'auricular', 'fone de ouvido', 'earphone'],
      headphone:    ['fone', 'auricular', 'fone de ouvido', 'earphone'],
      'fone de ouvido': ['headphone', 'fone', 'auricular'],
      caixa:        ['caixa de som', 'speaker', 'bluetooth', 'caixa acústica'],
      'caixa de som': ['speaker', 'bluetooth', 'áudio'],
      notebook:     ['laptop', 'computador', 'pc'],
      laptop:       ['notebook', 'computador', 'pc'],
      computador:   ['notebook', 'laptop', 'pc', 'computer'],
      celular:      ['smartphone', 'telefone', 'mobile', 'phone'],
      smartphone:   ['celular', 'telefone', 'mobile'],
      sofá:         ['sofa', 'couch', 'divã'],
      poltrona:     ['cadeira', 'assento', 'chair'],
      cadeira:      ['poltrona', 'assento', 'chair', 'seat'],
      mesa:         ['escrivaninha', 'bancada', 'desk', 'workstation'],
      colchão:      ['cama', 'mattress', 'colchonete'],
      jaqueta:      ['casaco', 'agasalho', 'jacket', 'coat'],
      casaco:       ['jaqueta', 'agasalho', 'jacket'],
      moletom:      ['blusa', 'hoodie', 'sweatshirt', 'fleece'],
      calça:        ['jeans', 'bermuda', 'pants', 'trousers'],
      jeans:        ['calça', 'denim', 'pants'],
      camiseta:     ['camisa', 'blusa', 'shirt', 'top', 'tee'],
      relógio:      ['smartwatch', 'cronógrafo', 'watch'],
      smartwatch:   ['relógio', 'pulseira inteligente', 'watch', 'fitness tracker'],
      câmera:       ['câmara', 'fotografia', 'camera', 'dslr', 'mirrorless'],
      'tv':         ['televisão', 'televisor', 'television', 'tela'],
      televisão:    ['tv', 'televisor', 'screen'],
      academia:     ['ginástica', 'fitness', 'musculação', 'treino', 'gym'],
      treino:       ['academia', 'exercício', 'workout', 'fitness'],
      corrida:      ['running', 'jogging', 'esporte', 'maratona'],
      yoga:         ['pilates', 'alongamento', 'meditação'],
      sem:          ['wireless', 'bluetooth'],  // "sem fio" → wireless
      fio:          ['wireless', 'bluetooth', 'sem fio'],
      'sem fio':    ['wireless', 'bluetooth', 'cordless'],
      impermeável:  ['waterproof', 'water resistant', 'chuva'],
      externo:      ['outdoor', 'trilha', 'camping'],
      trilha:       ['outdoor', 'hiking', 'trekking', 'externo'],
      ergonômico:   ['ergonomic', 'confortável', 'ajustável', 'lumbar'],
      altura:       ['ajustável', 'standing', 'ergonômico'],
    },
  };

  // ── Utility: Unicode normalization (keeps accents, adds unaccented variant) ──
  function normalizeText(text) {
    return text.trim().replace(/\s+/g, ' ').toLowerCase();
  }

  function stripAccents(text) {
    return text.normalize('NFD').replace(/[̀-ͯ]/g, '');
  }

  // ── Query enhancer ───────────────────────────────────────────────────────────
  class QueryEnhancer {
    constructor(locale, customSynonyms) {
      this.locale   = SYNONYMS[locale] ? locale : 'en-US';
      this.dict     = Object.assign({}, SYNONYMS[this.locale], customSynonyms || {});
      this.strings  = STRINGS[this.locale] || STRINGS['en-US'];
    }

    enhance(rawQuery) {
      const normalized  = normalizeText(rawQuery);
      const tokens      = normalized.split(/\s+/).filter(Boolean);
      const expansions  = [];
      const extra       = new Set();

      for (const token of tokens) {
        const stripped = stripAccents(token);

        // Look up original and accent-stripped form.
        const syns = this.dict[token] || this.dict[stripped] || [];
        if (syns.length) {
          expansions.push({ term: token, added: syns });
          syns.forEach(s => s.split(/\s+/).forEach(w => extra.add(w)));
        }
      }

      // Also add the accent-stripped version of the original query as a fallback
      // so BM25 matches both "tenis" and "tênis".
      const stripped = stripAccents(normalized);
      if (stripped !== normalized) {
        extra.add(stripped);
      }

      const enhanced = extra.size
        ? normalized + ' ' + [...extra].join(' ')
        : normalized;

      return { original: normalized, enhanced, expansions };
    }
  }

  // ── Vectoria HTTP client ─────────────────────────────────────────────────────
  class VectoriaClient {
    constructor(url, apiKey) {
      this.url    = url.replace(/\/$/, '');
      this.apiKey = apiKey;
    }

    _headers() {
      return {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${this.apiKey}`,
      };
    }

    async search(query, options = {}) {
      const { indexName, mode, limit, filters, rankingWeights } = options;
      const endpoint = indexName
        ? `${this.url}/indexes/${encodeURIComponent(indexName)}/search`
        : `${this.url}/search`;

      const body = { q: query, limit: limit || 10, mode: mode || 'hybrid' };
      if (filters)         body.filters         = filters;
      if (rankingWeights)  body.ranking_weights  = rankingWeights;

      const resp = await fetch(endpoint, {
        method: 'POST',
        headers: this._headers(),
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        const text = await resp.text().catch(() => resp.statusText);
        throw new Error(`Vectoria ${resp.status}: ${text}`);
      }
      return resp.json();
    }

    async autocomplete(prefix, limit) {
      const url = `${this.url}/autocomplete?q=${encodeURIComponent(prefix)}&limit=${limit || 5}`;
      const resp = await fetch(url, { headers: this._headers() });
      if (!resp.ok) return [];
      return resp.json();
    }
  }

  // ── Default CSS (injected once into <head>) ──────────────────────────────────
  const DEFAULT_CSS = `
.vs-wrap { position: relative; font-family: var(--vs-font, -apple-system, 'Inter', system-ui, sans-serif); }
.vs-input-row { position: relative; }
.vs-input {
  width: 100%; padding: 11px 44px 11px 14px; font-size: 0.9375rem;
  border: 1.5px solid var(--vs-border, #e2e8f0); border-radius: var(--vs-radius, 8px);
  background: var(--vs-bg, #fff); color: var(--vs-text, #0f172a);
  outline: none; transition: border-color 0.15s, box-shadow 0.15s; box-sizing: border-box;
  font-family: inherit;
}
.vs-input:focus {
  border-color: var(--vs-primary, #2563eb);
  box-shadow: 0 0 0 3px rgba(37,99,235,0.12);
}
.vs-input::placeholder { color: var(--vs-muted, #94a3b8); }
.vs-icon {
  position: absolute; right: 13px; top: 50%; transform: translateY(-50%);
  color: var(--vs-muted, #94a3b8); pointer-events: none; line-height: 0;
}
.vs-autocomplete {
  position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 999;
  background: var(--vs-bg, #fff); border: 1px solid var(--vs-border, #e2e8f0);
  border-radius: var(--vs-radius, 8px); box-shadow: var(--vs-shadow, 0 4px 16px rgba(0,0,0,0.1));
  overflow: hidden; display: none;
}
.vs-autocomplete.vs-open { display: block; }
.vs-ac-item {
  padding: 9px 14px; font-size: 0.875rem; color: var(--vs-text, #0f172a);
  cursor: pointer; transition: background 0.1s;
}
.vs-ac-item:hover, .vs-ac-item.vs-selected { background: var(--vs-bg2, #f8fafc); }
.vs-enhancement {
  margin-top: 6px; font-size: 0.78rem; color: var(--vs-muted, #64748b);
  display: none; flex-wrap: wrap; align-items: center; gap: 6px;
}
.vs-enhancement.vs-visible { display: flex; }
.vs-enhancement-label { font-weight: 500; }
.vs-expansion-chip {
  background: rgba(37,99,235,0.08); color: var(--vs-primary, #2563eb);
  padding: 1px 7px; border-radius: 12px; font-size: 0.74rem; font-weight: 500;
}
.vs-results { margin-top: 12px; }
.vs-status { font-size: 0.85rem; color: var(--vs-muted, #64748b); padding: 8px 0; }
.vs-results-grid { display: grid; gap: 10px; }
.vs-hit {
  border: 1px solid var(--vs-border, #e2e8f0); border-radius: var(--vs-radius, 8px);
  padding: 14px; cursor: pointer; transition: box-shadow 0.15s, border-color 0.15s;
  background: var(--vs-bg, #fff);
}
.vs-hit:hover { box-shadow: 0 4px 12px rgba(0,0,0,0.08); border-color: var(--vs-border2, #cbd5e1); }
.vs-hit-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 4px; }
.vs-hit-name { font-size: 0.9375rem; font-weight: 600; color: var(--vs-text, #0f172a); flex: 1; }
.vs-hit-score {
  font-size: 0.72rem; font-weight: 600; color: var(--vs-primary, #2563eb);
  background: rgba(37,99,235,0.08); padding: 2px 7px; border-radius: 10px;
  margin-left: 10px; flex-shrink: 0; font-family: var(--vs-mono, monospace);
}
.vs-hit-meta { font-size: 0.8125rem; color: var(--vs-muted, #64748b); }
.vs-hit-desc { font-size: 0.8125rem; color: var(--vs-text2, #334155); margin-top: 6px; line-height: 1.5; }
.vs-no-results { text-align: center; padding: 32px 16px; color: var(--vs-muted, #64748b); font-size: 0.9rem; }
`;

  let _cssInjected = false;
  function injectStyles() {
    if (_cssInjected || typeof document === 'undefined') return;
    const style = document.createElement('style');
    style.textContent = DEFAULT_CSS;
    document.head.appendChild(style);
    _cssInjected = true;
  }

  // ── HTML escape (safety) ─────────────────────────────────────────────────────
  function esc(str) {
    return String(str == null ? '' : str)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#x27;');
  }

  // ── Default hit renderer ─────────────────────────────────────────────────────
  function defaultRenderHit(hit) {
    const m = hit.metadata || {};
    const name  = m.title || m.name || hit.id;
    const brand = m.brand ? `${esc(m.brand)} · ` : '';
    const cat   = m.category || m.type || '';
    const price = m.price != null ? ` · $${Number(m.price).toLocaleString()}` : '';
    const desc  = m.description ? `<div class="vs-hit-desc">${esc(String(m.description).slice(0, 120))}</div>` : '';
    const score = Math.round((hit.score || 0) * 100);

    return `
      <div class="vs-hit-header">
        <div class="vs-hit-name">${esc(name)}</div>
        <div class="vs-hit-score">${score}%</div>
      </div>
      <div class="vs-hit-meta">${brand}${esc(cat)}${price}</div>
      ${desc}`;
  }

  // ── Core widget class ────────────────────────────────────────────────────────
  /**
   * VectoriaSearchWidget — attach to any DOM element.
   *
   * @example
   * const w = new VectoriaSearchWidget({
   *   container: '#search',
   *   url: 'https://demo.vectoriasearch.com',
   *   apiKey: 'vectoria-esci-demo',
   *   locale: 'pt-BR',
   * });
   */
  class VectoriaSearchWidget {
    constructor(options = {}) {
      this._opts     = this._defaults(options);
      this._enhancer = new QueryEnhancer(this._opts.locale, this._opts.synonyms);
      this._client   = new VectoriaClient(this._opts.url, this._opts.apiKey);
      this._strings  = STRINGS[this._opts.locale] || STRINGS['en-US'];
      this._timer    = null;
      this._acIdx    = -1;
      this._lastQ    = '';
      this._acItems  = [];

      if (this._opts.injectStyles) injectStyles();

      const container = typeof this._opts.container === 'string'
        ? document.querySelector(this._opts.container)
        : this._opts.container;

      if (!container) throw new Error('[VectoriaSearch] container not found');
      this._mount(container);
    }

    _defaults(opts) {
      return {
        container:    opts.container    || '#vectoria-search',
        url:          opts.url          || 'https://demo.vectoriasearch.com',
        apiKey:       opts.apiKey       || 'vectoria-esci-demo',
        locale:       opts.locale       || 'en-US',
        synonyms:     opts.synonyms     || {},
        mode:         opts.mode         || 'hybrid',
        limit:        opts.limit        || 10,
        indexName:    opts.indexName    || null,
        autocomplete: opts.autocomplete !== false,
        enhance:      opts.enhance      !== false,
        injectStyles: opts.injectStyles !== false,
        debounce:     opts.debounce     != null ? opts.debounce : 250,
        placeholder:  opts.placeholder  || null,
        renderHit:    opts.renderHit    || null,
        onResults:    opts.onResults    || null,
        onSelect:     opts.onSelect     || null,
        onError:      opts.onError      || null,
      };
    }

    _mount(container) {
      container.classList.add('vs-wrap');
      // Static markup only — no dynamic content interpolated here.
      container.innerHTML = `
        <div class="vs-input-row">
          <input class="vs-input" type="text" autocomplete="off" spellcheck="false">
          <span class="vs-icon">
            <svg width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
            </svg>
          </span>
          <div class="vs-autocomplete" role="listbox"></div>
        </div>
        <div class="vs-enhancement" aria-live="polite"></div>
        <div class="vs-results" role="region" aria-label="Search results"></div>`;

      this._input = container.querySelector('.vs-input');
      // Set placeholder as a property — avoids innerHTML interpolation.
      this._input.placeholder = this._opts.placeholder || this._strings.placeholder;
      this._acBox = container.querySelector('.vs-autocomplete');
      this._enhancement = container.querySelector('.vs-enhancement');
      this._results = container.querySelector('.vs-results');

      this._input.addEventListener('input',   () => this._onInput());
      this._input.addEventListener('keydown', (e) => this._onKey(e));
      this._acBox.addEventListener('click',   (e) => this._onAcClick(e));

      document.addEventListener('click', (e) => {
        if (!container.contains(e.target)) this._closeAc();
      });
    }

    _onInput() {
      clearTimeout(this._timer);
      const q = this._input.value;
      if (!q.trim() || q.trim().length < 2) {
        this._results.replaceChildren();
        this._enhancement.classList.remove('vs-visible');
        this._closeAc();
        return;
      }
      this._timer = setTimeout(() => this._run(q), this._opts.debounce);
    }

    _onKey(e) {
      if (!this._acBox.classList.contains('vs-open')) return;
      if (e.key === 'ArrowDown') {
        e.preventDefault(); this._acMove(1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault(); this._acMove(-1);
      } else if (e.key === 'Enter' && this._acIdx >= 0) {
        e.preventDefault();
        const items = this._acBox.querySelectorAll('.vs-ac-item');
        if (items[this._acIdx]) {
          this._input.value = items[this._acIdx].textContent;
          this._closeAc();
          this._run(this._input.value);
        }
      } else if (e.key === 'Escape') {
        this._closeAc();
      }
    }

    _onAcClick(e) {
      const item = e.target.closest('.vs-ac-item');
      if (!item) return;
      this._input.value = item.textContent;
      this._closeAc();
      this._run(this._input.value);
    }

    _acMove(dir) {
      const items = this._acBox.querySelectorAll('.vs-ac-item');
      if (!items.length) return;
      this._acIdx = Math.max(0, Math.min(items.length - 1, this._acIdx + dir));
      items.forEach((el, i) => el.classList.toggle('vs-selected', i === this._acIdx));
    }

    _closeAc() {
      this._acBox.classList.remove('vs-open');
      this._acIdx = -1;
    }

    async _run(rawQuery) {
      this._lastQ = rawQuery;

      // 1. Enhance query locally (zero roundtrips)
      const { original, enhanced, expansions } = this._opts.enhance
        ? this._enhancer.enhance(rawQuery)
        : { original: rawQuery, enhanced: rawQuery, expansions: [] };

      // 2. Show enhancement hints
      this._renderEnhancement(expansions);

      // 3. Optional: fetch autocomplete suggestions in parallel (does not block search)
      if (this._opts.autocomplete) {
        this._client.autocomplete(original, 6).then(sug => {
          if (this._input.value.trim() === rawQuery.trim()) this._renderAc(sug);
        }).catch(() => {});
      }

      // 4. Fetch results from Vectoria
      this._setStatus(this._strings.searching);
      try {
        const data = await this._client.search(enhanced, {
          indexName:     this._opts.indexName,
          mode:          this._opts.mode,
          limit:         this._opts.limit,
        });

        // Guard: ignore stale responses if the query changed while waiting
        if (this._input.value.trim() !== rawQuery.trim()) return;

        if (this._opts.onResults) this._opts.onResults(data, { original, enhanced });
        this._renderResults(data.hits || []);
      } catch (err) {
        if (this._input.value.trim() !== rawQuery.trim()) return;
        const msg = this._opts.onError ? this._opts.onError(err) : null;
        this._setStatus(typeof msg === 'string' ? msg : this._strings.error);
      }
    }

    _setStatus(text) {
      const el = document.createElement('div');
      el.className = 'vs-status';
      el.textContent = text;
      this._results.replaceChildren(el);
    }

    _renderEnhancement(expansions) {
      const el = this._enhancement;
      el.replaceChildren();
      if (!this._opts.enhance || !expansions.length) {
        el.classList.remove('vs-visible');
        return;
      }
      const label = document.createElement('span');
      label.className = 'vs-enhancement-label';
      label.textContent = this._strings.enhanced;
      el.appendChild(label);

      expansions.flatMap(e => e.added.slice(0, 3)).slice(0, 6).forEach(t => {
        const chip = document.createElement('span');
        chip.className = 'vs-expansion-chip';
        chip.textContent = '+' + t;
        el.appendChild(chip);
      });
      el.classList.add('vs-visible');
    }

    _renderAc(suggestions) {
      const items = Array.isArray(suggestions)
        ? suggestions
        : (Array.isArray(suggestions.suggestions) ? suggestions.suggestions : []);

      if (!items.length) { this._closeAc(); return; }
      const frag = document.createDocumentFragment();
      items.forEach(s => {
        const el = document.createElement('div');
        el.className = 'vs-ac-item';
        el.setAttribute('role', 'option');
        el.textContent = String(s);
        frag.appendChild(el);
      });
      this._acBox.replaceChildren(frag);
      this._acBox.classList.add('vs-open');
      this._acIdx = -1;
    }

    _renderResults(hits) {
      if (!hits.length) {
        const noRes = document.createElement('div');
        noRes.className = 'vs-no-results';
        noRes.textContent = this._strings.noResults;
        this._results.replaceChildren(noRes);
        return;
      }
      // Build hit HTML. `defaultRenderHit` runs esc() on every server-supplied field.
      // Custom `renderHit` callbacks own their own escaping — document this boundary.
      const renderer = this._opts.renderHit || defaultRenderHit;
      const html = hits.map(hit => {
        const inner = renderer(hit);
        return typeof inner === 'string'
          ? `<div class="vs-hit" data-id="${esc(hit.id)}" tabindex="0" role="button">${inner}</div>`
          : null;
      }).filter(Boolean).join('');

      const grid = document.createElement('div');
      grid.className = 'vs-results-grid';
      grid.innerHTML = html;
      this._results.replaceChildren(grid);

      this._results.querySelectorAll('.vs-hit').forEach(el => {
        const hitId = el.dataset.id;
        const hit   = hits.find(h => h.id === hitId);
        if (!hit) return;
        el.addEventListener('click', () => {
          if (this._opts.onSelect) this._opts.onSelect(hit);
          el.dispatchEvent(new CustomEvent('vs-select', { bubbles: true, detail: hit }));
        });
        el.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' || e.key === ' ') el.click();
        });
      });
    }

    /** Update configuration at runtime (e.g. change locale or mode). */
    setOption(key, value) {
      this._opts[key] = value;
      if (key === 'locale' || key === 'synonyms') {
        this._enhancer = new QueryEnhancer(this._opts.locale, this._opts.synonyms);
        this._strings  = STRINGS[this._opts.locale] || STRINGS['en-US'];
        const ph = this._opts.placeholder || this._strings.placeholder;
        if (this._input) this._input.placeholder = ph;
      }
    }

    /** Programmatically run a search. */
    search(query) {
      this._input.value = query;
      return this._run(query);
    }

    /** Clear input and results. */
    clear() {
      this._input.value = '';
      this._results.replaceChildren();
      this._enhancement.classList.remove('vs-visible');
      this._closeAc();
    }

    /** Remove the widget and its event listeners. */
    destroy() {
      clearTimeout(this._timer);
      const wrap = this._input && this._input.closest('.vs-wrap');
      if (wrap) wrap.replaceChildren();
    }
  }

  // ── Web Component: <vectoria-search> ────────────────────────────────────────
  /**
   * Custom element. Works in React 19+, Vue, Svelte, Angular, and plain HTML.
   *
   * Attributes mirror the widget options:
   *   url, api-key, locale, mode, limit, index-name, autocomplete, debounce
   *
   * Events dispatched on the element:
   *   vs-results — { detail: { hits, original, enhanced } }
   *   vs-select  — { detail: hit }
   *   vs-error   — { detail: Error }
   *
   * @example
   * <vectoria-search
   *   url="https://demo.vectoriasearch.com"
   *   api-key="vectoria-esci-demo"
   *   locale="pt-BR"
   *   mode="hybrid"
   *   limit="8">
   * </vectoria-search>
   */
  if (typeof customElements !== 'undefined') {
    class VectoriaSearchElement extends HTMLElement {
      static get observedAttributes() {
        return ['url', 'api-key', 'locale', 'mode', 'limit', 'index-name', 'autocomplete', 'debounce', 'placeholder'];
      }

      connectedCallback() {
        const self = this;
        const bool = (attr, def) => {
          const v = self.getAttribute(attr);
          return v === null ? def : v !== 'false' && v !== '0';
        };

        this._widget = new VectoriaSearchWidget({
          container:    this,
          url:          this.getAttribute('url')          || undefined,
          apiKey:       this.getAttribute('api-key')      || undefined,
          locale:       this.getAttribute('locale')       || 'en-US',
          mode:         this.getAttribute('mode')         || 'hybrid',
          limit:        parseInt(this.getAttribute('limit'), 10) || 10,
          indexName:    this.getAttribute('index-name')   || null,
          autocomplete: bool('autocomplete', true),
          debounce:     parseInt(this.getAttribute('debounce'), 10) || 250,
          placeholder:  this.getAttribute('placeholder')  || null,
          injectStyles: true,
          onResults: (data, meta) => {
            self.dispatchEvent(new CustomEvent('vs-results', {
              bubbles: true, detail: { hits: data.hits, ...meta }
            }));
          },
          onSelect: (hit) => {
            self.dispatchEvent(new CustomEvent('vs-select', { bubbles: true, detail: hit }));
          },
          onError: (err) => {
            self.dispatchEvent(new CustomEvent('vs-error', { bubbles: true, detail: err }));
          },
        });
      }

      disconnectedCallback() {
        if (this._widget) this._widget.destroy();
      }

      attributeChangedCallback(name, _old, val) {
        if (!this._widget) return;
        const map = { 'api-key': 'apiKey', 'index-name': 'indexName' };
        const key = map[name] || name;
        this._widget.setOption(key, val);
      }

      /** Expose programmatic search to external callers. */
      search(q)  { return this._widget && this._widget.search(q); }
      clear()    { return this._widget && this._widget.clear(); }
    }

    if (!customElements.get('vectoria-search')) {
      customElements.define('vectoria-search', VectoriaSearchElement);
    }
  }

  // ── Public API ───────────────────────────────────────────────────────────────
  return {
    /**
     * Programmatic entry point. Returns a VectoriaSearchWidget instance.
     * @param {object} options
     */
    init: function (options) {
      return new VectoriaSearchWidget(options);
    },

    /**
     * Low-level access to the query enhancer. Useful for server-side rendering
     * or custom integrations where you want the enhancement without the UI.
     *
     * @example
     * const enh = VectoriaSearch.enhancer('pt-BR');
     * const { enhanced, expansions } = enh.enhance('tênis corrida');
     */
    enhancer: function (locale, customSynonyms) {
      return new QueryEnhancer(locale, customSynonyms);
    },

    /** Low-level access to the Vectoria HTTP client. */
    client: function (url, apiKey) {
      return new VectoriaClient(url, apiKey);
    },

    VectoriaSearchWidget,
    QueryEnhancer,
    VectoriaClient,
  };
}));
