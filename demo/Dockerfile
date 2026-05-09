FROM rust:1-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM ghcr.io/charmbracelet/vhs

RUN apt-get update && apt-get install -y --no-install-recommends \
    tmux nodejs git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/roostr /usr/local/bin/roostr
COPY demo/demo.sh /demo/demo.sh
COPY demo/tapes /demo/tapes
COPY tmux.conf /root/.tmux.conf

RUN chmod +x /usr/local/bin/roostr /demo/demo.sh

WORKDIR /demo
