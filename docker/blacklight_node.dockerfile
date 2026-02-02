FROM rust:1.91-alpine AS builder

ARG BLACKLIGHT_VERSION

WORKDIR /app

RUN apk add --no-cache musl-dev git pkgconf openssl-dev

# Copy project files
COPY Cargo.toml ./
COPY crates ./crates
COPY nilcc-simulator ./nilcc-simulator
COPY keeper ./keeper
COPY blacklight-node ./blacklight-node
COPY monitor ./monitor

RUN BLACKLIGHT_VERSION="${BLACKLIGHT_VERSION}" RUSTFLAGS="-Ctarget-feature=-crt-static" cargo build --release --bin blacklight-node && \
  mkdir -p /out/bin && \
  mv target/release/blacklight-node /out/bin/

FROM alpine

RUN apk add libgcc openssl
WORKDIR /app

COPY --from=builder /out/bin/blacklight-node /usr/local/bin/blacklight-node
ENTRYPOINT ["/usr/local/bin/blacklight-node"]
