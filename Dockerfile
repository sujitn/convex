# syntax=docker/dockerfile:1

# Convex Pricing Server - Multi-stage Dockerfile for Fly.io deployment
# Build: docker build -t convex-server .
# Run:   docker run -p 8080:8080 convex-server

# ==============================================================================
# Stage 1: Build environment
# ==============================================================================
FROM rust:1.83-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests first (for better layer caching)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build the server in release mode
RUN cargo build --release -p convex-server

# ==============================================================================
# Stage 2: Runtime environment
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 convex

# Create data directory
RUN mkdir -p /app/data && chown -R convex:convex /app

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/convex-server /app/convex-server

# Switch to non-root user
USER convex

# Fly.io uses PORT env var, map it to CONVEX_PORT
# Default to 8080 if PORT not set
ENV CONVEX_HOST=0.0.0.0
ENV CONVEX_PORT=8080
ENV CONVEX_STORAGE_PATH=/app/data/convex.redb
ENV RUST_LOG=info,convex=debug

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:${CONVEX_PORT}/health || exit 1

# Run the server
# Note: Fly.io sets PORT env var, we use a shell to remap it
CMD ["sh", "-c", "CONVEX_PORT=${PORT:-8080} /app/convex-server"]
