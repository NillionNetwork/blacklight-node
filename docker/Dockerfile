# Multi-stage Dockerfile for blacklight project with cross-compilation support
# Builds all binaries: blacklight_node, nilcc_simulator, monitor, keeper

FROM --platform=$BUILDPLATFORM rust:1.91-slim-trixie AS builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM
ARG BLACKLIGHT_VERSION

WORKDIR /app

# Install base build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

# Set up cross-compilation based on target platform
RUN case "$TARGETPLATFORM" in \
    "linux/amd64") \
        apt-get update && apt-get install -y libssl-dev && \
        echo "x86_64-unknown-linux-gnu" > /rust-target.txt && \
        echo "x86_64-linux-gnu" > /gcc-prefix.txt \
        ;; \
    "linux/arm64") \
        dpkg --add-architecture arm64 && \
        apt-get update && apt-get install -y \
            gcc-aarch64-linux-gnu \
            g++-aarch64-linux-gnu \
            libssl-dev:arm64 \
        && echo "aarch64-unknown-linux-gnu" > /rust-target.txt && \
        echo "aarch64-linux-gnu" > /gcc-prefix.txt \
        ;; \
    *) echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac && rm -rf /var/lib/apt/lists/*

# Install Rust target
RUN rustup target add $(cat /rust-target.txt)

# Configure cargo for cross-compilation
RUN mkdir -p ~/.cargo && \
    RUST_TARGET=$(cat /rust-target.txt) && \
    GCC_PREFIX=$(cat /gcc-prefix.txt) && \
    if [ "$RUST_TARGET" = "aarch64-unknown-linux-gnu" ]; then \
        printf "[target.aarch64-unknown-linux-gnu]\n" > ~/.cargo/config.toml && \
        printf "linker = \"${GCC_PREFIX}-gcc\"\n" >> ~/.cargo/config.toml && \
        printf "\n[env]\n" >> ~/.cargo/config.toml && \
        printf "PKG_CONFIG_ALLOW_CROSS = \"1\"\n" >> ~/.cargo/config.toml && \
        printf "PKG_CONFIG_PATH = \"/usr/lib/aarch64-linux-gnu/pkgconfig\"\n" >> ~/.cargo/config.toml && \
        printf "OPENSSL_DIR = \"/usr\"\n" >> ~/.cargo/config.toml && \
        printf "OPENSSL_LIB_DIR = \"/usr/lib/aarch64-linux-gnu\"\n" >> ~/.cargo/config.toml && \
        printf "OPENSSL_INCLUDE_DIR = \"/usr/include\"\n" >> ~/.cargo/config.toml && \
        cat ~/.cargo/config.toml; \
    fi

# Copy project files
COPY Cargo.toml ./
COPY crates ./crates
COPY nilcc-simulator ./nilcc-simulator
COPY keeper ./keeper
COPY blacklight-node ./blacklight-node
COPY monitor ./monitor
COPY data ./data

WORKDIR /app

# Build Rust binaries for target platform
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    RUST_TARGET=$(cat /rust-target.txt) && \
    export BLACKLIGHT_VERSION="${BLACKLIGHT_VERSION}" && \
    if [ "$RUST_TARGET" = "aarch64-unknown-linux-gnu" ]; then \
        export PKG_CONFIG_ALLOW_CROSS=1 && \
        export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig && \
        export PKG_CONFIG_SYSROOT_DIR=/ && \
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc && \
        export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc && \
        export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar && \
        export OPENSSL_DIR=/usr && \
        export OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu && \
        export OPENSSL_INCLUDE_DIR=/usr/include; \
    fi && \
    cargo build --release --target $RUST_TARGET && \
    mkdir -p /out/bin && \
    cp target/$RUST_TARGET/release/blacklight-node /out/bin/ && \
    cp target/$RUST_TARGET/release/nilcc-simulator /out/bin/ && \
    cp target/$RUST_TARGET/release/monitor /out/bin/ && \
    cp target/$RUST_TARGET/release/keeper /out/bin/


FROM --platform=$TARGETPLATFORM debian:bookworm-slim AS base_release

ARG TARGETPLATFORM

RUN apt-get update && apt-get install -y ca-certificates util-linux && rm -rf /var/lib/apt/lists/* && \
    mkdir -p /app
WORKDIR /app


# Runtime stage for blacklight_node
FROM base_release AS blacklight_node
COPY --from=builder /out/bin/blacklight-node /usr/local/bin/blacklight-node
ENTRYPOINT ["/usr/local/bin/blacklight-node"]

# Foundry (cast) stage (for deriving keys from mnemonics at runtime)
FROM ghcr.io/foundry-rs/foundry:stable AS foundry

# Runtime stage for blacklight_node
FROM blacklight_node AS blacklight_node_test
COPY --from=foundry /usr/local/bin/cast /usr/local/bin/cast
COPY --chmod=755 docker/entrypoints/derive_private_key.sh /usr/local/bin/derive_private_key.sh
ENTRYPOINT ["/usr/local/bin/derive_private_key.sh", "/usr/local/bin/blacklight-node"]

# Runtime stage for nilcc_simulator
FROM base_release AS nilcc_simulator
COPY --from=builder /out/bin/nilcc-simulator /usr/local/bin/nilcc-simulator
COPY --from=builder /app/data /app/data
ENV HTXS_PATH=/app/data/htxs.json
ENTRYPOINT ["/usr/local/bin/nilcc-simulator"]

# Runtime stage for monitor
FROM base_release AS monitor
COPY --from=builder /out/bin/monitor /usr/local/bin/monitor
ENTRYPOINT ["/usr/local/bin/monitor"]

# Runtime stage for keeper
FROM base_release AS keeper
COPY --from=builder /out/bin/keeper /usr/local/bin/keeper
ENTRYPOINT ["/usr/local/bin/keeper"]

