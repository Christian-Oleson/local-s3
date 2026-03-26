<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-26 -->
<!-- Phase: SM-3 -->

<plan>
  <metadata>
    <phase>SM-3</phase>
    <name>Version Management, Tags, Policies, Rotation</name>
    <goal>Advanced version stage management, tagging, resource policies, rotation config storage</goal>
    <deliverable>Full LocalStack Community parity for Secrets Manager</deliverable>
    <created>2026-03-26</created>
  </metadata>

  <context>
    <dependencies>SM Phase 2 complete — 9 SM operations implemented, 216 tests</dependencies>
    <affected_areas>
      - src/secretsmanager/storage.rs — 8 new methods (all simple CRUD)
      - src/secretsmanager/dispatcher.rs — 8 new dispatch entries
      - src/secretsmanager/types.rs — request/response types for 8 operations
      - tests/secretsmanager_integration.rs — integration tests
    </affected_areas>
    <patterns_to_follow>
      - All operations: POST / with X-Amz-Target, JSON request/response
      - Tags stored in metadata.json tags array
      - Resource policy stored in policy.json (raw JSON string)
      - Rotation config stored in metadata.json (rotation_enabled, rotation_lambda_arn, rotation_rules)
      - UpdateSecretVersionStage: move/remove labels, update version files + metadata
      - All store-only (no enforcement, no Lambda invocation)
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>All 8 operations: version stages, tags, policies, rotation</name>
      <description>
        Implement UpdateSecretVersionStage, TagResource, UntagResource,
        PutResourcePolicy, GetResourcePolicy, DeleteResourcePolicy,
        RotateSecret, CancelRotateSecret. All are simple storage operations.
        Include unit tests.
      </description>

      <files>
        <modify>src/secretsmanager/storage.rs    — 8 new methods</modify>
        <modify>src/secretsmanager/dispatcher.rs — 8 new dispatch entries</modify>
        <modify>src/secretsmanager/types.rs      — request/response types</modify>
      </files>

      <action>
        1. UpdateSecretVersionStage:
           Request: { SecretId, VersionStage, MoveToVersionId, RemoveFromVersionId }
           Response: { ARN, Name }
           - Move a staging label from one version to another
           - Update version files on disk + metadata.version_ids_to_stages

        2. TagResource:
           Request: { SecretId, Tags: [{Key,Value}] }
           Response: {} (empty)
           - Additive: merge new tags into existing, overwrite on key match
           - Update metadata.json

        3. UntagResource:
           Request: { SecretId, TagKeys: [String] }
           Response: {} (empty)
           - Remove matching tag keys from metadata

        4. PutResourcePolicy:
           Request: { SecretId, ResourcePolicy: String, BlockPublicPolicy: bool }
           Response: { ARN, Name }
           - Store raw JSON policy string in policy.json file

        5. GetResourcePolicy:
           Request: { SecretId }
           Response: { ARN, Name, ResourcePolicy: String }
           - Return stored policy or null if none

        6. DeleteResourcePolicy:
           Request: { SecretId }
           Response: { ARN, Name }
           - Delete policy.json file

        7. RotateSecret:
           Request: { SecretId, ClientRequestToken, RotationLambdaARN, RotationRules: { AutomaticallyAfterDays, Duration, ScheduleExpression }, RotateImmediately }
           Response: { ARN, Name, VersionId }
           - Store rotation config in metadata (rotation_enabled=true, rotation_lambda_arn, rotation_rules)
           - Don't actually invoke Lambda — just store the config
           - Create a new version with AWSPENDING label if RotateImmediately (default true)

        8. CancelRotateSecret:
           Request: { SecretId }
           Response: { ARN, Name }
           - Set rotation_enabled=false, clear AWSPENDING label

        Add unit tests for each operation.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - All 8 operations work through dispatcher
        - Tags are additive/removable
        - Policy stored/returned as raw JSON string
        - Rotation config stored in metadata
        - All 216 existing tests pass
      </done>
    </task>

    <task id="2" type="test" complete="false">
      <name>Integration tests for all Phase 3 operations</name>
      <description>
        AWS SDK integration tests for version stages, tags, policies, rotation.
      </description>

      <files>
        <modify>tests/secretsmanager_integration.rs</modify>
      </files>

      <action>
        Add integration tests:
        - test_tag_and_untag_resource: add tags, describe → verify, remove tag, describe → verify
        - test_put_and_get_resource_policy: put JSON policy, get → verify round-trip
        - test_delete_resource_policy: put, delete, get → null/error
        - test_rotate_secret: configure rotation, describe → rotation_enabled=true
        - test_update_version_stage: create, put new value, move custom label, verify
      </action>

      <verification>
        <command>cargo test --test secretsmanager_integration</command>
        <command>cargo test</command>
      </verification>

      <done>
        - All integration tests pass with real AWS SDK
        - All existing tests still pass
        - Full LocalStack Community parity for SM
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
    <criterion>All 8 new operations work via AWS SDK</criterion>
    <criterion>17 total SM operations implemented</criterion>
    <criterion>All 216+ tests pass</criterion>
  </completion_criteria>
</plan>
