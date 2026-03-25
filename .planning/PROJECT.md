# local-s3

## Vision

A fast, lightweight, filesystem-backed S3-compatible server in Rust that replaces LocalStack's S3 for local development — no account signup required.

## North Star Metric

100% AWS SDK compatibility for core S3 operations, with sub-millisecond overhead on local requests.

## Target Users

- **Primary:** Developers running AWS-based applications locally who need S3 without LocalStack accounts
- **Secondary:** CI/CD pipelines needing ephemeral S3-compatible storage for integration tests

## Core Requirements

### Must Have (P0)

- [ ] Bucket operations: CreateBucket, DeleteBucket, ListBuckets, HeadBucket, GetBucketLocation
- [ ] Object CRUD: PutObject, GetObject, DeleteObject, HeadObject, CopyObject
- [ ] Batch delete: DeleteObjects (multi-object)
- [ ] List operations: ListObjectsV2 (prefix, delimiter, continuation-token, max-keys), ListObjects V1
- [ ] Presigned URL support (accept AWS SigV4 signatures without strict validation)
- [ ] Metadata preservation (x-amz-meta-*, Content-Type, Content-Disposition, Cache-Control, Content-Encoding)
- [ ] Proper ETag generation (MD5 for non-multipart, MD5-partcount for multipart)
- [ ] XML request/response format matching real S3
- [ ] S3 error response format (NoSuchKey, NoSuchBucket, BucketNotEmpty, etc.)
- [ ] Path-style URL routing (http://localhost:PORT/bucket/key)
- [ ] Filesystem-backed persistence with Docker volume support
- [ ] Docker image (small, fast startup)
- [ ] AWS SDK compatibility (tested with at minimum: aws-sdk-js-v3, boto3, aws-sdk-rust)

### Should Have (P1)

- [ ] Multipart upload: CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload, ListParts, ListMultipartUploads
- [ ] Object tagging: PutObjectTagging, GetObjectTagging, DeleteObjectTagging
- [ ] Range requests on GetObject (Range header)
- [ ] CORS configuration: PutBucketCors, GetBucketCors, DeleteBucketCors, OPTIONS preflight
- [ ] Bucket versioning: PutBucketVersioning, GetBucketVersioning, ListObjectVersions, get/delete by versionId, delete markers
- [ ] Bucket policy storage (store, no enforcement)
- [ ] Virtual-hosted-style URL support (http://bucket.s3.localhost:PORT/key)
- [ ] Conditional requests: If-None-Match, If-Modified-Since (304 responses)

### Nice to Have (P2)

- [ ] Bucket/object ACLs (store, no enforcement)
- [ ] Lifecycle configuration (store, no execution)
- [ ] Notification configuration (store only)
- [ ] Server-side encryption headers (accept, no actual encryption)
- [ ] Static website hosting configuration
- [ ] Bucket tagging
- [ ] UploadPartCopy

## Technical Stack

- **Language:** Rust (latest stable)
- **HTTP Framework:** axum (tokio-based, high performance)
- **Async Runtime:** tokio
- **Serialization:** quick-xml for S3 XML format
- **Hashing:** md5 crate for ETag generation
- **Storage:** Local filesystem (configurable root directory)
- **Containerization:** Docker (multi-stage build, distroless/scratch base)
- **Testing:** cargo test + AWS SDK integration tests

## Constraints

- Must be a drop-in replacement for LocalStack S3 — existing `AWS_ENDPOINT_URL` configs should just work
- Docker image must be under 20MB compressed
- Startup time under 100ms
- No external dependencies (no database, no runtime, just filesystem)
- Must handle concurrent requests safely
- All responses must use S3's XML format, not JSON
- Path-style URLs are the primary routing mode (virtual-hosted is P1)

## Success Criteria

- [ ] All P0 operations pass AWS SDK compatibility tests (JS, Python, Rust)
- [ ] Docker image builds and runs with `docker run -p 4566:4566 -v ./data:/data local-s3`
- [ ] Existing HealioSpace local dev setup works when swapping LocalStack S3 for local-s3
- [ ] Files persist across container restarts via volume mount
- [ ] Handles 100+ concurrent requests without errors

## Out of Scope

- IAM/policy enforcement (store policies, don't evaluate them)
- Real encryption at rest
- S3 Object Lock, Replication, Inventory, Analytics
- Glacier/storage class transitions
- Access Points
- CloudWatch metrics integration
- Any AWS service other than S3
