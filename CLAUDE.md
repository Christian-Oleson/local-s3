# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

`local-s3` is a fast, lightweight S3-compatible server written in Rust. It replaces LocalStack's S3 for local development — no account signup required. Filesystem-backed with Docker volume persistence.

## Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release build
cargo test                           # Run all tests
cargo test --lib                     # Unit tests only
cargo test --test integration        # Integration tests only
cargo run -- --port 4566 --data-dir ./data  # Run locally
cargo fmt                            # Format code
cargo fmt -- --check                 # Check formatting
cargo clippy -- -D warnings          # Lint (treat warnings as errors)
```

### Docker

```bash
docker build -t local-s3 .
docker run -p 4566:4566 -v ./data:/data local-s3

# With environment variable overrides
docker run -p 9000:9000 -e LOCAL_S3_PORT=9000 -v ./data:/data local-s3
```

### Docker Compose

```yaml
services:
  local-s3:
    image: ghcr.io/christian-oleson/local-s3
    ports:
      - "4566:4566"
    volumes:
      - ./data:/data
```

## Architecture

### Key Design Decisions

- **axum** for HTTP (tokio-based, zero-cost routing)
- **quick-xml** for S3 XML serialization/deserialization (S3 uses XML, not JSON)
- **Filesystem storage**: buckets are directories, objects are files, metadata in sidecar `.meta.json` files
- **Path-style URLs** as primary routing: `http://localhost:4566/bucket/key`
- **Virtual-hosted-style** as secondary: `http://bucket.s3.localhost:4566/key`
- **SigV4 passthrough**: accept any AWS signature without validation (local dev only)
- **Port 4566**: same as LocalStack default for drop-in replacement

### S3 Behavioral Requirements

- ETags: quoted MD5 hex for single uploads (`"abc123"`), quoted MD5-partcount for multipart (`"abc123-3"`)
- DeleteObject returns 204 even for non-existent keys
- ListObjectsV2 with delimiter must return CommonPrefixes for "folder" simulation
- All error responses use S3 XML format: `<Error><Code>NoSuchKey</Code><Message>...</Message></Error>`
- Response headers must include: x-amz-request-id, x-amz-id-2, Server

### Health Check

`GET /` is the health check endpoint — it returns the ListBuckets XML response with HTTP 200. Safe to use as a Docker/load balancer health probe.

### Supported S3 Operations

| Category | Operations |
|----------|-----------|
| Buckets | CreateBucket, DeleteBucket, ListBuckets, HeadBucket, GetBucketLocation |
| Objects | PutObject, GetObject, HeadObject, DeleteObject, CopyObject, DeleteObjects |
| Listing | ListObjectsV2, ListObjects (V1), ListObjectVersions |
| Multipart | CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload, ListParts, ListMultipartUploads |
| Versioning | PutBucketVersioning, GetBucketVersioning (Enabled/Suspended, full version chain and delete markers) |
| Tagging | PutObjectTagging, GetObjectTagging, DeleteObjectTagging |
| CORS | PutBucketCors, GetBucketCors, DeleteBucketCors, OPTIONS preflight |
| Config | PutBucketPolicy, GetBucketPolicy, DeleteBucketPolicy, Put/GetBucketAcl, Put/GetObjectAcl, Lifecycle configuration |
| Advanced | Range requests (206), Conditional requests (304), Presigned URL acceptance |

## Configuration

| Option | Env Var | CLI Flag | Default |
|--------|---------|----------|---------|
| Port | `LOCAL_S3_PORT` | `--port` | `4566` |
| Data directory | `LOCAL_S3_DATA_DIR` | `--data-dir` | `./data` |
| Log level | `RUST_LOG` | — | `info` |

CLI flags take precedence over environment variables.

## Conventions

- Rust 2024 edition, latest stable toolchain
- `cargo fmt` + `cargo clippy -- -D warnings` must pass before commit
- All public APIs have integration tests using AWS SDK clients
- Error types use thiserror, map to proper S3 error codes and HTTP status
- Async everywhere — no blocking I/O on the tokio runtime

## Project Planning

Planning documents live in `.planning/`:
- `PROJECT.md` — requirements and scope
- `ROADMAP.md` — phased delivery plan
- `STATE.md` — current progress and session tracking
