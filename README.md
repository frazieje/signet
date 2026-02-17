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

## 📦 Quick Start

### Build

```sh
git clone https://github.com/frazieje/signet.git
cd signet
cargo build --release
