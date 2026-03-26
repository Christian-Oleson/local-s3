# Roadmap — Secrets Manager

## Overview

Total Phases: 4
Estimated Complexity: High

---

## Phase 1: Foundation + Core CRUD

**Goal:** JSON protocol dispatcher, service multiplexing, and core secret CRUD operations
**Deliverable:** Developers can create, read, update, delete secrets via any AWS SDK
**Complexity:** High (new protocol pattern, new storage engine, multiplexing)

### Tasks

- [ ] 1.1: Implement AWS JSON 1.1 protocol dispatcher (X-Amz-Target routing, JSON error format with __type + Message)
- [ ] 1.2: Implement service multiplexer on port 4566 (detect secretsmanager.* target → Secrets Manager, else → S3)
- [ ] 1.3: Implement Secrets Manager storage engine (filesystem-backed: .secrets-manager/secrets/{name}/, versions, metadata.json)
- [ ] 1.4: Implement CreateSecret (ARN generation with 6-char suffix, initial version with AWSCURRENT, SecretString/Binary, Tags, Description)
- [ ] 1.5: Implement GetSecretValue (by name/ARN, by VersionId, by VersionStage, AWSCURRENT default)
- [ ] 1.6: Implement PutSecretValue (new version, AWSCURRENT/AWSPREVIOUS label rotation, idempotency via ClientRequestToken)
- [ ] 1.7: Implement DeleteSecret (ForceDeleteWithoutRecovery, RecoveryWindowInDays, scheduled deletion state) + RestoreSecret
- [ ] 1.8: Write integration tests using aws-sdk-rust secretsmanager client

### Dependencies

- None (builds on existing local-s3 infrastructure)

---

## Phase 2: Metadata + Discovery

**Goal:** Full secret metadata, listing with filtering/pagination, version enumeration
**Deliverable:** Full secret lifecycle management and discovery
**Complexity:** Medium

### Tasks

- [ ] 2.1: Implement DescribeSecret (full metadata: VersionIdsToStages, Tags, RotationConfig, dates, DeletedDate)
- [ ] 2.2: Implement ListSecrets (pagination with MaxResults/NextToken, Filters: name, tag-key, tag-value, description, SortOrder)
- [ ] 2.3: Implement UpdateSecret (metadata-only update + optional new version, combined Description/KmsKeyId/value)
- [ ] 2.4: Implement ListSecretVersionIds (version enumeration, IncludeDeprecated flag, pagination)
- [ ] 2.5: Write integration tests for all discovery/metadata operations

### Dependencies

- Phase 1 complete

---

## Phase 3: Version Management, Tags, Policies, Rotation

**Goal:** Advanced version stage management, tagging, resource policies, rotation config storage
**Deliverable:** Full LocalStack Community parity for Secrets Manager
**Complexity:** Medium

### Tasks

- [ ] 3.1: Implement UpdateSecretVersionStage (move/remove staging labels between versions)
- [ ] 3.2: Implement TagResource + UntagResource (additive tags, case-sensitive keys, max 50)
- [ ] 3.3: Implement GetResourcePolicy + PutResourcePolicy + DeleteResourcePolicy (store raw JSON, no enforcement)
- [ ] 3.4: Implement RotateSecret + CancelRotateSecret (store RotationLambdaARN + RotationRules, set RotationEnabled, no Lambda invocation)
- [ ] 3.5: Write integration tests for version stages, tags, policies, rotation config

### Dependencies

- Phase 2 complete

---

## Phase 4: Batch Operations + Documentation

**Goal:** BatchGetSecretValue, policy validation, documentation update
**Deliverable:** Complete Secrets Manager emulator ready for team adoption
**Complexity:** Low

### Tasks

- [ ] 4.1: Implement BatchGetSecretValue (bulk retrieval, partial failures in Errors array, pagination)
- [ ] 4.2: Implement ValidateResourcePolicy (basic JSON structure validation, return ValidationErrors)
- [ ] 4.3: Update README with Secrets Manager section (SDK config examples, supported operations table)
- [ ] 4.4: Update CLAUDE.md with Secrets Manager architecture notes
- [ ] 4.5: Update Dockerfile and docker-compose.yml if needed

### Dependencies

- Phase 3 complete

---

## Milestones

| Milestone | Phase | Description |
|-----------|-------|-------------|
| Protocol Works | 1 | JSON dispatcher routes to Secrets Manager, first CreateSecret/GetSecretValue works |
| MVP | 1 | Core CRUD — apps can read/write secrets via SDK |
| Full Discovery | 2 | List, describe, update, version enumeration |
| LocalStack Parity | 3 | Tags, policies, rotation config — matches free LocalStack |
| Ship It | 4 | Batch ops, docs, ready for team adoption |
