# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Passed by deploy.sh; changing it invalidates the cargo build cache layer on version bumps.
ARG CARGO_VERSION=dev

COPY Cargo.toml Cargo.lock ./
COPY vectoria-core/ vectoria-core/
COPY vectoria-server/ vectoria-server/
# CLI and WASM manifests copied for workspace resolution; neither is compiled here.
# Skipping -p vectoria-cli avoids parquet/arrow deps (~15min extra build time).
COPY vectoria-cli/ vectoria-cli/
COPY vectoria-wasm/ vectoria-wasm/

RUN cargo build --release -p vectoria-server

# ── Stage 2: Runtime (full — ONNX model downloaded at first start) ────────────
FROM debian:bookworm-slim AS vectoria-full

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/vectoria-server /usr/local/bin/vectoria-server

RUN mkdir -p /data /root/.cache/fastembed
WORKDIR /data

EXPOSE 7700

ENV VECTORIA_STORAGE_PATH=/data/vectoria
ENV VECTORIA_EMBEDDING_PROVIDER=local
ENV VECTORIA_SKIP_CONSENT=1

VOLUME ["/data", "/root/.cache/fastembed"]

HEALTHCHECK --interval=10s --timeout=3s --start-period=30s --retries=3 \
  CMD curl -fsS http://localhost:7700/health || exit 1

ENTRYPOINT ["/usr/local/bin/vectoria-server"]

# ── Stage 3: Runtime (slim — external OpenAI-compatible embedding required) ───
FROM debian:bookworm-slim AS vectoria-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/vectoria-server /usr/local/bin/vectoria-server

RUN mkdir -p /data
WORKDIR /data

EXPOSE 7700

ENV VECTORIA_STORAGE_PATH=/data/vectoria
ENV VECTORIA_EMBEDDING_PROVIDER=openai-compatible
ENV VECTORIA_SKIP_CONSENT=1

VOLUME ["/data"]

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://localhost:7700/health || exit 1

ENTRYPOINT ["/usr/local/bin/vectoria-server"]
