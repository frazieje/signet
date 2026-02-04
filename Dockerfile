# ---- build stage ----
FROM rust:1-bookworm AS builder

WORKDIR /app

# 1) Cache dependencies first (speeds up rebuilds)
COPY Cargo.toml Cargo.lock ./
# If you have a workspace, copy the workspace Cargo.toml(s) too.
# COPY crates/*/Cargo.toml crates/*/Cargo.toml

# Create a dummy src to let cargo resolve + build deps
RUN mkdir src && echo "fn main() {}" > src/server.rs
RUN cargo build --release
RUN rm -rf src

# 2) Copy actual source and build
COPY . .
RUN cargo build --release

# ---- runtime stage ----
FROM debian:bookworm-slim AS runtime

# If your gRPC server uses TLS at runtime or makes HTTPS calls, keep CA certs.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Create a non-root user (optional but recommended)
RUN useradd -m -u 10001 appuser

WORKDIR /app

# IMPORTANT: replace "my_grpc_server" with your actual crate/binary name
COPY --from=builder /app/target/release/signet /app/signet

USER appuser

EXPOSE 50051

# Ensure your server binds to 0.0.0.0:50051 (not 127.0.0.1)
ENV RUST_LOG=info
ENTRYPOINT ["/app/signet"]

