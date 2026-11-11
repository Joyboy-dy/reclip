# syntax=docker/dockerfile:1
# ReClip core image. Build context = repository root:
#   docker build -f deploy/core.Dockerfile -t reclip-core:dev .

# ---- Rust: cargo-chef caches the dependency build ---------------------------
FROM lukemathwalker/cargo-chef:latest-rust-1.94-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p reclip-core --bin reclip-core --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p reclip-core --bin reclip-core

# ---- Frontend (Bun) ----------------------------------------------------------
FROM oven/bun:1.3.14 AS web
WORKDIR /web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web ./
RUN bun run build

# ---- Tools: yt-dlp (latest release) -----------------------------------------
FROM debian:bookworm-slim AS tools
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN case "${TARGETARCH:-amd64}" in \
        arm64) asset=yt-dlp_linux_aarch64 ;; \
        *) asset=yt-dlp_linux ;; \
    esac \
    && curl -fsSL -o /yt-dlp "https://github.com/yt-dlp/yt-dlp/releases/latest/download/${asset}" \
    && chmod 0755 /yt-dlp

# ---- Final image -------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# JavaScript runtime used by yt-dlp to solve YouTube's challenges.
# Deno is yt-dlp's default and recommended runtime.
COPY --from=denoland/deno:bin-2.5.6 /deno /usr/local/bin/deno
COPY --from=mwader/static-ffmpeg:7.1.1 /ffmpeg /ffprobe /usr/local/bin/

RUN useradd -m -u 1000 reclip \
    && mkdir -p /opt/yt-dlp /data /app/web \
    && chown -R reclip:reclip /opt/yt-dlp /data

# Owned by reclip so `yt-dlp -U` (entrypoint) can replace the binary.
COPY --from=tools --chown=reclip:reclip /yt-dlp /opt/yt-dlp/yt-dlp
COPY --from=builder /app/target/release/reclip-core /usr/local/bin/reclip-core
COPY --from=web /web/dist /app/web
COPY deploy/entrypoint.sh /app/entrypoint.sh
RUN chmod 0755 /app/entrypoint.sh

ENV PATH=/opt/yt-dlp:$PATH \
    RECLIP_DATA_DIR=/data \
    RECLIP_WEB_DIR=/app/web

USER reclip
WORKDIR /app
VOLUME /data
EXPOSE 8899

HEALTHCHECK --interval=30s --timeout=5s --start-period=40s --retries=3 \
    CMD ["reclip-core", "healthcheck"]

ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["reclip-core"]
