#!/usr/bin/env python3
"""
Qualitative pt-BR evaluation: compare multiple cross-encoder models on Portuguese
shoe/apparel queries against a Vectoria index. No ground-truth labels — outputs
top-5 before/after per query and model confidence scores for manual inspection.

Usage:
    KMP_DUPLICATE_LIB_OK=TRUE \
    VECTORIA_URL=http://localhost:7700 \
    VECTORIA_API_KEY=test-key \
    VECTORIA_INDEX=esci-eval \
    python3 experiments/ptbr_qualitative_eval.py
"""

import os, sys, time
import requests
from sentence_transformers import CrossEncoder

VECTORIA_URL   = os.getenv("VECTORIA_URL",    "http://localhost:7700")
VECTORIA_KEY   = os.getenv("VECTORIA_API_KEY", "test-key")
VECTORIA_INDEX = os.getenv("VECTORIA_INDEX",   "esci-eval")
TOP_K          = 20

CE_MODELS = [
    "cross-encoder/ms-marco-MiniLM-L-6-v2",
    "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1",
    "BAAI/bge-reranker-v2-m3",
]
CE_MODELS = [m for m in CE_MODELS if m in os.getenv("CE_MODELS", ",".join(CE_MODELS)).split(",")]

PTBR_QUERIES = [
    "tênis de corrida masculino",
    "tenis corrida masculino",          # without diacritics
    "sandália rasteira feminina",
    "bota de couro masculina",
    "tênis infantil escola",
    "sapato social masculino preto",
    "chinelo de borracha masculino",
    "tênis feminino academia",
]

def vectoria_search(query: str, limit: int = TOP_K) -> list[dict]:
    if VECTORIA_INDEX and VECTORIA_INDEX != "default":
        url  = f"{VECTORIA_URL}/indexes/{VECTORIA_INDEX}/search"
        body = {"q": query, "limit": limit}
    else:
        url  = f"{VECTORIA_URL}/search"
        body = {"q": query, "index": VECTORIA_INDEX, "limit": limit}
    r = requests.post(url, json=body, headers={"Authorization": f"Bearer {VECTORIA_KEY}"}, timeout=15)
    r.raise_for_status()
    return r.json().get("hits", [])

def doc_text(hit: dict) -> str:
    m = hit.get("metadata", {})
    return f"{m.get('title','')}. {m.get('description','') or m.get('text','')}".strip(". ")

def ce_rerank(query: str, hits: list[dict], model: CrossEncoder) -> list[dict]:
    if not hits:
        return []
    scores = model.predict([[query, doc_text(h)] for h in hits], show_progress_bar=False)
    scored = [{**h, "_ce": float(s)} for h, s in zip(hits, scores)]
    scored.sort(key=lambda h: -h["_ce"])
    return scored

def show_top(hits: list[dict], n: int = 5, score_key: str = "score") -> None:
    for i, h in enumerate(hits[:n], 1):
        title = h.get("metadata", {}).get("title", h.get("id", "?"))[:55]
        score = h.get(score_key, 0.0)
        print(f"    {i}. [{score:.3f}] {title}")

def main():
    models = {}
    for name in CE_MODELS:
        print(f"Loading {name}...")
        t0 = time.time()
        models[name] = CrossEncoder(name, max_length=512)
        print(f"  loaded in {time.time()-t0:.1f}s")
    print()

    for query in PTBR_QUERIES:
        print(f"━━━ {query!r} ━━━")
        try:
            hits = vectoria_search(query, TOP_K)
        except Exception as e:
            print(f"  ERROR: {e}")
            continue
        if not hits:
            print("  0 hits")
            continue

        print(f"  Vectoria baseline ({len(hits)} hits):")
        show_top(hits)

        for name, model in models.items():
            reranked = ce_rerank(query, hits, model)
            label = name.split("/")[-1]
            print(f"  {label}:")
            show_top(reranked, score_key="_ce")
        print()

if __name__ == "__main__":
    main()
