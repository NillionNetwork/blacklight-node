# Multi-stage Dockerfile for nilAV project
# Builds all binaries: nilav_node, nilcc_simulator, contract_cli, monitor

FROM rust:1.85-slim-bookworm AS builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl git \
    && rm -rf /var/lib/apt/lists/*

# Install Foundry toolchain
RUN curl -L https://foundry.paradigm.xyz | bash && \
    /root/.foundry/bin/foundryup
ENV PATH="/root/.foundry/bin:${PATH}"

# Copy project files
COPY Cargo.toml ./
COPY src ./src
COPY config ./config
COPY data ./data
COPY ./contracts/ ./contracts/

WORKDIR /app/contracts/

RUN forge build 

WORKDIR /app
# Build all binaries with caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bins && \
    mkdir -p /out/bin && \
    cp target/release/nilav_node /out/bin/ && \
    cp target/release/nilcc_simulator /out/bin/ && \
    cp target/release/contract_cli /out/bin/ && \
    cp target/release/monitor /out/bin/


FROM debian:bookworm-slim AS base_release
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/* && useradd -u 10001 -m appuser
USER appuser

# Runtime stage for nilav_node
FROM base_release AS nilav_node
COPY --from=builder /out/bin/nilav_node /usr/local/bin/nilav_node
ENTRYPOINT ["/usr/local/bin/nilav_node"]

# Runtime stage for nilcc_simulator
FROM base_release AS nilcc_simulator
COPY --from=builder /out/bin/nilcc_simulator /usr/local/bin/nilcc_simulator
COPY --from=builder /app/config /app/config
COPY --from=builder /app/data /app/data
ENV CONFIG_PATH=/app/config/config.toml
ENV HTXS_PATH=/app/data/htxs.json
ENTRYPOINT ["/usr/local/bin/nilcc_simulator"]

# Runtime stage for contract_cli (used for deployment)
FROM base_release AS contract_cli
COPY --from=builder /out/bin/contract_cli /usr/local/bin/contract_cli
ENTRYPOINT ["/usr/local/bin/contract_cli"]

# Runtime stage for monitor
FROM base_release AS monitor
COPY --from=builder /out/bin/monitor /usr/local/bin/monitor
ENTRYPOINT ["/usr/local/bin/monitor"]
