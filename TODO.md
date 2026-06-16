# Vectoria — Open Items

## Done

- [x] Benchmark (ESCI, 5000 products, E/E+S/E+S+C label sets) — see README for numbers
  - All modes 100% coverage across all label sets (spell corrector zero-result fallback)
  - E+S: BM25 MRR=0.6595, Hybrid=0.6576, Semantic=0.5690
  - Reproduce: `make esci-import && make esci-judges && make bench`

## Done

- [x] Demo webstore (`examples/webstore/index.html`) — hybrid/semantic/BM25 toggle
- [x] ESCI loader (`examples/esci_import`) + Makefile targets (`esci-import`, `esci-judges`)
- [x] ESCI setup script (`examples/webstore/setup.sh`) — downloads data, imports catalog
- [x] Signal accumulation: `recompute_product_signals` on `StorageEngine` trait; aggregation loop now recomputes from raw events each cycle
- [x] SQLite storage backend (`SqliteStorage`) + integration tests
- [x] `cosine_similarity` deduplicated into `vector/mod.rs`
- [x] Query cache key fixed to include filters, aggregate, rerank fields
- [x] Binary rename: server is `vectoria-server`, CLI is `vectoria`
- [x] CORS enabled (`CorsLayer::permissive()`) for webstore cross-origin requests
- [x] EdgeStore HNSW wired: `flush()` calls `build_vector_index`; `reindex_all()` calls `flush()` — HNSW activates after `POST /admin/reindex`
- [x] TurboVec deleted (brute-force JSON index, identical algo to EdgeStore flat scan with no advantage)
- [x] Algolia-compat multi-index layer deleted (`IndexRegistry` + 5 routes, ~320 lines) — untested, no callers
- [x] edgestore upgraded to 1.0.2 — fixes HNSW staleness check (stamp file + Merkle root)
- [x] Spell corrector (`SpellCorrector` in `search/spell.rs`) — catalog-seeded SymSpell with zero-result fallback; corrects typos and compound splits ("whiteshoes" → "white shoes") without hurting well-formed queries
