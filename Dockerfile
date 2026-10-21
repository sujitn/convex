# syntax=docker/dockerfile:1

# Convex Pricing Server - Multi-stage Dockerfile for Fly.io deployment
# Build: docker build -t convex-showcase .
# The image runs the read-only showcase: Caddy on :8080 in front of a loopback
# server. See deploy/showcase/README.md.

# ==============================================================================
# Stage 1: Build environment
# ==============================================================================
FROM rust:latest AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests first (for better layer caching)
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY tools/library/api-capabilities.json tools/library/api-capabilities.json

# Build the server in release mode
RUN cargo build --locked --release -p convex-server --bin convex-server --bin convex-core-inputs

# ==============================================================================
# Stage 2: Runtime environment
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies (the server uses rustls; no OpenSSL needed)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    tini \
    && rm -rf /var/lib/apt/lists/*

# Caddy: public edge on :8080, read-only allowlist in front of convex-server
COPY --from=caddy:2.8.4 /usr/bin/caddy /usr/bin/caddy

# Create non-root user
RUN useradd -m -u 1000 convex

# Create data directory
RUN mkdir -p /app/data/caddy && chown -R convex:convex /app

WORKDIR /app

# Server, offline seed importer and showcase deploy files
COPY --from=builder /app/target/release/convex-server /app/convex-server
COPY --from=builder /app/target/release/convex-core-inputs /app/convex-core-inputs
COPY deploy/showcase/ /app/showcase/
RUN chmod 0755 /app/showcase/entrypoint.sh

USER convex

# Server config comes from /app/showcase/server.toml (the server rejects
# unknown CONVEX_* env vars, so none are set here).
ENV XDG_DATA_HOME=/app/data/caddy \
    XDG_CONFIG_HOME=/app/data/caddy \
    RUST_LOG=info

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1

# -g: SIGTERM reaches the whole group, so convex-server drains alongside Caddy.
ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/app/showcase/entrypoint.sh"]
