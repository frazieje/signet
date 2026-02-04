# ---- build stage ----
FROM rust:1-bookworm AS builder

WORKDIR /app

# 1) Cache dependencies first (speeds up rebuilds)
COPY Cargo.toml Cargo.lock ./

# Create a dummy src to let cargo resolve + build deps
RUN mkdir src && echo "fn main() {}" > src/server.rs
RUN cargo build --release
RUN rm -rf src

# 2) Copy actual source and build
COPY . .
RUN cargo build --release

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

