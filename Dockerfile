FROM rust:1.94-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Create a dummy main to cache dependency compilation
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true
# Copy real source and rebuild (touch to ensure Cargo detects the change)
COPY src/ src/
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/local-s3 /usr/local/bin/local-s3
EXPOSE 4566
VOLUME /data
ENTRYPOINT ["local-s3"]
CMD ["--port", "4566", "--data-dir", "/data"]
