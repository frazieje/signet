# 🪶 Signet

**Rust implementation of an Envoy External Processor for RFC-9421 HTTP Message Signatures**

Signet provides a lightweight external gRPC processor that can be used with Envoy’s **External Processing** filter to generate HTTP Message Signatures according to **IETF RFC-9421**.  

This enables advanced HTTP integrity use cases in Envoy-based API gateways, service meshes, and edge proxies — including interoperable message signing using standardized headers and canonicalization semantics.

---

## 🚀 Overview

Envoy’s External Processing extension allows you to **intercept and modify HTTP requests and responses** using an external gRPC service. Signet implements that service in **Rust**, focusing on RFC-9421 compliant signatures. :contentReference[oaicite:0]{index=0}

### What It Does

- Accepts HTTP messages via Envoy’s gRPC ext_proc interface  
- Applies RFC-9421 signature generation or verification logic  
- Produces signed responses back to Envoy  
- Designed to be secure, extensible, and performant  

### Why Use Signet

RFC-9421 introduces an interoperable method for signing and verifying parts of HTTP messages — even when intermediaries or transformations are applied. This enables use cases like:

- End-to-end request authentication
- Request provenance verification
- Secure webhook delivery and validation  

The project is useful when you want **standardized HTTP message signatures** in your service mesh or API gateway stack. :contentReference[oaicite:1]{index=1}

---

## 📦 Features

- ✅ Rust-based implementation (safe, fast, and maintainable)  
- ✅ gRPC service compatible with Envoy External Proc filter  
- ⚙️ Easily containerized (e.g., Docker)  
- 🔒 Signature operations conformant with RFC-9421  

---

## Performance Targets

- Signing latency: P50 < 5ms, P99 < 20ms per response
- Sign success rate: >= 99.99% over 30 days

---

## 📦 Quick Start

### Build

```sh
git clone https://github.com/frazieje/signet.git
cd signet
cargo build --release
```

---

## Load Testing

A built-in load test tool (`signet-loadtest`) sends synthetic responses through Signet and measures signing latency and throughput. For reproducible results, run Signet in a Docker container with constrained CPU and memory.

### Setup

```bash
# Generate test key (PKCS#8 format)
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out test-key.pem
chmod 644 test-key.pem  # must be readable by container's non-root user

# Run Signet with constrained resources
docker build -t signet .
docker run --rm -d --name signet-bench \
  --cpus=1 --memory=256m \
  -v $(pwd)/test-key.pem:/app/key.pem:ro \
  -p 50051:50051 signet
```

### Modes

**Single run** — fixed concurrency and body size range:
```bash
cargo run --release -p signet-loadtest -- \
  -c 20 -n 5000 --min-body 128 --max-body 131072
```

**Concurrency sweep** (`--sweep <threshold_ms>`) — doubles concurrency from 1 until P50 exceeds the threshold:
```bash
cargo run --release -p signet-loadtest -- -n 500 --sweep 5
```

**Body size sweep** (`--sweep-body`) — tests fixed body sizes from 256B to 5MB to measure how payload size affects latency:
```bash
cargo run --release -p signet-loadtest -- -n 500 -c 10 --sweep-body
```

### Cleanup

```bash
docker stop signet-bench
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--addr` | `http://localhost:50051` | Signet gRPC address |
| `-c`, `--concurrency` | `10` | Concurrent streams |
| `-n`, `--requests` | `1000` | Total requests (per band in sweep modes) |
| `--min-body` | `64` | Min body size in bytes |
| `--max-body` | `65536` | Max body size in bytes |
| `--sweep <ms>` | — | Sweep concurrency until P50 >= threshold |
| `--sweep-body` | — | Sweep body size bands (256B to 5MB) |

Adjust `--cpus` and `--memory` on the Docker container to control the resource envelope (e.g., `--cpus=0.5` for half a core, `--memory=128m` for tighter memory).
