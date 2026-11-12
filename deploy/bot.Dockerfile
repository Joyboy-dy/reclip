# syntax=docker/dockerfile:1
# ReClip Telegram bot image. Build context = repository root:
#   docker build -f deploy/bot.Dockerfile -t reclip-bot:dev .

# ---- Rust: cargo-chef caches the dependency build ---------------------------
FROM lukemathwalker/cargo-chef:latest-rust-1.94-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p reclip-bot --bin reclip-bot --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p reclip-bot --bin reclip-bot \
    && mkdir -p /out/var/lib/reclip-bot

# ---- Final image: distroless (glibc + CA certificates, no shell) -------------
FROM gcr.io/distroless/cc-debian12:nonroot

# Same uid as the core image (1000), so the bot can read the key file the core
# writes in the shared "secrets" volume. The state directory is created with
# that owner so a fresh named volume inherits it.
COPY --from=builder --chown=1000:1000 /out/var/lib/reclip-bot /var/lib/reclip-bot
COPY --from=builder /app/target/release/reclip-bot /usr/local/bin/reclip-bot

ENV RECLIP_BOT_STATE=/var/lib/reclip-bot/state.json

USER 1000:1000
VOLUME /var/lib/reclip-bot

# No HEALTHCHECK: the bot has no HTTP port and the image has no shell.
# Docker restarts it (restart: unless-stopped) when the process exits.
ENTRYPOINT ["/usr/local/bin/reclip-bot"]
