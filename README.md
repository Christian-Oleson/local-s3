# local-s3

A fast, lightweight S3-compatible server for local development. No account required.

[![CI](https://github.com/christian-oleson/local-s3/actions/workflows/ci.yml/badge.svg)](https://github.com/christian-oleson/local-s3/actions/workflows/ci.yml)

## Why?

LocalStack now requires account signup. local-s3 is a zero-dependency, drop-in replacement
for LocalStack's S3 that runs locally with no account, no signup, no telemetry.

- **Rust-native**: built on axum and tokio — sub-millisecond overhead on local requests
- **Filesystem-backed**: buckets are directories, objects are files — easy to inspect and debug
- **Docker volume persistence**: data survives container restarts
- **Drop-in compatible**: set `AWS_ENDPOINT_URL=http://localhost:4566` and existing code just works
- **Small image**: multi-stage build targeting scratch, under 20 MB compressed

## Quickstart

### Docker (recommended)

```bash
docker run -p 4566:4566 -v ./data:/data ghcr.io/christian-oleson/local-s3
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

### From Source

```bash
cargo install --path .
local-s3 --port 4566 --data-dir ./data
```

## SDK Configuration

### AWS CLI

```bash
aws --endpoint-url http://localhost:4566 s3 mb s3://my-bucket
aws --endpoint-url http://localhost:4566 s3 cp file.txt s3://my-bucket/
aws --endpoint-url http://localhost:4566 s3 ls s3://my-bucket/
```

Or set `AWS_ENDPOINT_URL=http://localhost:4566` in your environment to avoid passing `--endpoint-url` to every command.

### Node.js (aws-sdk v3)

```javascript
import { S3Client } from "@aws-sdk/client-s3";

const s3 = new S3Client({
  endpoint: "http://localhost:4566",
  region: "us-east-1",
  forcePathStyle: true,
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});
```

### Python (boto3)

```python
import boto3

s3 = boto3.client(
    "s3",
    endpoint_url="http://localhost:4566",
    region_name="us-east-1",
    aws_access_key_id="test",
    aws_secret_access_key="test",
)
```

### Rust (aws-sdk-s3)

```rust
use aws_sdk_s3::config::{Credentials, Region};

let config = aws_sdk_s3::Config::builder()
    .endpoint_url("http://localhost:4566")
    .region(Region::new("us-east-1"))
    .credentials_provider(Credentials::new("test", "test", None, None, "test"))
    .force_path_style(true)
    .build();

let client = aws_sdk_s3::Client::from_conf(config);
```

### .NET (AWSSDK.S3)

```csharp
var config = new AmazonS3Config
{
    ServiceURL = "http://localhost:4566",
    ForcePathStyle = true,
};
var client = new AmazonS3Client("test", "test", config);
```

## Supported Operations

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
| Advanced | Range requests (206 Partial Content), Conditional requests (304 Not Modified), Presigned URL acceptance |

All responses use S3's XML format. Error responses follow S3's `<Error><Code>...</Code><Message>...</Message></Error>` structure with correct HTTP status codes.

### Notes on compatibility

- **SigV4 passthrough**: signatures are accepted without cryptographic validation — this is intentional for local development
- **ACL and policy storage**: configs are accepted and stored but not enforced
- **Lifecycle rules**: stored but not executed
- Any credential values are accepted (use `"test"` / `"test"` for simplicity)

## Secrets Manager

Both S3 and Secrets Manager run on the same port (4566). Set `AWS_ENDPOINT_URL=http://localhost:4566` once and both services work without any other configuration change.

### Secrets Manager SDK Configuration

#### AWS CLI

```bash
aws --endpoint-url http://localhost:4566 secretsmanager create-secret --name my-secret --secret-string '{"password":"abc123"}'
aws --endpoint-url http://localhost:4566 secretsmanager get-secret-value --secret-id my-secret
```

#### Python (boto3)

```python
client = boto3.client('secretsmanager', endpoint_url='http://localhost:4566')
```

#### Node.js (aws-sdk v3)

```javascript
import { SecretsManagerClient } from "@aws-sdk/client-secrets-manager";
const client = new SecretsManagerClient({ endpoint: "http://localhost:4566" });
```

### Supported Secrets Manager Operations

| Category | Operations |
|----------|-----------|
| Core CRUD | CreateSecret, GetSecretValue, PutSecretValue, UpdateSecret, DeleteSecret, RestoreSecret |
| Discovery | DescribeSecret, ListSecrets, ListSecretVersionIds |
| Versioning | UpdateSecretVersionStage (AWSCURRENT/AWSPREVIOUS/custom labels) |
| Tags | TagResource, UntagResource |
| Policies | PutResourcePolicy, GetResourcePolicy, DeleteResourcePolicy, ValidateResourcePolicy |
| Rotation | RotateSecret, CancelRotateSecret (config storage, no Lambda) |
| Batch | BatchGetSecretValue |

**Protocol note:** Secrets Manager uses AWS JSON 1.1 protocol (POST / with X-Amz-Target header, JSON bodies) — different from S3's REST+XML. The server routes requests automatically: any request with an `X-Amz-Target` header beginning with `secretsmanager.` is dispatched to the Secrets Manager handler; all other requests go to S3.

## Configuration

| Option | Env Var | CLI Flag | Default |
|--------|---------|----------|---------|
| Port | `LOCAL_S3_PORT` | `--port` | `4566` |
| Data directory | `LOCAL_S3_DATA_DIR` | `--data-dir` | `./data` |
| Log level | `RUST_LOG` | — | `info` |

CLI flags take precedence over environment variables.

Example using environment variables:

```bash
LOCAL_S3_PORT=9000 LOCAL_S3_DATA_DIR=/tmp/s3 local-s3
```

Example using CLI flags:

```bash
local-s3 --port 9000 --data-dir /tmp/s3
```

## Health Check

`GET /` returns the list-buckets response (HTTP 200). Use this as your container or load balancer health check endpoint.

```bash
curl -s http://localhost:4566/ | head -5
```

## Persistence

Data is stored on the filesystem at the configured data directory:

```
data/
  my-bucket/
    photo.jpg
    photo.jpg.meta.json       # content-type, custom headers, etag
    documents/report.pdf
    documents/report.pdf.meta.json
  .secrets-manager/
    secrets/
      my-secret/
        metadata.json         # name, ARN, description, tags, rotation config
        policy.json           # resource policy
        versions/
          {version-id}.json   # secret value + staging labels
```

Each bucket is a directory. Object data is stored as a plain file at the key path. Metadata (Content-Type, ETag, `x-amz-meta-*` headers, etc.) is stored in a sidecar `.meta.json` file alongside each object.

Multipart upload state is kept under a `.uploads/` directory inside each bucket and cleaned up automatically on complete or abort.

Versioned objects maintain a version chain directory per key. Delete markers are stored as zero-byte version entries.

Secrets are stored under `.secrets-manager/secrets/` with one subdirectory per secret name. Each version is a separate JSON file under `versions/`.

Use a Docker volume to persist data across restarts:

```bash
docker run -p 4566:4566 -v /path/on/host:/data ghcr.io/christian-oleson/local-s3
```

## Development

### Prerequisites

- Rust stable toolchain (`rustup install stable`)

### Commands

```bash
cargo build                          # Debug build
cargo build --release                # Optimized release build
cargo test                           # All tests (unit + integration)
cargo test --lib                     # Unit tests only
cargo test --test integration        # Integration tests only
cargo run -- --port 4566 --data-dir ./data  # Run locally
cargo fmt                            # Format code
cargo fmt -- --check                 # Verify formatting (CI)
cargo clippy -- -D warnings          # Lint
```

### Running with Docker

```bash
docker build -t local-s3 .
docker run -p 4566:4566 -v ./data:/data local-s3
```

### Project Structure

```
src/
  main.rs          # Entry point: arg/env parsing, logging setup
  server.rs        # axum router, AppState, service multiplexer, CORS preflight handler
  services/
    s3/            # S3 request handlers and storage engine
    secretsmanager/
      dispatcher.rs  # X-Amz-Target routing
      handlers.rs    # Operation handlers
      storage.rs     # Filesystem-backed secret storage
      types.rs       # Request/response JSON types
      error.rs       # Secrets Manager error types
  middleware.rs    # Response header injection (x-amz-request-id, etc.)
tests/
  integration/     # AWS SDK integration tests (aws-sdk-rust)
```

### Architecture Notes

- **Path-style routing** is the primary mode: `http://localhost:4566/{bucket}/{key}`
- **Virtual-hosted-style** (`http://{bucket}.s3.localhost:4566/{key}`) is supported as a secondary mode
- **ETags** are quoted MD5 hex for single-part uploads (`"abc123"`) and quoted `MD5-partcount` for multipart (`"abc123-3"`)
- `DeleteObject` returns 204 even for non-existent keys (S3 behavior)
- `ListObjectsV2` with a delimiter returns `CommonPrefixes` entries for simulated folder navigation
- **Service multiplexing**: the `X-Amz-Target` header determines whether a request goes to S3 or Secrets Manager — no port split required

## License

MIT
