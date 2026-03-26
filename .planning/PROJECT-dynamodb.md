# Local DynamoDB

## Vision

A Rust-native DynamoDB emulator embedded in the local-s3 binary, enabling developers to run DynamoDB-backed applications locally without JRE, LocalStack accounts, or AWS credentials.

## North Star Metric

Any application using `aws-sdk-*` DynamoDB client works against this service with zero code changes — only an endpoint URL override.

## Target Users

- **Primary:** Developers running DynamoDB-backed applications locally (HealioSpace uses 5+ DynamoDB tables)
- **Secondary:** CI/CD pipelines running integration tests
- **Tertiary:** IaC developers (CDK/Terraform) provisioning tables

## Core Requirements

### Must Have (P0)

- [ ] DynamoDB_20120810 protocol dispatcher (JSON 1.0, X-Amz-Target routing)
- [ ] AttributeValue type system (all 10 types: S, N, B, BOOL, NULL, L, M, SS, NS, BS)
- [ ] Expression engine: condition, update, key condition, filter, projection expressions
- [ ] CreateTable (KeySchema, AttributeDefinitions, BillingMode, GSI, LSI)
- [ ] DescribeTable (full TableDescription with ARN, status, item count)
- [ ] DeleteTable, ListTables
- [ ] PutItem (ConditionExpression, ReturnValues)
- [ ] GetItem (key lookup, ProjectionExpression, ConsistentRead accepted)
- [ ] UpdateItem (UpdateExpression SET/REMOVE/ADD/DELETE, ConditionExpression, ReturnValues)
- [ ] DeleteItem (ConditionExpression, ReturnValues)
- [ ] Query (KeyConditionExpression, FilterExpression, IndexName, pagination, ScanIndexForward)
- [ ] Scan (FilterExpression, pagination, Limit)

### Should Have (P1)

- [ ] BatchWriteItem (up to 25 put/delete across tables)
- [ ] BatchGetItem (up to 100 keys across tables)
- [ ] TransactWriteItems (Put/Update/Delete/ConditionCheck, all-or-nothing)
- [ ] TransactGetItems (atomic multi-item read)
- [ ] UpdateTable (GSI add/remove, billing mode change)
- [ ] UpdateTimeToLive / DescribeTimeToLive (store config)
- [ ] TagResource / UntagResource / ListTagsOfResource (store-only)

### Nice to Have (P2)

- [ ] DynamoDB Streams (4 operations, separate target prefix)
- [ ] PartiQL (ExecuteStatement, BatchExecuteStatement)
- [ ] Backup/restore metadata storage
- [ ] Continuous backups / PITR config
- [ ] Global tables config
- [ ] Resource policies

## Technical Stack

- **Language:** Rust (same binary as S3 + Secrets Manager)
- **Protocol:** AWS JSON 1.0 (POST /, DynamoDB_20120810.* targets)
- **Storage:** Filesystem ({data-dir}/.dynamodb/tables/{name}/)
- **Expression engine:** Hand-written recursive descent parser (no new crates needed)
- **Dependencies:** No new crates — serde_json, uuid, chrono sufficient

## Constraints

- Must not break existing S3 (189 tests) or Secrets Manager (36 tests)
- Same Docker image, same port (4566), same CLI args
- Numbers are strings on the wire (arbitrary precision)
- Key attributes limited to S, N, B types
- Empty sets are invalid (DynamoDB rejects them)
- Tables are immediately ACTIVE (no CREATING state for local dev)
- Item size limit: 400 KB (should enforce for SDK compatibility)
- Expression engine must handle ExpressionAttributeNames (#placeholders) and ExpressionAttributeValues (:placeholders)

## Success Criteria

- [ ] All P0 operations pass AWS SDK compatibility tests
- [ ] Expression engine evaluates condition, update, filter, projection correctly
- [ ] GSI and LSI queries work
- [ ] HealioSpace API tables can be created and queried
- [ ] Existing S3 + SM tests unaffected
- [ ] Items persist across container restarts

## Out of Scope

- Provisioned throughput enforcement (accept, don't throttle)
- DynamoDB Streams event delivery
- PartiQL query language
- DAX (DynamoDB Accelerator)
- Export/Import operations
- Global table replication
- Encryption at rest
- Backup execution
