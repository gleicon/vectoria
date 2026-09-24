#!/usr/bin/env python3
"""
Cross-encoder reranking experiment for Vectoria — ESCI ground-truth evaluation.

Mirrors jev_rerank_eval.py but uses a local HuggingFace cross-encoder instead of Jev.
Fetches top-30 candidates from Vectoria hybrid search, reranks with the cross-encoder,
and reports NDCG@10 before/after using ESCI judges as ground truth.

Usage:
    pip install sentence-transformers requests
    python experiments/cross_encoder_rerank_eval.py

    VECTORIA_URL=http://localhost:7700 \
    VECTORIA_API_KEY=test-key \
    VECTORIA_INDEX=esci-eval \
    JUDGES_FILE=data/esci/judges.ndjson \
    python experiments/cross_encoder_rerank_eval.py
"""

import json
import math
import os
import sys
import time

import requests
from sentence_transformers import CrossEncoder

# ── Config ───────────────────────────────────────────────────────────────────

VECTORIA_URL   = os.getenv("VECTORIA_URL",    "http://localhost:7700")
VECTORIA_KEY   = os.getenv("VECTORIA_API_KEY", "test-key")
VECTORIA_INDEX = os.getenv("VECTORIA_INDEX",   "esci-eval")
JUDGES_FILE    = os.getenv("JUDGES_FILE",      "data/esci/judges.ndjson")
CE_MODEL       = os.getenv("CE_MODEL", "cross-encoder/ms-marco-MiniLM-L-6-v2")
TOP_K          = 30
TOP_N          = 10
MAX_QUERIES    = int(os.getenv("MAX_QUERIES", "39"))

# ── Judges ────────────────────────────────────────────────────────────────────

def load_judges(path: str) -> list[dict]:
    with open(path) as f:
        return [json.loads(l) for l in f if l.strip()]

def pick_queries(judges: list[dict], max_q: int) -> list[dict]:
    return sorted(judges, key=lambda j: len(j["relevant_ids"]), reverse=True)[:max_q]

# ── Vectoria search ───────────────────────────────────────────────────────────

def vectoria_search(query: str, index: str, limit: int = TOP_K) -> list[dict]:
    if index and index != "default":
        url  = f"{VECTORIA_URL}/indexes/{index}/search"
        body = {"q": query, "limit": limit}
    else:
        url  = f"{VECTORIA_URL}/search"
        body = {"q": query, "index": index, "limit": limit}
    resp = requests.post(url, json=body,
                         headers={"Authorization": f"Bearer {VECTORIA_KEY}"},
                         timeout=15)
    resp.raise_for_status()
    return resp.json().get("hits", [])

# ── Cross-encoder reranking ───────────────────────────────────────────────────

def ce_rerank(query: str, hits: list[dict], model: CrossEncoder) -> list[dict]:
    if not hits:
        return []
    pairs = []
    for h in hits:
        meta  = h.get("metadata", {})
        title = meta.get("title", "")
        desc  = meta.get("description", "") or meta.get("text", "")
        doc   = f"{title}. {desc}".strip(". ")
        pairs.append([query, doc])

    scores = model.predict(pairs, show_progress_bar=False)
    scored = [{**h, "_ce_score": float(s)} for h, s in zip(hits, scores)]
    scored.sort(key=lambda h: -h["_ce_score"])
    return scored

# ── Metrics ───────────────────────────────────────────────────────────────────

def _dcg(relevances: list[float]) -> float:
    return sum(r / math.log2(i + 2) for i, r in enumerate(relevances))

def ndcg_at_k(ranked: list[dict], relevant_ids: set[str], k: int = TOP_N) -> float:
    gains = [1.0 if h["id"] in relevant_ids else 0.0 for h in ranked[:k]]
    n_rel = min(sum(gains), k)
    ideal = [1.0] * int(n_rel) + [0.0] * (k - int(n_rel))
    dcg   = _dcg(gains)
    idcg  = _dcg(ideal)
    return dcg / idcg if idcg > 0 else 0.0

def hits_in_top_k(ranked: list[dict], relevant_ids: set[str], k: int) -> int:
    return sum(1 for h in ranked[:k] if h["id"] in relevant_ids)

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    if not os.path.exists(JUDGES_FILE):
        sys.exit(f"Judges file not found: {JUDGES_FILE}")

    print(f"Loading cross-encoder: {CE_MODEL}")
    t0 = time.time()
    model = CrossEncoder(CE_MODEL, max_length=512)
    print(f"  loaded in {time.time()-t0:.1f}s")
    print()

    judges  = load_judges(JUDGES_FILE)
    queries = pick_queries(judges, MAX_QUERIES)

    print(f"Vectoria: {VECTORIA_URL}  index={VECTORIA_INDEX}")
    print(f"Judges  : {JUDGES_FILE}  ({len(judges)} total, using top {len(queries)} by #relevant)")
    print(f"Window  : top_k={TOP_K}  ndcg@{TOP_N}")
    print()

    rows = []
    for jq in queries:
        query    = jq["query"]
        relevant = set(jq["relevant_ids"])
        print(f"  {query!r} ({len(relevant)} relevant)...", end=" ", flush=True)

        try:
            hits = vectoria_search(query, VECTORIA_INDEX, TOP_K)
        except Exception as e:
            print(f"VECTORIA ERROR: {e}")
            continue

        if not hits:
            print("0 hits — skipped")
            continue

        if hits_in_top_k(hits, relevant, TOP_K) == 0:
            print(f"hits={len(hits)} but 0 judged-relevant in top-{TOP_K} — skipped")
            continue

        t1       = time.time()
        reranked = ce_rerank(query, hits, model)
        latency  = (time.time() - t1) * 1000

        before = ndcg_at_k(hits,     relevant, k=TOP_N)
        after  = ndcg_at_k(reranked, relevant, k=TOP_N)
        delta  = after - before
        top_s  = reranked[0]["_ce_score"] if reranked else 0.0

        rows.append({
            "query":    query,
            "n_rel":    len(relevant),
            "ret_rel":  hits_in_top_k(hits, relevant, TOP_K),
            "hits":     len(hits),
            "before":   before,
            "after":    after,
            "delta":    delta,
            "top_s":    top_s,
            "latency":  latency,
        })
        print(f"hits={len(hits)}  ndcg_before={before:.3f}  ndcg_after={after:.3f}  "
              f"delta={delta:+.3f}  latency={latency:.0f}ms")

    if not rows:
        print("No evaluable queries.")
        return

    print()
    print("─" * 96)
    print(f"{'Query':<40} {'N-rel':>5} {'Ret':>4} {'Before':>7} {'After':>7} {'Δ':>7} {'ms':>5}")
    print("─" * 96)
    for r in rows:
        print(f"{r['query']:<40} {r['n_rel']:>5} {r['ret_rel']:>4} "
              f"{r['before']:>7.3f} {r['after']:>7.3f} {r['delta']:>+7.3f} {r['latency']:>5.0f}")
    print("─" * 96)

    avg_before  = sum(r["before"]  for r in rows) / len(rows)
    avg_after   = sum(r["after"]   for r in rows) / len(rows)
    avg_delta   = avg_after - avg_before
    avg_latency = sum(r["latency"] for r in rows) / len(rows)
    wins   = sum(1 for r in rows if r["delta"] >  0.001)
    losses = sum(1 for r in rows if r["delta"] < -0.001)
    ties   = len(rows) - wins - losses
    print(f"{'MEAN':<40} {'':>5} {'':>4} {avg_before:>7.3f} {avg_after:>7.3f} {avg_delta:>+7.3f} {avg_latency:>5.0f}")
    print(f"  CE improves: {wins}  ties: {ties}  hurts: {losses}  (threshold 0.001)")
    print(f"  Model: {CE_MODEL}")


if __name__ == "__main__":
    main()
