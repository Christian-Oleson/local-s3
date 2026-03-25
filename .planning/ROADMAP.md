# Roadmap

## Overview

Total Phases: 5
Estimated Complexity: High

---

## Phase 1: Foundation

**Goal:** Establish Rust project structure, HTTP server, filesystem storage layer, and basic bucket operations
**Deliverable:** A running server that can create/delete/list buckets and return proper S3 XML responses
**Complexity:** Medium

### Tasks

- [ ] 1.1: Initialize Rust project with Cargo workspace, dependencies (axum, tokio, quick-xml, md5, serde, chrono, uuid)
- [ ] 1.2: Implement filesystem storage engine (bucket directories, object files, metadata sidecar files)
- [ ] 1.3: Implement S3 XML response serialization (ListAllMyBucketsResult, Error responses, common types)
- [ ] 1.4: Implement S3 XML request deserialization (CreateBucketConfiguration, Delete request body)
- [ ] 1.5: Implement bucket operations: CreateBucket, DeleteBucket, ListBuckets, HeadBucket, GetBucketLocation
- [ ] 1.6: Implement S3 error handling (NoSuchBucket, BucketAlreadyExists, BucketNotEmpty — proper XML + HTTP status)
- [ ] 1.7: Implement path-style URL router (parse bucket/key from URL path)
- [ ] 1.8: Add request ID generation and common S3 response headers (x-amz-request-id, x-amz-id-2, Server)
- [ ] 1.9: Write unit tests for storage engine and XML serialization
- [ ] 1.10: Write integration tests using aws-sdk-rust for bucket operations

### Dependencies

- None (first phase)

---

## Phase 2: Core Object Operations

**Goal:** Full object CRUD with metadata, ETags, listing, and batch operations
**Deliverable:** Developers can PUT, GET, DELETE, COPY, and LIST objects — enough to run most S3-dependent apps
**Complexity:** High

### Tasks

- [ ] 2.1: Implement PutObject (body streaming, metadata headers, Content-Type, ETag response)
- [ ] 2.2: Implement GetObject (body streaming, metadata headers, Content-Type, ETag, Content-Length)
- [ ] 2.3: Implement HeadObject (same headers as GET, no body)
- [ ] 2.4: Implement DeleteObject (204 response, idempotent for missing keys)
- [ ] 2.5: Implement CopyObject (same-bucket and cross-bucket, x-amz-copy-source header, metadata directive)
- [ ] 2.6: Implement DeleteObjects (batch delete via POST ?delete, XML request/response)
- [ ] 2.7: Implement ListObjectsV2 (prefix, delimiter, max-keys, continuation-token, CommonPrefixes)
- [ ] 2.8: Implement ListObjects V1 (prefix, delimiter, max-keys, marker)
- [ ] 2.9: Implement metadata storage/retrieval (x-amz-meta-*, Content-Disposition, Cache-Control, Content-Encoding, Expires)
- [ ] 2.10: Implement proper ETag generation (MD5 of content, quoted)
- [ ] 2.11: Implement presigned URL acceptance (pass-through SigV4 — accept any signature for local dev)
- [ ] 2.12: Write integration tests using aws-sdk-rust for all object operations
- [ ] 2.13: Write integration tests using aws-sdk-js-v3 (Node.js test suite)

### Dependencies

- Phase 1 complete

---

## Phase 3: Multipart Upload & Advanced Features

**Goal:** Support large file uploads and commonly-used S3 features (tagging, range requests, CORS, conditional requests)
**Deliverable:** Full multipart upload lifecycle, object tagging, range reads, and CORS — covers P1 requirements
**Complexity:** High

### Tasks

- [ ] 3.1: Implement CreateMultipartUpload (return UploadId, store upload state)
- [ ] 3.2: Implement UploadPart (store parts on filesystem, return ETag per part)
- [ ] 3.3: Implement CompleteMultipartUpload (assemble parts, generate composite ETag, XML response)
- [ ] 3.4: Implement AbortMultipartUpload (clean up parts)
- [ ] 3.5: Implement ListParts and ListMultipartUploads
- [ ] 3.6: Implement Range requests on GetObject (single range, multipart range)
- [ ] 3.7: Implement object tagging: PutObjectTagging, GetObjectTagging, DeleteObjectTagging
- [ ] 3.8: Implement CORS: PutBucketCors, GetBucketCors, DeleteBucketCors, OPTIONS preflight handling
- [ ] 3.9: Implement conditional requests: If-None-Match, If-Modified-Since → 304 Not Modified
- [ ] 3.10: Write integration tests for multipart upload lifecycle
- [ ] 3.11: Write integration tests for tagging, range requests, CORS, conditional requests

### Dependencies

- Phase 2 complete

---

## Phase 4: Versioning & Configuration Storage

**Goal:** Bucket versioning support and storage of bucket configurations (policies, ACLs, lifecycle) without enforcement
**Deliverable:** Apps that use versioning work correctly; bucket configs are accepted and stored
**Complexity:** Medium

### Tasks

- [ ] 4.1: Implement PutBucketVersioning / GetBucketVersioning (Enabled/Suspended states)
- [ ] 4.2: Implement versioned object storage (version IDs, version chain per key)
- [ ] 4.3: Implement ListObjectVersions
- [ ] 4.4: Implement GetObject / HeadObject / DeleteObject with ?versionId
- [ ] 4.5: Implement delete markers for versioned buckets
- [ ] 4.6: Implement virtual-hosted-style URL routing (bucket.s3.localhost)
- [ ] 4.7: Implement bucket policy storage: PutBucketPolicy, GetBucketPolicy, DeleteBucketPolicy
- [ ] 4.8: Implement bucket/object ACL storage: Put/Get BucketAcl, Put/Get ObjectAcl
- [ ] 4.9: Implement lifecycle configuration storage: Put/Get/Delete BucketLifecycleConfiguration
- [ ] 4.10: Write integration tests for versioning operations
- [ ] 4.11: Write integration tests for config storage operations

### Dependencies

- Phase 3 complete

---

## Phase 5: Docker, CI, Polish

**Goal:** Production-quality Docker image, CI pipeline, documentation, and final AWS SDK compatibility validation
**Deliverable:** `docker run` one-liner that replaces LocalStack S3 for any developer
**Complexity:** Medium

### Tasks

- [ ] 5.1: Create multi-stage Dockerfile (rust builder → scratch/distroless runtime)
- [ ] 5.2: Optimize binary size (strip, LTO, panic=abort, musl target for static linking)
- [ ] 5.3: Add docker-compose.yml example with volume mount
- [ ] 5.4: Add CLI argument parsing (port, data directory, log level)
- [ ] 5.5: Add structured logging (tracing crate)
- [ ] 5.6: Add health check endpoint (GET /)
- [ ] 5.7: Run full AWS SDK compatibility test suite (JS, Python, Rust)
- [ ] 5.8: Add GitHub Actions CI (build, test, Docker build, SDK compat tests)
- [ ] 5.9: Write README with quickstart, docker-compose example, and SDK configuration snippets
- [ ] 5.10: Performance benchmarking (concurrent PutObject/GetObject throughput)

### Dependencies

- Phase 4 complete

---

## Milestones

| Milestone | Phase | Description |
|-----------|-------|-------------|
| Server Boots | 1 | HTTP server runs, bucket CRUD works |
| MVP | 2 | Object CRUD + listing — enough for most apps |
| Feature Complete | 3 | Multipart, tagging, CORS, ranges — covers all common use |
| Full Compat | 4 | Versioning + config storage — parity with LocalStack free |
| Ship It | 5 | Docker image, CI, docs — ready for team adoption |
