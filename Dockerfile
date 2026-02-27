# ---- build stage ----
FROM rust:1-bookworm AS builder

WORKDIR /app

# 1) Cache dependencies first (speeds up rebuilds)
COPY Cargo.toml Cargo.lock ./
COPY tools/loadtest/Cargo.toml tools/loadtest/Cargo.toml

# Create dummy sources to let cargo resolve + build deps
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && mkdir -p tools/loadtest/src && echo "fn main() {}" > tools/loadtest/src/main.rs
RUN cargo build --release
RUN rm -rf src tools/loadtest/src

# 2) Copy actual source and build
COPY . .
RUN touch src/lib.rs src/main.rs && cargo build --release -p signet

# ---- runtime stage ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -u 10001 appuser

WORKDIR /app

COPY --from=builder /app/target/release/signet /app/signet

USER appuser

EXPOSE 50051

ENV RUST_LOG=info
ENTRYPOINT ["/app/signet"]
