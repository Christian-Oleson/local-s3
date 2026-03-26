# Roadmap — DynamoDB

## Overview

Total Phases: 5
Estimated Complexity: Very High (expression engine is a mini language)

---

## Phase 1: Foundation + Table Management

**Goal:** Protocol dispatcher, AttributeValue types, table CRUD
**Deliverable:** CreateTable/DescribeTable/DeleteTable/ListTables work via SDK
**Complexity:** Medium

### Tasks

- [ ] 1.1: DynamoDB protocol dispatcher (DynamoDB_20120810.* routing, JSON 1.0 content type, error format)
- [ ] 1.2: AttributeValue enum with full serde support (10 types: S, N, B, BOOL, NULL, L, M, SS, NS, BS)
- [ ] 1.3: DynamoDB storage engine (table metadata + item storage at .dynamodb/tables/{name}/)
- [ ] 1.4: CreateTable handler (KeySchema, AttributeDefinitions, BillingMode, GSI, LSI, Projection)
- [ ] 1.5: DescribeTable handler (full TableDescription: status, ARN, item count, key schema, indexes)
- [ ] 1.6: DeleteTable, ListTables handlers (with pagination)
- [ ] 1.7: Integration tests with aws-sdk-dynamodb

### Dependencies

- None (builds on existing multiplexer pattern)

---

## Phase 2: Expression Engine + Item CRUD

**Goal:** Full expression parser/evaluator, PutItem/GetItem/UpdateItem/DeleteItem
**Deliverable:** Applications can do full CRUD with conditions, updates, and projections
**Complexity:** Very High (expression engine is a mini programming language)

### Tasks

- [ ] 2.1: Expression lexer (tokenize expression strings into tokens)
- [ ] 2.2: Expression parser (recursive descent, build typed AST)
- [ ] 2.3: Condition expression evaluator (comparisons, AND/OR/NOT, functions: attribute_exists, begins_with, contains, size, etc.)
- [ ] 2.4: Update expression evaluator (SET with if_not_exists/list_append, REMOVE, ADD for numbers/sets, DELETE for set members)
- [ ] 2.5: Projection expression evaluator (attribute paths, nested access, list indexing)
- [ ] 2.6: ExpressionAttributeNames (#placeholder) and ExpressionAttributeValues (:placeholder) substitution
- [ ] 2.7: PutItem + GetItem handlers (with ConditionExpression, ProjectionExpression, ReturnValues)
- [ ] 2.8: UpdateItem + DeleteItem handlers (with UpdateExpression, ConditionExpression, ReturnValues)
- [ ] 2.9: Integration tests for CRUD with various expression patterns

### Dependencies

- Phase 1 complete

---

## Phase 3: Query, Scan, Index Support

**Goal:** Query with key conditions, Scan with filters, GSI/LSI index queries
**Deliverable:** Applications can query data using indexes with pagination
**Complexity:** High

### Tasks

- [ ] 3.1: Key condition expression evaluator (=, <, <=, >, >=, BETWEEN, begins_with on sort key)
- [ ] 3.2: Query handler (KeyConditionExpression, FilterExpression, pagination via LastEvaluatedKey/ExclusiveStartKey)
- [ ] 3.3: Scan handler (FilterExpression, pagination, Limit)
- [ ] 3.4: GSI query support (maintain secondary index item projections, query against them)
- [ ] 3.5: LSI query support (same partition key, alternate sort key)
- [ ] 3.6: ScanIndexForward (ascending/descending sort key order)
- [ ] 3.7: Integration tests for query/scan with indexes and pagination

### Dependencies

- Phase 2 complete

---

## Phase 4: Batch + Transactions

**Goal:** Batch operations and transactional writes/reads
**Deliverable:** Production patterns like bulk operations and ACID transactions work
**Complexity:** Medium

### Tasks

- [ ] 4.1: BatchWriteItem (multi-table put/delete, up to 25, UnprocessedItems)
- [ ] 4.2: BatchGetItem (multi-table get, up to 100, UnprocessedKeys)
- [ ] 4.3: TransactWriteItems (Put/Update/Delete/ConditionCheck, all-or-nothing, CancellationReasons)
- [ ] 4.4: TransactGetItems (atomic multi-item read)
- [ ] 4.5: Integration tests for batch and transaction operations

### Dependencies

- Phase 3 complete

---

## Phase 5: Table Features + Documentation

**Goal:** UpdateTable, TTL, tags, documentation update
**Deliverable:** Complete DynamoDB emulator ready for team adoption
**Complexity:** Low

### Tasks

- [ ] 5.1: UpdateTable (GSI add/remove, billing mode change, stream config)
- [ ] 5.2: UpdateTimeToLive / DescribeTimeToLive (store config)
- [ ] 5.3: TagResource / UntagResource / ListTagsOfResource (store-only)
- [ ] 5.4: Update README with DynamoDB section (SDK config, operations table)
- [ ] 5.5: Update CLAUDE.md with DynamoDB architecture notes

### Dependencies

- Phase 4 complete

---

## Milestones

| Milestone | Phase | Description |
|-----------|-------|-------------|
| Tables Work | 1 | Create/describe/delete tables via SDK |
| CRUD Works | 2 | PutItem/GetItem/UpdateItem/DeleteItem with expressions |
| Query Works | 3 | Query/Scan with indexes — most apps functional |
| Full Compat | 4 | Batch + transactions — production patterns work |
| Ship It | 5 | Docs, features, ready for team adoption |
