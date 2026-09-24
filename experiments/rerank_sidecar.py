#!/usr/bin/env python3
"""
Minimal reranker sidecar for Vectoria.

Exposes a /rerank endpoint that Vectoria can POST candidates to and receive
sorted IDs back. Configure in vectoria.toml:

    [reranker]
    url = "http://localhost:8001/rerank"

Usage:
    pip install fastapi uvicorn sentence-transformers
    KMP_DUPLICATE_LIB_OK=TRUE python3 experiments/rerank_sidecar.py

    # Custom model or port:
    CE_MODEL=cross-encoder/mmarco-mMiniLMv2-L12-H384-v1 PORT=8001 \
    KMP_DUPLICATE_LIB_OK=TRUE python3 experiments/rerank_sidecar.py
"""

import os
from contextlib import asynccontextmanager

import uvicorn
from fastapi import FastAPI
from pydantic import BaseModel
from sentence_transformers import CrossEncoder

CE_MODEL = os.getenv("CE_MODEL", "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1")
PORT     = int(os.getenv("PORT", "8001"))

model: CrossEncoder | None = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global model
    print(f"Loading {CE_MODEL}…")
    model = CrossEncoder(CE_MODEL, max_length=512)
    print("Reranker ready.")
    yield


app = FastAPI(lifespan=lifespan)


class Hit(BaseModel):
    id: str
    title: str = ""
    description: str = ""


class RerankRequest(BaseModel):
    query: str
    hits: list[Hit]


class RerankResponse(BaseModel):
    ranked_ids: list[str]
    model: str


@app.post("/rerank", response_model=RerankResponse)
def rerank(req: RerankRequest) -> RerankResponse:
    if not req.hits:
        return RerankResponse(ranked_ids=[], model=CE_MODEL)

    pairs  = [[req.query, f"{h.title}. {h.description}".strip(". ")] for h in req.hits]
    scores = model.predict(pairs, show_progress_bar=False)
    ranked = sorted(zip(req.hits, scores), key=lambda x: -x[1])
    return RerankResponse(
        ranked_ids=[h.id for h, _ in ranked],
        model=CE_MODEL,
    )


@app.get("/health")
def health():
    return {"status": "ok", "model": CE_MODEL, "ready": model is not None}


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=PORT)
