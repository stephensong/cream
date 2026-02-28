# Stage 1: Build WASM contracts + guardian binary
FROM rust:1.86-bookworm AS builder

# Install wasm target for contract compilation
RUN rustup target add wasm32-unknown-unknown

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY common/ common/
COPY contracts/ contracts/
COPY delegates/ delegates/
COPY guardian/ guardian/
COPY tests/ tests/

# Build WASM contracts first (guardian embeds them via include_bytes!)
RUN cargo build -p cream-directory-contract \
        --target wasm32-unknown-unknown --release --features contract && \
    cargo build -p cream-storefront-contract \
        --target wasm32-unknown-unknown --release --features contract && \
    cargo build -p cream-user-contract \
        --target wasm32-unknown-unknown --release --features contract

# Build guardian binary (release profile: LTO, stripped, size-optimized)
RUN cargo build -p cream-guardian --release

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cream-guardian /usr/local/bin/cream-guardian
COPY cream-startos/docker_entrypoint.sh /usr/local/bin/docker_entrypoint.sh
COPY cream-startos/health_check.sh /usr/local/bin/health_check.sh

RUN chmod +x /usr/local/bin/docker_entrypoint.sh /usr/local/bin/health_check.sh

VOLUME /data

ENV HOME=/data

ENTRYPOINT ["docker_entrypoint.sh"]
