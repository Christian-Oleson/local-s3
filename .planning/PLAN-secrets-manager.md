<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-26 -->
<!-- Phase: SM-4 (final) -->

<plan>
  <metadata>
    <phase>SM-4</phase>
    <name>Batch Operations + Documentation</name>
    <goal>BatchGetSecretValue, ValidateResourcePolicy, complete documentation</goal>
    <deliverable>Complete Secrets Manager emulator ready for team adoption</deliverable>
    <created>2026-03-26</created>
  </metadata>

  <context>
    <dependencies>SM Phase 3 complete — 17 SM operations, 221 tests</dependencies>
    <affected_areas>
      - src/secretsmanager/ — 2 new operations
      - README.md — Secrets Manager section
      - CLAUDE.md — SM architecture update
    </affected_areas>
    <patterns_to_follow>
      - Same dispatcher + storage + types pattern
      - BatchGetSecretValue has partial failure semantics (SecretValues + Errors arrays)
      - ValidateResourcePolicy just checks JSON parsability
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>BatchGetSecretValue, ValidateResourcePolicy, integration tests</name>
      <description>
        Implement the final 2 operations plus integration tests. BatchGetSecretValue
        retrieves multiple secrets in one call with partial failure support.
        ValidateResourcePolicy does basic JSON validation.
      </description>

      <files>
        <modify>src/secretsmanager/storage.rs</modify>
        <modify>src/secretsmanager/dispatcher.rs</modify>
        <modify>src/secretsmanager/types.rs</modify>
        <modify>tests/secretsmanager_integration.rs</modify>
      </files>

      <action>
        1. BatchGetSecretValue:
           Request: { SecretIdList: [String], MaxResults: i32, NextToken: String }
           Response: { SecretValues: [...], Errors: [...], NextToken: String }

           SecretValues entry: same as GetSecretValueResponse
           Errors entry: { SecretId, ErrorCode, Message }

           - For each SecretId in list, try get_secret_value
           - Successes go in SecretValues, failures go in Errors
           - Pagination via MaxResults + NextToken (base64 index)

        2. ValidateResourcePolicy:
           Request: { SecretId: Option, ResourcePolicy: String }
           Response: { PolicyValidationPassed: bool, ValidationErrors: [...] }

           - Try to parse ResourcePolicy as JSON
           - If valid JSON: PolicyValidationPassed = true, empty errors
           - If invalid: PolicyValidationPassed = false, one error with message

        3. Integration tests:
           - test_batch_get_secret_value: create 3 secrets, batch get → all 3 in SecretValues
           - test_batch_get_partial_failure: create 1 secret, batch get with 1 real + 1 fake → 1 success + 1 error
           - test_validate_resource_policy_valid: valid JSON → passed
           - test_validate_resource_policy_invalid: invalid JSON → not passed with error
      </action>

      <verification>
        <command>cargo test</command>
        <command>cargo clippy -- -D warnings</command>
      </verification>

      <done>
        - BatchGetSecretValue works with partial failures
        - ValidateResourcePolicy checks JSON parsability
        - 4 new integration tests pass
        - All 221 existing tests pass
      </done>
    </task>

    <task id="2" type="setup" complete="false">
      <name>Update README and CLAUDE.md with Secrets Manager documentation</name>
      <description>
        Add Secrets Manager section to README with SDK config examples and
        supported operations table. Update CLAUDE.md architecture notes.
      </description>

      <files>
        <modify>README.md</modify>
        <modify>CLAUDE.md</modify>
      </files>

      <action>
        1. README.md — add "Secrets Manager" section after S3 operations:
           - SDK config examples showing endpoint_url usage for SM clients
           - Supported SM operations table (19 operations)
           - Note that SM uses JSON protocol (not XML like S3)

        2. CLAUDE.md — update the SM section with final operation count and architecture
      </action>

      <verification>
        <manual>Review README for completeness and accuracy</manual>
      </verification>

      <done>
        - README has SM section with SDK examples and operations table
        - CLAUDE.md reflects final SM architecture
      </done>
    </task>
  </tasks>

  <phase_verification>
    <commands>
      <command>cargo build</command>
      <command>cargo test</command>
      <command>cargo fmt -- --check</command>
      <command>cargo clippy -- -D warnings</command>
    </commands>
  </phase_verification>

  <completion_criteria>
    <criterion>Both tasks complete</criterion>
    <criterion>19 total SM operations</criterion>
    <criterion>README documents both S3 and Secrets Manager</criterion>
    <criterion>All tests pass</criterion>
  </completion_criteria>
</plan>
