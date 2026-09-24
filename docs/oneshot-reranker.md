# Zero-Shot Reranker Experiment

Evaluation of four zero-shot post-rankers on top of Vectoria's hybrid search, using ESCI ground-truth labels. None were fine-tuned on Vectoria's data.

**Rerankers compared:**
- **Jev** (`jev-latest`) — commercial system-one API (TypeSafe), multilingual LLM base
- **ms-marco-MiniLM-L-6-v2** — local cross-encoder, English MS MARCO, 22M params
- **mmarco-mMiniLMv2-L12-H384-v1** — local cross-encoder, multilingual mMARCO (13 languages incl. pt-BR), 117M params
- **bge-reranker-v2-m3** — local cross-encoder, multilingual BGE-M3 base, ~568M params

---

## Setup

**Index**: 2,000 ESCI US-locale products (Amazon ESCI dataset)  
**Ground truth**: ESCI judges file — E and S labels treated as relevant (binary NDCG)  
**Retrieval**: Vectoria hybrid search, top-30 candidates  
**Queries**: 23 evaluable (from 39 judged; others had 0 relevant products in top-30)  
**Evaluation**: NDCG@10 before and after reranking, same query set for both rerankers

Both rerankers receive the same top-30 Vectoria candidates. The before-NDCG is identical for both.

---

## Results

| Metric | Vectoria | ms-marco (EN) | mmarco (multilingual) | bge-v2-m3 | Jev |
|---|---|---|---|---|---|
| Mean NDCG@10 | 0.411 | 0.792 | 0.829 | 0.827 | **0.935** |
| Mean Δ | — | +0.381 | +0.419 | +0.416 | **+0.524** |
| Queries improved | — | 17/23 | 16/23 | 18/23 | **19/23** |
| Queries hurt | — | 3/23 | 3/23 | 3/23 | **1/23** |
| Avg latency (CPU) | — | ~127ms | ~167ms | ~2415ms | ~2–4s (API) |
| Multilingual | — | ✗ | ✓ | ✓ | ✓ |
| Cost | free | free | free | free | ~$0.15/300q |

**mmarco is the best local option**: beats English CE by +0.038 NDCG, handles multilingual queries, and is only 40ms slower than the English model. BGE-v2-m3 ties mmarco (+0.416 vs +0.419) but runs 14× slower on CPU — not worth it without a GPU.

### Where models diverge

| Query | Before | ms-marco | mmarco | bge-v2-m3 | Jev |
|---|---|---|---|---|---|
| `ジャスミンボディソープ` | 0.387 | −0.387 ✗ | **+0.613** ✓ | **+0.613** ✓ | **+0.613** ✓ |
| `(travel size) juillet perfume…` | 0.000 | 0.000 | +0.631 | +0.356 | **+1.000** |
| `$1 first addition charizard…` | 0.877 | +0.123 | 0.000 | **−0.395** ✗ | +0.123 |
| `'but not the hippopotamus book` | 1.000 | −0.067 ✗ | −0.099 ✗ | −0.087 ✗ | **0.000** |
| `.8 beading wire plastic…` | 0.000 | +0.644 | +0.889 | +0.637 | **+0.927** |

The hippopotamus book query is the interesting failure: all three local models hurt a perfect Vectoria result, while Jev leaves it unchanged.

---

## How it works

### Jev (system-one / Noul)

Jev receives the query and all 30 candidate documents in a single API call. For each document it answers a `Noul` (probability of truth) claim:

```
"Document documents.D00 is relevant to query: it contains information
that answers or directly addresses it."
```

One question per document, all sharing the same state. Sorting by P(true) descending produces the reranked list.

Key property: P(relevant) is an absolute confidence, not just an ordering. A `top_p` of 0.02 means "no good candidates were retrieved" — actionable signal for query expansion or fallback. Jev uses a multilingual LLM base, so non-English queries work.

### Cross-encoders

All three score (query, document) pairs jointly through a shared transformer — no separate embedding spaces, no approximation. Runs locally on CPU.

- **ms-marco-MiniLM-L-6-v2**: English-only MS MARCO, 22M params, ~127ms/query. Fails on non-English input.
- **mmarco-mMiniLMv2-L12-H384-v1**: mMARCO multilingual (13 languages including pt-BR), 117M params, ~167ms/query. Best local option: handles Portuguese and Japanese with minimal latency overhead.
- **bge-reranker-v2-m3**: BGE-M3 base, multilingual, ~568M params. Ties mmarco on quality but runs at ~2400ms/query on CPU — only competitive on GPU.

---

## Caveats

**Query set bias.** The 39 judged queries are the alphabetical head of the ESCI test split — mostly special characters, numbers, and punctuation. Results on a representative Portuguese or mixed-language distribution may differ significantly.

**Structural advantage.** Both rerankers benefit structurally: if Vectoria retrieves a relevant product at rank-15, any competent reranker can push it to rank-1 for an automatic NDCG@10 gain. Improving Vectoria's retrieval recall@30 compounds with reranking quality.

**Small index.** 2,000 products is narrow. A larger import (50k+) would produce more evaluable queries and more realistic retrieval recall.

---

## The recall problem

Reranking only works on what retrieval already found. Two separate problems hide under "bad results":

**1. Catalog coverage** — if the product doesn't exist in the index, no reranker helps. The pt-BR qualitative test showed this directly: Portuguese shoe queries against a US-locale ESCI catalog (clothing, books, electronics) return garbage, and all models correctly scored everything near zero. Fix: import the right products.

**2. Language mismatch in BM25** — BM25 is token-matching. "tênis" does not match "sneaker". Phonetics normalization fixes diacritics (`tênis` ↔ `tenis`) but not cross-language synonymy. Semantic search (multilingual-e5-small) bridges this at the vector leg, but BM25 recall stays low for Portuguese queries against English titles.

**Fix for language mismatch**: translate the query to English before the BM25 leg (keep the original for semantic search which is already multilingual). A lightweight local translation model (`Helsinki-NLP/opus-mt-pt-en`, ~300MB) or the LLM rewriter already wired in Vectoria covers this. This is a retrieval improvement independent of reranking.

---

## Integration design

### Sidecar architecture (recommended for Vectoria)

Vectoria is a single Rust binary. Embedding ML inference in Rust (ONNX, Candle) is feasible but a significant engineering investment. The right approach for an optional feature is a **Python sidecar**:

```
[vectoria.toml]
[reranker]
url = "http://localhost:8001/rerank"   # absent = reranking disabled
```

Vectoria POSTs candidates to the sidecar, gets back sorted IDs:

```json
POST /rerank
{"query": "tênis corrida", "hits": [{"id": "B001", "title": "...", "description": "..."}, ...]}

→ {"ranked_ids": ["B003", "B001", "B007", ...]}
```

If `[reranker]` is absent from config, the step is skipped silently — zero impact on existing deployments. The sidecar can be `experiments/rerank_sidecar.py` (a dozen lines of FastAPI) running as a separate process or container.

### Capability detection

The `/health` endpoint (or a `/capabilities` endpoint) reports what's loaded:

```json
{
  "status": "ok",
  "version": "0.1.24",
  "capabilities": {
    "reranker": "mmarco-mMiniLMv2-L12-H384-v1",
    "reranker_status": "ready"
  }
}
```

When no sidecar is configured: `"reranker": null`. Clients can inspect this before sending `rerank: true`.

### Request-level opt-in

`rerank: true` in `SearchRequest` triggers the reranking step only if a sidecar is configured. If not configured, the field is silently ignored — no error, no behaviour change.

```rust
pub rerank: bool,   // default false; ignored if no reranker configured
```

### VPS feasibility

| Config | RAM needed | Latency/query | Notes |
|---|---|---|---|
| Vectoria alone | ~300MB | baseline | current state |
| + mmarco-mMiniLMv2 sidecar | ~800MB total | +500–800ms (2-vCPU VPS) | 2GB VPS minimum |
| + mmarco-mMiniLMv2 sidecar | ~800MB total | +167ms (MacBook M-series) | comfortable |
| + bge-reranker-v2-m3 sidecar | ~1.3GB total | +2400ms CPU / ~200ms GPU | needs 4GB VPS or GPU |

On a $6/mo 2GB VPS: mmarco adds ~600ms at low traffic, acceptable if `rerank` is opt-in per request. At >20 req/sec with reranking enabled, the sidecar becomes the bottleneck — scale by adding a second sidecar instance or disabling reranking under load.

**The sidecar loads once at startup** (~20–30s on a slow VPS). After that it stays resident and serves inference with no cold-start penalty.

### Query-level cache for paid rerankers (Jev)

Jev is deterministic given (query, candidate set). Cache on:

```
key = sha256(query + sorted(candidate_ids))
```

Eliminates cost for repeated head queries. Tail queries — which benefit most from reranking — rarely repeat, so they always pay. TTL should match catalog update frequency.

---

## Cost estimate (Jev)

| Daily queries | Monthly cost (est.) |
|---|---|
| 1,000 | ~$13–19 |
| 10,000 | ~$130–190 |
| 100,000 | ~$1,300–1,900 |

With query-level caching and typical head-heavy distributions, effective cost is 30–50% lower.

---

## Recommended next steps

1. **Fix BM25 recall for pt-BR first.** Add query translation (`Helsinki-NLP/opus-mt-pt-en`) to the BM25 leg. Reranking is only as good as what retrieval finds — this is higher leverage than a better reranker.

2. **Implement the sidecar.** A minimal FastAPI sidecar (`experiments/rerank_sidecar.py`) with `mmarco-mMiniLMv2` is ~50 lines. Wire the `[reranker] url` config into Vectoria as a no-op when absent.

3. **Repeat with a larger, representative query set.** Import 50k ESCI products, use the full test split. The current 23-query evaluation is too narrow to be conclusive — the alphabetical test split overrepresents special-character queries.

4. **pt-BR ground-truth evaluation.** Needs a domain-specific judge set: run the Portuguese shoe queries against a real shoestore catalog, have humans label E/S/C/I, then measure NDCG. The ESCI US-locale index is the wrong catalog for this.

5. **Jev for quality-sensitive use cases.** If Δ+0.1 NDCG vs mmarco justifies the API cost for a given tenant, Jev slots in behind the same sidecar interface. The query+candidates cache keeps costs bounded.

---

## Experiment code

- [`experiments/jev_rerank_eval.py`](../experiments/jev_rerank_eval.py) — Jev evaluation (requires `TYPESAFE_API_KEY`)
- [`experiments/cross_encoder_rerank_eval.py`](../experiments/cross_encoder_rerank_eval.py) — Cross-encoder evaluation (swap model via `CE_MODEL=`)
- [`experiments/ptbr_qualitative_eval.py`](../experiments/ptbr_qualitative_eval.py) — qualitative pt-BR comparison across multiple models

All scripts share the same env vars: `VECTORIA_URL`, `VECTORIA_API_KEY`, `VECTORIA_INDEX`, `JUDGES_FILE`.
