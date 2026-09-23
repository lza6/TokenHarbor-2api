# TokenHarbor2API Rust 版 — 多阶段构建
FROM rust:1.95-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs && \
    cargo build --release 2>/dev/null || true

COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD curl -sf http://127.0.0.1:47830/healthz >/dev/null || exit 1
COPY --from=builder /build/target/release/tokenharbor2api /usr/local/bin/tokenharbor2api
ENV PORT=47830
VOLUME ["/data"]
EXPOSE 47830
ENTRYPOINT ["tokenharbor2api", "--config", "/data/config.json"]
