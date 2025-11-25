# Multi-stage Dockerfile for nilAV project
# Builds all binaries: nilav_node, nilcc_simulator, contract_cli, monitor

FROM rust:1.85-slim-bookworm AS builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy project files
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config ./config
COPY data ./data
COPY out ./out

# Build all binaries with caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bins && \
    mkdir -p /out/bin && \
    cp target/release/nilav_node /out/bin/ && \
    cp target/release/nilcc_simulator /out/bin/ && \
    cp target/release/contract_cli /out/bin/ && \
    cp target/release/monitor /out/bin/

# Runtime stage for nilav_node
FROM debian:bookworm-slim AS nilav_node
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -m appuser
COPY --from=builder /out/bin/nilav_node /usr/local/bin/nilav_node
USER appuser
ENTRYPOINT ["/usr/local/bin/nilav_node"]

# Runtime stage for nilcc_simulator
FROM debian:bookworm-slim AS nilcc_simulator
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -m appuser
COPY --from=builder /out/bin/nilcc_simulator /usr/local/bin/nilcc_simulator
COPY --from=builder /app/config /app/config
COPY --from=builder /app/data /app/data
USER appuser
ENV CONFIG_PATH=/app/config/config.toml
ENV HTXS_PATH=/app/data/htxs.json
ENTRYPOINT ["/usr/local/bin/nilcc_simulator"]

# Runtime stage for contract_cli (used for deployment)
FROM debian:bookworm-slim AS contract_cli
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -m appuser
COPY --from=builder /out/bin/contract_cli /usr/local/bin/contract_cli
USER appuser
ENTRYPOINT ["/usr/local/bin/contract_cli"]

# Runtime stage for monitor
FROM debian:bookworm-slim AS monitor
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -m appuser
COPY --from=builder /out/bin/monitor /usr/local/bin/monitor
USER appuser
ENTRYPOINT ["/usr/local/bin/monitor"]
