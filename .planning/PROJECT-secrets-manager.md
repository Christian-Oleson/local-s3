# Local Secrets Manager

## Vision

A drop-in AWS Secrets Manager replacement sharing local-s3's Docker image and port (4566), enabling developers to use Secrets Manager in local development with zero account signup.

## North Star Metric

Any application using `aws-sdk-*` Secrets Manager client works against this service with zero code changes — only an endpoint URL override.

## Target Users

- **Primary:** Developers running AWS-based apps locally that read secrets at startup
- **Secondary:** CI/CD pipelines needing deterministic secret values for integration tests
- **Tertiary:** IaC developers (CDK/Terraform) provisioning secrets

## Core Requirements

### Must Have (P0)

- [ ] AWS JSON 1.1 protocol dispatcher (X-Amz-Target routing, JSON errors)
- [ ] Service multiplexing — Secrets Manager + S3 on port 4566
- [ ] CreateSecret (string + binary, tags, description, idempotency token, ARN generation)
- [ ] GetSecretValue (by name/ARN, by version_id, by version_stage, AWSCURRENT default)
- [ ] PutSecretValue (new version with AWSCURRENT/AWSPREVIOUS label rotation)
- [ ] UpdateSecret (metadata + optional value update)
- [ ] DeleteSecret (ForceDeleteWithoutRecovery + recovery window scheduling)
- [ ] RestoreSecret (cancel scheduled deletion)
- [ ] DescribeSecret (full metadata including VersionIdsToStages map)
- [ ] ListSecrets (pagination, filtering by name/tag/description)
- [ ] ListSecretVersionIds (version enumeration with staging labels)

### Should Have (P1)

- [ ] UpdateSecretVersionStage (move staging labels between versions)
- [ ] TagResource / UntagResource
- [ ] GetResourcePolicy / PutResourcePolicy / DeleteResourcePolicy (store-only)
- [ ] RotateSecret / CancelRotateSecret (store config, no Lambda execution)

### Nice to Have (P2)

- [ ] BatchGetSecretValue (bulk retrieval with partial failure)
- [ ] ValidateResourcePolicy (basic JSON validation)
- [ ] Replication metadata (store only, no actual replication)

## Technical Stack

- **Language:** Rust (same binary as local-s3)
- **HTTP Framework:** axum (existing)
- **Protocol:** AWS JSON 1.1 (POST /, X-Amz-Target dispatch, JSON bodies)
- **Storage:** Local filesystem ({data-dir}/.secrets-manager/)
- **Dependencies:** No new crates — serde_json, uuid, chrono already present

## Constraints

- Must not break existing S3 functionality (189 tests must pass)
- Same Docker image, same port (4566), same CLI args
- HTTP 400 (not 404) for ResourceNotFoundException — AWS quirk, must match for SDK compat
- All timestamps are epoch floats (seconds with millisecond precision)
- ARN suffix is 6 random alphanumeric chars, stable for secret lifetime
- Cannot supply both SecretString and SecretBinary in same request
- AWSCURRENT label must exist on exactly one version per secret at all times

## Success Criteria

- [ ] All P0 operations pass AWS SDK compatibility tests (Rust, Python, JS)
- [ ] Existing S3 functionality unaffected (189 S3 tests still pass)
- [ ] Secrets persist across container restarts via volume mount
- [ ] AWSCURRENT/AWSPREVIOUS version rotation works correctly
- [ ] Recovery window delete + restore works
- [ ] Same `docker run` command serves both S3 and Secrets Manager

## Out of Scope

- KMS encryption (accept KmsKeyId, store it, don't encrypt)
- Lambda-based rotation execution (store config, don't invoke)
- IAM policy enforcement on resource policies (store-only)
- Cross-region replication (metadata only)
- Service quotas enforcement
- Automatic deprecated version garbage collection
