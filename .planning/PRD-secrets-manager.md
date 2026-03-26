# PRD: Local Secrets Manager Service

## Executive Summary

Expand the local-s3 project into a multi-service local AWS replacement by adding a Secrets Manager implementation. This will be the second service in what becomes the **local-aws** platform — a suite of lightweight, filesystem-backed AWS service emulators for local development, requiring no account signup.

## Problem Statement

Developers using AWS Secrets Manager in their applications face the same LocalStack account requirement that prompted local-s3. Applications that read secrets at startup (database credentials, API keys, service endpoints) cannot run locally without either:

1. A LocalStack account (paid or signup-gated)
2. Real AWS credentials (security risk, costs money, requires connectivity)
3. Environment variable workarounds (diverges from production code paths)

A local Secrets Manager emulator eliminates this friction while keeping application code identical to production.

## Vision

A drop-in Secrets Manager replacement running alongside local-s3, sharing the same Docker image, same port (4566), and same filesystem persistence model. Developers configure `AWS_ENDPOINT_URL=http://localhost:4566` once and both S3 and Secrets Manager work.

## Target Users

- **Primary:** Developers running AWS-based applications locally who store secrets in Secrets Manager
- **Secondary:** CI/CD pipelines needing deterministic secret values for integration tests
- **Tertiary:** Developers writing IaC (CDK/Terraform) that provisions secrets

## North Star Metric

Any application using `aws-sdk-*` Secrets Manager client works against this service with zero code changes — only an endpoint URL override.

---

## Technical Design

### Protocol: AWS JSON 1.1

Unlike S3 (which uses REST-style paths + XML), Secrets Manager uses the **AWS JSON 1.1 protocol**:

- **All requests**: `POST /` with `Content-Type: application/x-amz-json-1.1`
- **Action dispatch**: `X-Amz-Target: secretsmanager.<OperationName>` header
- **Request/response bodies**: JSON (not XML)
- **Errors**: JSON with `__type` and `Message` fields, HTTP 400 for client errors

This is a fundamentally different protocol from S3 and requires a new request dispatcher.

### Service Multiplexing on Port 4566

The AWS SDK determines the service from the `X-Amz-Target` header (for JSON protocol services) or the URL path/hostname (for REST services). To serve both S3 and Secrets Manager on port 4566:

**Routing strategy:**
1. If `X-Amz-Target` header starts with `secretsmanager.` → route to Secrets Manager handler
2. Else → route to S3 handler (existing behavior)

This is clean because S3 never uses `X-Amz-Target` — the two protocols don't overlap.

### Storage Model

Secrets stored in the filesystem alongside S3 data:

```
{data-dir}/
├── .secrets-manager/           # Secrets Manager data root
│   ├── secrets/
│   │   ├── {secret-name}/      # One directory per secret (URL-encoded if contains /)
│   │   │   ├── metadata.json   # Name, ARN, Description, KmsKeyId, Tags, Rotation config
│   │   │   ├── policy.json     # Resource policy (raw JSON string)
│   │   │   └── versions/
│   │   │       ├── {version-id}.json  # { SecretString/SecretBinary, VersionStages, CreatedDate }
│   │   │       └── ...
│   │   └── ...
│   └── state.json              # Account ID, region, ARN suffix cache
├── bucket-a/                   # S3 buckets (existing)
├── bucket-b/
└── ...
```

### ARN Generation

```
arn:aws:secretsmanager:{region}:{account-id}:secret:{name}-{6-random-chars}
```

- **Region**: from config (default `us-east-1`)
- **Account ID**: hardcoded `000000000000` (matching LocalStack convention)
- **Random suffix**: 6 alphanumeric chars, generated once at creation, stored in metadata

### Secret Versioning Model

Each secret has a map of `version_id → VersionData`:

```json
{
  "version_id": "uuid",
  "secret_string": "...",       // or secret_binary (base64)
  "version_stages": ["AWSCURRENT"],
  "created_date": 1234567890.123
}
```

**Staging label rules:**
- `AWSCURRENT`: exactly one version per secret has this label
- `AWSPREVIOUS`: at most one version has this (the previous AWSCURRENT)
- `AWSPENDING`: used during rotation
- Custom labels: any string, no constraints
- Versions with no labels are "deprecated" (retained but not actively used)

**Version lifecycle on PutSecretValue:**
1. New version V(n) created with `AWSCURRENT`
2. Old V(n-1) loses `AWSCURRENT`, gains `AWSPREVIOUS`
3. Old V(n-2) loses `AWSPREVIOUS` (becomes deprecated, no labels)

---

## Core Requirements

### Must Have (P0)

| # | Operation | Priority | Notes |
|---|-----------|----------|-------|
| 1 | CreateSecret | P0 | String + binary, tags, description, idempotency token |
| 2 | GetSecretValue | P0 | By name/ARN, by version_id, by version_stage |
| 3 | PutSecretValue | P0 | New version, AWSCURRENT/AWSPREVIOUS label rotation |
| 4 | DeleteSecret | P0 | ForceDeleteWithoutRecovery + recovery window |
| 5 | DescribeSecret | P0 | Full metadata including VersionIdsToStages map |
| 6 | ListSecrets | P0 | Pagination, filtering by name/tag/description |
| 7 | UpdateSecret | P0 | Metadata + optional value update |
| 8 | RestoreSecret | P0 | Cancel scheduled deletion |
| 9 | ListSecretVersionIds | P0 | Version enumeration with staging labels |
| 10 | JSON protocol dispatcher | P0 | X-Amz-Target routing, JSON error format |
| 11 | Service multiplexing | P0 | Secrets Manager + S3 on same port |

### Should Have (P1)

| # | Operation | Priority | Notes |
|---|-----------|----------|-------|
| 12 | UpdateSecretVersionStage | P1 | Move staging labels between versions |
| 13 | TagResource | P1 | Add/update tags |
| 14 | UntagResource | P1 | Remove tags by key |
| 15 | GetResourcePolicy | P1 | Return stored policy (no enforcement) |
| 16 | PutResourcePolicy | P1 | Store policy JSON |
| 17 | DeleteResourcePolicy | P1 | Remove stored policy |
| 18 | RotateSecret | P1 | Store rotation config (no Lambda execution) |
| 19 | CancelRotateSecret | P1 | Clear rotation state |

### Nice to Have (P2)

| # | Operation | Priority | Notes |
|---|-----------|----------|-------|
| 20 | BatchGetSecretValue | P2 | Bulk retrieval |
| 21 | ValidateResourcePolicy | P2 | Basic JSON validation |
| 22 | ReplicateSecretToRegions | P2 | Store metadata only |
| 23 | RemoveRegionsFromReplication | P2 | Metadata only |
| 24 | StopReplicationToReplica | P2 | Metadata only |

---

## Implementation Phases

### Phase 1: Foundation + Core CRUD (8 tasks)

**Goal:** CreateSecret, GetSecretValue, PutSecretValue, DeleteSecret, ListSecrets with proper JSON protocol dispatch.

**Tasks:**
1. JSON protocol dispatcher — route `X-Amz-Target` to handlers, JSON error format
2. Service multiplexer — detect Secrets Manager vs S3 requests on same port
3. Secrets storage engine — filesystem-backed secret + version storage
4. CreateSecret handler — full spec with ARN generation, versioning, tags
5. GetSecretValue handler — by name, ARN, version_id, version_stage
6. PutSecretValue handler — new version with AWSCURRENT/AWSPREVIOUS rotation
7. DeleteSecret + RestoreSecret — recovery window + force delete
8. Integration tests with aws-sdk-rust secretsmanager client

**Deliverable:** Developers can create, read, update, delete secrets via any AWS SDK.

### Phase 2: Metadata + Discovery (5 tasks)

**Goal:** DescribeSecret, ListSecrets with filtering/pagination, UpdateSecret, ListSecretVersionIds.

**Tasks:**
1. DescribeSecret handler — full metadata response
2. ListSecrets handler — pagination, MaxResults, Filters (name, tag-key, tag-value, description)
3. UpdateSecret handler — metadata + optional value
4. ListSecretVersionIds handler — version enumeration, IncludeDeprecated
5. Integration tests for all discovery operations

**Deliverable:** Full secret lifecycle management and discovery.

### Phase 3: Versioning, Tags, Policies (5 tasks)

**Goal:** Advanced version management, tagging, resource policies, rotation config.

**Tasks:**
1. UpdateSecretVersionStage — move staging labels
2. TagResource + UntagResource
3. GetResourcePolicy + PutResourcePolicy + DeleteResourcePolicy (store-only)
4. RotateSecret + CancelRotateSecret (store config, no Lambda execution)
5. Integration tests

**Deliverable:** Full LocalStack Community parity for Secrets Manager.

### Phase 4: Polish + Batch (3 tasks)

**Goal:** BatchGetSecretValue, ValidateResourcePolicy, documentation.

**Tasks:**
1. BatchGetSecretValue — bulk retrieval with partial failure handling
2. ValidateResourcePolicy — basic JSON structure validation
3. README update, SDK configuration examples, Secrets Manager section in docs

**Deliverable:** Complete Secrets Manager emulator ready for team adoption.

---

## Error Handling

All errors follow the AWS JSON error format:

```json
{
  "__type": "ResourceNotFoundException",
  "Message": "Secrets Manager can't find the specified secret."
}
```

| Error | HTTP | When |
|-------|------|------|
| ResourceNotFoundException | 400 | Secret not found |
| ResourceExistsException | 400 | Duplicate name on create |
| InvalidParameterException | 400 | Bad param value |
| InvalidRequestException | 400 | Invalid for current state (e.g., get deleted secret) |
| MalformedPolicyDocumentException | 400 | Invalid policy JSON |
| LimitExceededException | 400 | Too many secrets (optional to enforce) |
| InternalServiceError | 500 | Internal error |

**Important quirk:** AWS Secrets Manager returns HTTP **400** for `ResourceNotFoundException`, not 404. Our implementation must match this for SDK compatibility.

---

## SDK Compatibility Requirements

The implementation must work with these SDK clients out of the box:

```python
# Python (boto3)
client = boto3.client('secretsmanager', endpoint_url='http://localhost:4566')
client.create_secret(Name='my-secret', SecretString='{"password":"abc123"}')
value = client.get_secret_value(SecretId='my-secret')
```

```javascript
// Node.js (aws-sdk-v3)
import { SecretsManagerClient, GetSecretValueCommand } from "@aws-sdk/client-secrets-manager";
const client = new SecretsManagerClient({ endpoint: "http://localhost:4566" });
await client.send(new GetSecretValueCommand({ SecretId: "my-secret" }));
```

```rust
// Rust (aws-sdk-secretsmanager)
let config = aws_sdk_secretsmanager::Config::builder()
    .endpoint_url("http://localhost:4566")
    .build();
let client = aws_sdk_secretsmanager::Client::from_conf(config);
client.get_secret_value().secret_id("my-secret").send().await?;
```

```csharp
// .NET (AWSSDK.SecretsManager)
var config = new AmazonSecretsManagerConfig { ServiceURL = "http://localhost:4566" };
var client = new AmazonSecretsManagerClient("test", "test", config);
var response = await client.GetSecretValueAsync(new GetSecretValueRequest { SecretId = "my-secret" });
```

---

## Architectural Considerations

### Project Rename

With multiple services, the project should be renamed from `local-s3` to `local-aws` or remain `local-s3` with Secrets Manager as an additional module. Options:

1. **Rename to `local-aws`** — cleaner, supports future services (SQS, SNS, DynamoDB)
2. **Keep `local-s3` and add `local-secrets`** — separate binaries, harder to deploy
3. **Keep `local-s3` with built-in Secrets Manager** — simplest, single binary

**Recommendation:** Option 3 for now (single binary, single port). Rename when a third service is added.

### Code Organization

```
src/
├── main.rs                    # Entry point, CLI
├── server.rs                  # Router with service multiplexer
├── services/
│   ├── mod.rs
│   ├── s3/                    # Refactored from current flat structure
│   │   ├── mod.rs
│   │   ├── routes/
│   │   ├── storage/
│   │   └── types/
│   └── secretsmanager/        # New service
│       ├── mod.rs
│       ├── dispatcher.rs      # X-Amz-Target → handler routing
│       ├── handlers.rs        # Operation handlers
│       ├── storage.rs         # Filesystem-backed secret storage
│       ├── types.rs           # Request/response JSON types
│       └── error.rs           # Secrets Manager error types
├── error.rs                   # Shared error infrastructure
├── middleware.rs               # Shared middleware (headers, CORS)
└── types/                     # Shared types
```

### Cargo Dependencies (New)

No new crates needed — `serde_json` (already present), `uuid` (already present), `chrono` (already present) cover all requirements. The JSON protocol is simpler than S3's XML.

---

## Success Criteria

- [ ] All P0 operations pass AWS SDK compatibility tests (Rust, Python, JS)
- [ ] Existing S3 functionality unaffected (all 189 S3 tests still pass)
- [ ] Secrets persist across container restarts via volume mount
- [ ] `GetSecretValue` latency < 5ms for local requests
- [ ] AWSCURRENT/AWSPREVIOUS version rotation works correctly
- [ ] Recovery window delete + restore works
- [ ] Same `docker run` command serves both S3 and Secrets Manager
- [ ] README updated with Secrets Manager SDK configuration examples

## Out of Scope

- KMS encryption (accept KmsKeyId, store it, don't actually encrypt)
- Lambda-based rotation execution (store rotation config, don't invoke Lambdas)
- IAM policy enforcement on resource policies (store-only)
- Cross-region replication (store metadata, don't replicate)
- Service quotas enforcement (no limit on secret count)
- Automatic secret expiration/garbage collection of deprecated versions
- CloudWatch metrics or CloudTrail logging

---

## Timeline Estimate

| Phase | Scope | Relative Effort |
|-------|-------|-----------------|
| 1 | Foundation + Core CRUD | Large (new protocol, storage engine, multiplexer) |
| 2 | Metadata + Discovery | Medium |
| 3 | Versioning, Tags, Policies | Medium |
| 4 | Polish + Batch | Small |

Phase 1 is the heaviest lift due to the new JSON protocol dispatcher and service multiplexing architecture. Phases 2-4 are incremental additions on the established foundation.
