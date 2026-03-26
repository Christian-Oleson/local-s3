# PRD: Local DynamoDB Service

## Executive Summary

Add DynamoDB as the third AWS service in local-s3, enabling developers to run applications that depend on DynamoDB without LocalStack accounts or real AWS credentials. DynamoDB is the most complex service to emulate due to its expression engine, but also the most impactful — it's the primary database for serverless AWS applications.

## Problem Statement

DynamoDB is central to the HealioSpace platform (calendar, files, messages, subscriptions, users tables). Every developer running the API locally needs DynamoDB. The current options are:

1. **LocalStack** — requires account signup
2. **DynamoDB Local** (Amazon's Java emulator) — requires JRE, 300MB+ download, separate process
3. **Real AWS** — costs money, requires connectivity, shared state across developers

A Rust-native DynamoDB emulator embedded in the same binary eliminates all three problems.

## Vision

DynamoDB joins S3 and Secrets Manager on port 4566. Same binary, same Docker image, same `AWS_ENDPOINT_URL`. No JRE. No separate process. Sub-millisecond local latency.

## Target Users

- **Primary:** Developers running DynamoDB-backed applications locally (HealioSpace API uses 5+ DynamoDB tables)
- **Secondary:** CI/CD pipelines running integration tests against DynamoDB
- **Tertiary:** IaC developers (CDK/Terraform) provisioning tables

## North Star Metric

Any application using `aws-sdk-*` DynamoDB client works against this service with zero code changes — only an endpoint URL override.

---

## Technical Design

### Protocol: AWS JSON 1.0

DynamoDB uses **AWS JSON 1.0** — similar to Secrets Manager's JSON 1.1 but with a different Content-Type and target prefix:

| Aspect | DynamoDB | Secrets Manager | S3 |
|--------|----------|-----------------|-----|
| Content-Type | `application/x-amz-json-1.0` | `application/x-amz-json-1.1` | N/A (XML) |
| X-Amz-Target | `DynamoDB_20120810.<Op>` | `secretsmanager.<Op>` | N/A |
| Request/Response | JSON | JSON | XML |
| Errors | `{"__type": "...", "Message": "..."}` | Same | XML |

### Service Multiplexing (extends existing pattern)

```
POST / + X-Amz-Target: DynamoDB_20120810.*      → DynamoDB handler
POST / + X-Amz-Target: secretsmanager.*          → Secrets Manager handler (existing)
Everything else                                   → S3 handler (existing)
```

### Storage Model

```
{data-dir}/
├── .dynamodb/                    # DynamoDB data root
│   └── tables/
│       └── {table-name}/
│           ├── metadata.json     # KeySchema, AttributeDefinitions, GSI/LSI, billing, TTL, tags
│           └── items/
│               └── {partition-hash}/
│                   └── {sort-key-hash}.json  # Full item as AttributeValue map
├── .secrets-manager/             # Secrets Manager (existing)
└── bucket-a/                     # S3 buckets (existing)
```

Items are stored as JSON files keyed by hash of their primary key. This allows efficient single-item lookups while keeping the implementation simple. Query/Scan operations load and filter in-memory (acceptable for local dev volumes).

### The Expression Engine (Critical Subsystem)

DynamoDB's expression language is the single most complex component. It requires:

1. **Lexer/Parser** for expression grammar
2. **Evaluator** for condition/filter expressions
3. **Updater** for update expressions
4. **Projector** for projection expressions

Supported expression types:
- **ConditionExpression**: `attribute_exists(#a) AND #b > :val OR size(#c) > :n`
- **UpdateExpression**: `SET #a = :val, #b = list_append(#b, :items) REMOVE #c ADD counter :one DELETE #s :subset`
- **KeyConditionExpression**: `PK = :pk AND SK begins_with(:prefix)`
- **FilterExpression**: same grammar as ConditionExpression
- **ProjectionExpression**: `#a, #b.nested, items[0]`

Functions: `attribute_exists`, `attribute_not_exists`, `attribute_type`, `begins_with`, `contains`, `size`, `if_not_exists`, `list_append`

Operators: `=`, `<>`, `<`, `<=`, `>`, `>=`, `BETWEEN ... AND ...`, `IN (...)`, `AND`, `OR`, `NOT`

### AttributeValue Type System

DynamoDB has 10 data types, each represented as a single-key JSON object:

| Type | Key | Rust Representation |
|------|-----|---------------------|
| String | `S` | `String` |
| Number | `N` | `String` (preserves precision) |
| Binary | `B` | `String` (base64) |
| Boolean | `BOOL` | `bool` |
| Null | `NULL` | `bool` (always true) |
| List | `L` | `Vec<AttributeValue>` |
| Map | `M` | `HashMap<String, AttributeValue>` |
| String Set | `SS` | `Vec<String>` |
| Number Set | `NS` | `Vec<String>` |
| Binary Set | `BS` | `Vec<String>` |

Numbers are strings on the wire to preserve arbitrary precision. Key attributes may only be S, N, or B.

---

## Core Requirements

### Must Have (P0)

| # | Operation | Notes |
|---|-----------|-------|
| 1 | DynamoDB_20120810 protocol dispatcher | Route targets, JSON 1.0 content type |
| 2 | AttributeValue type system | All 10 types, serde serialization |
| 3 | Expression engine (parser + evaluator) | Condition, update, key condition, filter, projection |
| 4 | CreateTable | KeySchema, AttributeDefinitions, BillingMode, GSI, LSI |
| 5 | DescribeTable | Full TableDescription with status, counts, ARN |
| 6 | DeleteTable | Basic delete |
| 7 | ListTables | Pagination |
| 8 | PutItem | ConditionExpression, ReturnValues |
| 9 | GetItem | Key lookup, ProjectionExpression, ConsistentRead (accepted/ignored) |
| 10 | DeleteItem | ConditionExpression, ReturnValues |
| 11 | UpdateItem | UpdateExpression (SET/REMOVE/ADD/DELETE), ConditionExpression, ReturnValues |
| 12 | Query | KeyConditionExpression, FilterExpression, IndexName (GSI/LSI), pagination, ScanIndexForward |
| 13 | Scan | FilterExpression, pagination, Limit |

### Should Have (P1)

| # | Operation | Notes |
|---|-----------|-------|
| 14 | BatchWriteItem | Up to 25 put/delete across tables, UnprocessedItems |
| 15 | BatchGetItem | Up to 100 keys across tables, UnprocessedKeys |
| 16 | TransactWriteItems | Put/Update/Delete/ConditionCheck, all-or-nothing |
| 17 | TransactGetItems | Atomic multi-item read |
| 18 | UpdateTable | GSI add/remove, stream config |
| 19 | UpdateTimeToLive / DescribeTimeToLive | Store config (optional: execute TTL expiry) |
| 20 | TagResource / UntagResource / ListTagsOfResource | Store-only |

### Nice to Have (P2)

| # | Operation | Notes |
|---|-----------|-------|
| 21 | DynamoDB Streams | Separate target prefix, 4 operations |
| 22 | PartiQL | ExecuteStatement, BatchExecuteStatement |
| 23 | Backups | CreateBackup, DescribeBackup, etc. (store metadata) |
| 24 | Continuous Backups / PITR | Store config |
| 25 | Global Tables | Store config |
| 26 | Resource Policies | Store-only |

---

## Implementation Phases

### Phase 1: Foundation + Table Management (High complexity)

**Goal:** Protocol dispatcher, AttributeValue type system, table CRUD
**Deliverable:** CreateTable/DescribeTable/DeleteTable/ListTables work via SDK

Tasks:
1. DynamoDB protocol dispatcher (DynamoDB_20120810.* routing)
2. AttributeValue enum with full serde support (10 types)
3. DynamoDB storage engine (table metadata + item storage)
4. CreateTable handler (KeySchema, AttributeDefinitions, BillingMode, GSI, LSI)
5. DescribeTable, DeleteTable, ListTables handlers
6. Integration tests with aws-sdk-dynamodb

### Phase 2: Expression Engine + Item CRUD (Highest complexity)

**Goal:** Expression parser/evaluator, PutItem/GetItem/UpdateItem/DeleteItem
**Deliverable:** Applications can do full CRUD with conditions and updates

Tasks:
1. Expression lexer (tokenize expression strings)
2. Expression parser (build AST from tokens)
3. Condition expression evaluator (comparisons, logical operators, functions)
4. Update expression evaluator (SET/REMOVE/ADD/DELETE)
5. Key condition expression evaluator (for Query)
6. Projection expression evaluator
7. PutItem + GetItem handlers
8. UpdateItem + DeleteItem handlers
9. Integration tests for all CRUD with expressions

**This is the hardest phase.** The expression engine is a mini programming language. Consider using a parser combinator library (e.g., `nom`) or hand-written recursive descent.

### Phase 3: Query, Scan, Indexes (High complexity)

**Goal:** Query with key conditions, Scan with filters, GSI/LSI index queries
**Deliverable:** Applications can query data efficiently using indexes

Tasks:
1. Query handler (key condition + filter, pagination, ScanIndexForward)
2. Scan handler (filter, pagination, Limit)
3. GSI query support (secondary index item projections)
4. LSI query support
5. Integration tests for query/scan with various expressions and indexes

### Phase 4: Batch + Transactions (Medium complexity)

**Goal:** BatchWriteItem, BatchGetItem, TransactWriteItems, TransactGetItems
**Deliverable:** Production patterns like bulk operations and transactions work

Tasks:
1. BatchWriteItem (multi-table put/delete, up to 25)
2. BatchGetItem (multi-table get, up to 100)
3. TransactWriteItems (all-or-nothing write with conditions)
4. TransactGetItems (atomic multi-read)
5. Integration tests

### Phase 5: Table Features + Documentation (Low complexity)

**Goal:** UpdateTable, TTL, tags, documentation
**Deliverable:** Complete DynamoDB emulator ready for team adoption

Tasks:
1. UpdateTable (GSI add/remove, billing mode change)
2. TTL config storage (UpdateTimeToLive/DescribeTimeToLive)
3. Tags (TagResource/UntagResource/ListTagsOfResource)
4. Update README with DynamoDB section
5. Update CLAUDE.md

---

## Expression Engine Design

### Grammar (simplified)

```
expression      = or_expr
or_expr         = and_expr ("OR" and_expr)*
and_expr        = not_expr ("AND" not_expr)*
not_expr        = "NOT" not_expr | comparison
comparison      = operand comparator operand
                | operand "BETWEEN" operand "AND" operand
                | operand "IN" "(" operand ("," operand)* ")"
                | function
comparator      = "=" | "<>" | "<" | "<=" | ">" | ">="
function        = "attribute_exists" "(" path ")"
                | "attribute_not_exists" "(" path ")"
                | "attribute_type" "(" path "," operand ")"
                | "begins_with" "(" path "," operand ")"
                | "contains" "(" path "," operand ")"
                | "size" "(" path ")"
operand         = path | literal | function
path            = attribute_name ("." attribute_name | "[" number "]")*
attribute_name  = "#name_placeholder" | identifier
literal         = ":value_placeholder"

update_expr     = set_clause? remove_clause? add_clause? delete_clause?
set_clause      = "SET" set_action ("," set_action)*
set_action      = path "=" operand ("+"|"-") operand | path "=" operand | path "=" function
remove_clause   = "REMOVE" path ("," path)*
add_clause      = "ADD" path operand ("," path operand)*
delete_clause   = "DELETE" path operand ("," path operand)*
```

### Recommended approach

Hand-written recursive descent parser. It's straightforward for this grammar (no operator precedence ambiguity beyond AND/OR/NOT). Each function returns a typed AST node. The evaluator walks the AST against the item's AttributeValue map.

Consider creating a `src/dynamodb/expressions/` module with:
- `lexer.rs` — tokenize expression string
- `parser.rs` — build AST from tokens
- `evaluator.rs` — evaluate condition/filter against item
- `updater.rs` — apply update expression to item
- `projector.rs` — extract projected attributes

---

## Code Organization

```
src/
├── dynamodb/
│   ├── mod.rs
│   ├── dispatcher.rs      # DynamoDB_20120810.* target routing
│   ├── handlers.rs         # Operation handlers
│   ├── storage.rs          # Table + item storage engine
│   ├── types.rs            # AttributeValue, request/response types
│   ├── error.rs            # DynamoDB error types
│   └── expressions/
│       ├── mod.rs
│       ├── lexer.rs        # Tokenizer
│       ├── parser.rs       # AST builder
│       ├── evaluator.rs    # Condition/filter evaluation
│       ├── updater.rs      # Update expression execution
│       └── projector.rs    # Projection expression
├── secretsmanager/         # Existing
└── ...                     # Existing S3 modules
```

### Cargo Dependencies (New)

- No new required crates. `serde_json` handles all JSON. Hand-written parser avoids external deps.
- Optional: `nom` for parser combinators (if hand-written recursive descent proves too complex)

---

## Error Handling

```json
{
  "__type": "com.amazonaws.dynamodb.v20120810#ValidationException",
  "Message": "One or more parameter values were invalid"
}
```

| Error | HTTP | When |
|-------|------|------|
| ResourceNotFoundException | 400 | Table not found |
| ResourceInUseException | 400 | Table already exists |
| ValidationException | 400 | Invalid params, bad expressions |
| ConditionalCheckFailedException | 400 | Condition expression failed |
| TransactionCanceledException | 400 | Transaction failed (with reasons) |
| SerializationException | 400 | Malformed JSON |
| InternalServerError | 500 | Internal error |

Note: DynamoDB uses the full Shape ID format for `__type` (e.g., `com.amazonaws.dynamodb.v20120810#ResourceNotFoundException`), but AWS SDKs accept both short and full forms. We can use short form for simplicity.

---

## Success Criteria

- [ ] All P0 operations pass AWS SDK compatibility tests
- [ ] Existing S3 (189 tests) and SM (36 tests) unaffected
- [ ] Expression engine correctly evaluates condition, update, filter, projection, key condition expressions
- [ ] GSI and LSI queries work correctly
- [ ] Items persist across container restarts
- [ ] HealioSpace API tables can be created and queried
- [ ] Same `docker run` command serves S3 + SM + DynamoDB

## Out of Scope

- Provisioned throughput enforcement (accept config, don't throttle)
- DynamoDB Streams event delivery
- PartiQL query language
- DAX (DynamoDB Accelerator)
- Export/Import operations
- Global table replication
- Encryption at rest (accept config, don't encrypt)
- Backup execution (store config)

---

## Complexity Assessment

| Phase | Scope | Complexity | Why |
|-------|-------|-----------|-----|
| 1 | Table management | Medium | Similar to SM Phase 1 — new dispatcher + storage |
| 2 | Expression engine + CRUD | **Very High** | Mini programming language: lexer, parser, evaluator |
| 3 | Query/Scan + indexes | High | Index materialization, key condition evaluation |
| 4 | Batch + transactions | Medium | Builds on Phase 2 CRUD |
| 5 | Features + docs | Low | Store-only config APIs |

**Phase 2 is the critical path.** The expression engine determines whether the emulator is useful or not. Most DynamoDB applications use condition expressions and update expressions heavily. A partial or buggy expression engine renders the entire service unusable.

**Estimated effort:** Roughly 2x the total effort of the Secrets Manager implementation, primarily due to Phase 2's expression engine.
