FROM rust:1.91-alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev git pkgconf openssl-dev

# Copy project files
COPY Cargo.toml ./
COPY crates ./crates
COPY nilcc-simulator ./nilcc-simulator
COPY keeper ./keeper
COPY blacklight-node ./blacklight-node
COPY monitor ./monitor

RUN RUSTFLAGS="-Ctarget-feature=-crt-static" cargo build --release --bin keeper && \
  mkdir -p /out/bin && \
  mv target/release/keeper /out/bin/

FROM alpine

RUN apk add libgcc openssl
WORKDIR /app

COPY --from=builder /out/bin/keeper /usr/local/bin/keeper
ENTRYPOINT ["/usr/local/bin/keeper"]

