#!/usr/bin/env python3
"""
Jev reranking experiment for Vectoria — ESCI ground-truth evaluation.

Fetches top-30 candidates from Vectoria hybrid search, reranks with Jev (TypeSafe),
and reports NDCG@10 before/after using ESCI judges file as ground truth.

Usage:
    pip install typesafe_sdk requests
    export TYPESAFE_API_KEY=your_key_here
    python experiments/jev_rerank_eval.py

    # Against a specific Vectoria endpoint / index with a judges file:
    VECTORIA_URL=http://localhost:7700 \
    VECTORIA_API_KEY=test-key \
    VECTORIA_INDEX=esci-eval \
    JUDGES_FILE=data/esci/judges.ndjson \
    python experiments/jev_rerank_eval.py
"""

import json
import math
import os
import sys

import requests
from typesafe_sdk import Noul, RetryPolicy, TypeSafeClient

# ── Config ───────────────────────────────────────────────────────────────────

VECTORIA_URL   = os.getenv("VECTORIA_URL",    "https://demo.vectoriasearch.com")
VECTORIA_KEY   = os.getenv("VECTORIA_API_KEY", "vectoria-demo")
VECTORIA_INDEX = os.getenv("VECTORIA_INDEX",   "shoestore")
TYPESAFE_KEY   = os.getenv("TYPESAFE_API_KEY")
JUDGES_FILE    = os.getenv("JUDGES_FILE",      "data/esci/judges.ndjson")
TOP_K          = 30   # candidates fetched from Vectoria
TOP_N          = 10   # window for NDCG / MRR
MAX_QUERIES    = int(os.getenv("MAX_QUERIES", "20"))

# Jev relevance question — domain-neutral, from hev/jev-rerank (Apache-2.0)
JEV_QUESTION = (
    "Document `documents.{id}` is relevant to `query`: "
    "it contains information that answers or directly addresses it."
)
JEV_CRITERIA = {
    "true": (
        "The document contains information that answers the query "
        "or directly addresses what it asks about."
    ),
    "false": (
        "The document is only loosely related, on a similar topic, "
        "or does not address what the query asks."
    ),
}

# ── Judges ────────────────────────────────────────────────────────────────────

def load_judges(path: str) -> list[dict]:
    """Load ESCI judged queries. Each entry: {query, relevant_ids, k}."""
    judges = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                judges.append(json.loads(line))
    return judges


def pick_queries(judges: list[dict], max_q: int) -> list[dict]:
    """Pick queries that have enough judged relevant products to be informative."""
    # Prefer queries with 3+ relevant products so NDCG@10 has signal
    ranked = sorted(judges, key=lambda j: len(j["relevant_ids"]), reverse=True)
    return ranked[:max_q]


# ── Vectoria search ───────────────────────────────────────────────────────────

def vectoria_search(query: str, index: str, limit: int = TOP_K) -> list[dict]:
    # Named indexes use /indexes/{name}/search; default uses /search
    if index and index != "default":
        url = f"{VECTORIA_URL}/indexes/{index}/search"
        body = {"q": query, "limit": limit}
    else:
        url = f"{VECTORIA_URL}/search"
        body = {"q": query, "index": index, "limit": limit}
    resp = requests.post(
        url,
        json=body,
        headers={"Authorization": f"Bearer {VECTORIA_KEY}"},
        timeout=15,
    )
    resp.raise_for_status()
    return resp.json().get("hits", [])


# ── Jev reranking ─────────────────────────────────────────────────────────────

def jev_rerank(query: str, hits: list[dict], client: TypeSafeClient) -> list[dict]:
    if not hits:
        return []

    doc_ids = {f"D{i:02d}": i for i in range(len(hits))}
    state = {
        "query": query,
        "documents": {
            k: {
                "title": hits[i].get("metadata", {}).get("title", ""),
                "text":  hits[i].get("metadata", {}).get("description", "")
                         or hits[i].get("metadata", {}).get("text", ""),
            }
            for k, i in doc_ids.items()
        },
    }
    questions = {
        k: Noul(instructions=JEV_QUESTION.format(id=k), criteria=JEV_CRITERIA)
        for k in doc_ids
    }

    result = client.system_one(state=state, questions=questions, model="jev-latest")

    scored = []
    for k, i in doc_ids.items():
        prob = float(result.answers[k].noul)
        scored.append({**hits[i], "_jev_score": prob, "_jev_doc_id": k})

    scored.sort(key=lambda h: -h["_jev_score"])
    return scored


# ── Metrics (ESCI ground truth) ───────────────────────────────────────────────

def _dcg(relevances: list[float]) -> float:
    return sum(r / math.log2(i + 2) for i, r in enumerate(relevances))


def ndcg_at_k(ranked: list[dict], relevant_ids: set[str], k: int = TOP_N) -> float:
    """NDCG@k using ESCI binary relevance (1 if id in relevant_ids, else 0)."""
    gains = [1.0 if h["id"] in relevant_ids else 0.0 for h in ranked[:k]]
    # Ideal: put all 1s first
    n_rel_in_top = min(sum(gains), k)
    ideal = [1.0] * int(n_rel_in_top) + [0.0] * (k - int(n_rel_in_top))
    actual_dcg = _dcg(gains)
    ideal_dcg  = _dcg(ideal)
    return actual_dcg / ideal_dcg if ideal_dcg > 0 else 0.0


def hits_in_top_k(ranked: list[dict], relevant_ids: set[str], k: int = TOP_N) -> int:
    """Count relevant documents in top-k results."""
    return sum(1 for h in ranked[:k] if h["id"] in relevant_ids)


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    if not TYPESAFE_KEY:
        sys.exit("Set TYPESAFE_API_KEY env var before running.")

    if not os.path.exists(JUDGES_FILE):
        sys.exit(f"Judges file not found: {JUDGES_FILE}\n"
                 "Run: make esci-judges  (or set JUDGES_FILE=path/to/judges.ndjson)")

    judges = load_judges(JUDGES_FILE)
    queries = pick_queries(judges, MAX_QUERIES)

    client = TypeSafeClient(
        api_key=TYPESAFE_KEY,
        retry=RetryPolicy(max_retries=3, backoff_max=10.0, timeout=60.0),
    )

    print(f"Vectoria: {VECTORIA_URL}  index={VECTORIA_INDEX}")
    print(f"Judges  : {JUDGES_FILE}  ({len(judges)} total, using top {len(queries)} by #relevant)")
    print(f"Window  : top_k={TOP_K}  ndcg@{TOP_N}")
    print()

    rows = []
    for jq in queries:
        query       = jq["query"]
        relevant    = set(jq["relevant_ids"])
        print(f"  {query!r} ({len(relevant)} relevant)...", end=" ", flush=True)

        try:
            hits = vectoria_search(query, VECTORIA_INDEX, TOP_K)
        except Exception as e:
            print(f"VECTORIA ERROR: {e}")
            continue

        if not hits:
            print("0 hits — skipped")
            continue

        # How many of the judged relevant products appear in the top-30 candidates?
        retrieved_rel = hits_in_top_k(hits, relevant, TOP_K)
        if retrieved_rel == 0:
            print(f"hits={len(hits)} but 0 judged-relevant in top-{TOP_K} — skipped")
            continue

        try:
            reranked = jev_rerank(query, hits, client)
        except Exception as e:
            print(f"JEV ERROR: {e}")
            continue

        before_ndcg = ndcg_at_k(hits,     relevant, k=TOP_N)
        after_ndcg  = ndcg_at_k(reranked, relevant, k=TOP_N)
        delta       = after_ndcg - before_ndcg
        top_p       = reranked[0]["_jev_score"] if reranked else 0.0

        rows.append({
            "query":        query,
            "n_relevant":   len(relevant),
            "retrieved_rel": retrieved_rel,
            "hits":         len(hits),
            "before_ndcg":  before_ndcg,
            "after_ndcg":   after_ndcg,
            "delta":        delta,
            "top_p":        top_p,
        })
        print(f"hits={len(hits)}  rel_in_top30={retrieved_rel}  "
              f"ndcg_before={before_ndcg:.3f}  ndcg_after={after_ndcg:.3f}  "
              f"delta={delta:+.3f}  top_p={top_p:.3f}")

    if not rows:
        print("No evaluable queries.")
        return

    print()
    print("─" * 92)
    print(f"{'Query':<38} {'N-rel':>5} {'Ret':>4} {'Before':>7} {'After':>7} {'Δ':>7} {'Top-P':>6}")
    print("─" * 92)
    for r in rows:
        print(f"{r['query']:<38} {r['n_relevant']:>5} {r['retrieved_rel']:>4} "
              f"{r['before_ndcg']:>7.3f} {r['after_ndcg']:>7.3f} {r['delta']:>+7.3f} {r['top_p']:>6.3f}")
    print("─" * 92)

    avg_before = sum(r["before_ndcg"] for r in rows) / len(rows)
    avg_after  = sum(r["after_ndcg"]  for r in rows) / len(rows)
    avg_delta  = avg_after - avg_before
    wins   = sum(1 for r in rows if r["delta"] > 0.001)
    losses = sum(1 for r in rows if r["delta"] < -0.001)
    ties   = len(rows) - wins - losses
    print(f"{'MEAN':<38} {'':>5} {'':>4} {avg_before:>7.3f} {avg_after:>7.3f} {avg_delta:>+7.3f}")
    print(f"  Jev improves: {wins}  ties: {ties}  hurts: {losses}  (threshold 0.001)")


if __name__ == "__main__":
    main()
