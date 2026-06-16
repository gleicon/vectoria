# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.80-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation: copy manifests first, then build deps only.
COPY Cargo.toml Cargo.lock ./
COPY vectoria-core/Cargo.toml vectoria-core/
COPY vectoria-server/Cargo.toml vectoria-server/
COPY vectoria-cli/Cargo.toml vectoria-cli/

RUN mkdir -p vectoria-core/src vectoria-server/src vectoria-cli/src \
    && echo "pub fn main() {}" > vectoria-server/src/main.rs \
    && echo "pub fn main() {}" > vectoria-cli/src/main.rs \
    && echo "" > vectoria-core/src/lib.rs \
    && cargo build --release -p vectoria-server -p vectoria-cli 2>/dev/null || true \
    && rm -rf vectoria-core/src vectoria-server/src vectoria-cli/src

COPY vectoria-core/src/ vectoria-core/src/
COPY vectoria-server/src/ vectoria-server/src/
COPY vectoria-cli/src/ vectoria-cli/src/

RUN cargo build --release -p vectoria-server -p vectoria-cli

# ── Stage 2: Runtime (full — ONNX model downloaded at first start) ────────────
FROM debian:bookworm-slim AS vectoria-full

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/vectoria-server /usr/local/bin/vectoria-server
COPY --from=builder /build/target/release/vectoria        /usr/local/bin/vectoria

RUN mkdir -p /data /root/.cache/fastembed
WORKDIR /data

EXPOSE 7700

ENV VECTORIA_STORAGE_PATH=/data/vectoria.db
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
COPY --from=builder /build/target/release/vectoria        /usr/local/bin/vectoria

RUN mkdir -p /data
WORKDIR /data

EXPOSE 7700

ENV VECTORIA_STORAGE_PATH=/data/vectoria.db
ENV VECTORIA_EMBEDDING_PROVIDER=openai-compatible
ENV VECTORIA_SKIP_CONSENT=1

VOLUME ["/data"]

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://localhost:7700/health || exit 1

ENTRYPOINT ["/usr/local/bin/vectoria-server"]
