# ── Stage 1: Fetch release binary ─────────────────────────────────────────────
FROM debian:bookworm-slim AS fetcher

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

ARG VERSION=0.1.24
RUN curl -fsSL https://github.com/gleicon/vectoria/releases/download/v${VERSION}/vectoria-linux-amd64.tar.gz \
    | tar -xz -C /usr/local/bin ./vectoria-server \
    && chmod +x /usr/local/bin/vectoria-server

# ── Stage 2: Runtime (full — ONNX model downloaded at first start) ────────────
FROM debian:bookworm-slim AS vectoria-full

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=fetcher /usr/local/bin/vectoria-server /usr/local/bin/vectoria-server

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

COPY --from=fetcher /usr/local/bin/vectoria-server /usr/local/bin/vectoria-server

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
