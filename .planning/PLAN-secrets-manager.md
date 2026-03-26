<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: SM-2 (Secrets Manager Phase 2) -->

<plan>
  <metadata>
    <phase>SM-2</phase>
    <name>Metadata + Discovery</name>
    <goal>Full secret metadata, listing with filtering/pagination, version enumeration</goal>
    <deliverable>Full secret lifecycle management and discovery via any AWS SDK</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>SM Phase 1 complete — CreateSecret, GetSecretValue, PutSecretValue, DeleteSecret, RestoreSecret, 208 tests</dependencies>
    <affected_areas>
      - src/secretsmanager/storage.rs — add describe, list, update, list_versions methods
      - src/secretsmanager/dispatcher.rs — add 4 new operation handlers
      - src/secretsmanager/types.rs — add request/response types for 4 new operations
      - tests/secretsmanager_integration.rs — new integration tests
    </affected_areas>
    <patterns_to_follow>
      - Follow existing dispatcher pattern: deserialize JSON → call storage → serialize JSON
      - PascalCase JSON field names with serde rename
      - Content-Type: application/x-amz-json-1.1
      - Timestamps as f64 epoch seconds
      - Pagination: MaxResults + NextToken pattern (opaque token)
      - VersionIdsToStages: HashMap of version_id → Vec of staging labels
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>DescribeSecret, ListSecrets, UpdateSecret, ListSecretVersionIds</name>
      <description>
        Implement all 4 metadata/discovery operations in storage + dispatcher + types.
        These are medium-complexity: DescribeSecret is a metadata read, ListSecrets needs
        filtering and pagination, UpdateSecret combines metadata + optional value update,
        ListSecretVersionIds enumerates versions.
      </description>

      <files>
        <modify>src/secretsmanager/storage.rs    — add 4 storage methods</modify>
        <modify>src/secretsmanager/dispatcher.rs — add 4 dispatch handlers</modify>
        <modify>src/secretsmanager/types.rs      — add request/response types</modify>
      </files>

      <action>
        1. Add types to types.rs:

           DescribeSecretRequest: { SecretId }
           DescribeSecretResponse: { ARN, Name, Description, KmsKeyId, RotationEnabled,
             RotationLambdaARN, RotationRules, LastRotatedDate, LastChangedDate,
             LastAccessedDate, DeletedDate, Tags, VersionIdsToStages, CreatedDate }
           Note: VersionIdsToStages is a HashMap&lt;String, Vec&lt;String&gt;&gt;

           ListSecretsRequest: { MaxResults, NextToken, Filters, SortOrder, IncludePlannedDeletion }
           Filter: { Key, Values: Vec&lt;String&gt; }
           ListSecretsResponse: { SecretList: Vec&lt;SecretListEntry&gt;, NextToken }
           SecretListEntry: same fields as DescribeSecretResponse but named SecretVersionsToStages

           UpdateSecretRequest: { SecretId, SecretString, SecretBinary, Description, KmsKeyId, ClientRequestToken }
           UpdateSecretResponse: { ARN, Name, VersionId (optional — only if value changed) }

           ListSecretVersionIdsRequest: { SecretId, MaxResults, NextToken, IncludeDeprecated }
           ListSecretVersionIdsResponse: { ARN, Name, Versions: Vec&lt;SecretVersionInfo&gt;, NextToken }
           SecretVersionInfo: { VersionId, VersionStages, CreatedDate, LastAccessedDate }

        2. Add storage methods:

           a) describe_secret(secret_id) → metadata + version_ids_to_stages
              - Resolve secret_id, return full metadata
              - Build VersionIdsToStages from scanning version files

           b) list_secrets(max_results, next_token, filters, include_planned_deletion) → (Vec&lt;metadata&gt;, next_token)
              - Scan all secret directories
              - Apply filters: "name" (contains), "description" (contains),
                "tag-key" (tag key matches), "tag-value" (tag value matches), "all" (any field)
              - Exclude deleted secrets unless include_planned_deletion
              - Sort by name (default)
              - Paginate with max_results (default 100) + next_token (base64-encoded last name)

           c) update_secret(secret_id, description, kms_key_id, secret_string, secret_binary, client_token)
              - Resolve and check not deleted
              - Update metadata fields (description, kms_key_id) if provided
              - If secret_string or secret_binary provided: create new version (same as put_secret_value)
              - Return version_id only if value was updated

           d) list_secret_version_ids(secret_id, max_results, next_token, include_deprecated) → versions
              - Read all version files for the secret
              - If !include_deprecated: only versions with staging labels
              - Paginate

        3. Add dispatcher entries:
           - "DescribeSecret" → handle_describe_secret
           - "ListSecrets" → handle_list_secrets
           - "UpdateSecret" → handle_update_secret
           - "ListSecretVersionIds" → handle_list_secret_version_ids

        4. Add unit tests for each storage method.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - All 4 operations work through dispatcher
        - DescribeSecret returns full metadata with VersionIdsToStages
        - ListSecrets supports filtering by name, tag, description
        - ListSecrets pagination works with MaxResults + NextToken
        - UpdateSecret updates metadata and optionally creates new version
        - ListSecretVersionIds enumerates versions with IncludeDeprecated
        - All 208 existing tests pass
      </done>
    </task>

    <task id="2" type="test" complete="false">
      <name>Integration tests for metadata and discovery operations</name>
      <description>
        AWS SDK integration tests for DescribeSecret, ListSecrets, UpdateSecret,
        and ListSecretVersionIds.
      </description>

      <files>
        <modify>tests/secretsmanager_integration.rs — add integration tests</modify>
      </files>

      <action>
        Add to existing secretsmanager_integration.rs:

        - test_describe_secret: create secret with tags+description, describe → verify all fields
        - test_describe_secret_version_stages: create, put new value, describe → verify VersionIdsToStages has AWSCURRENT + AWSPREVIOUS
        - test_list_secrets: create 3 secrets, list → all 3 returned
        - test_list_secrets_pagination: create 5 secrets, list with max_results=2, paginate through all
        - test_list_secrets_filter_by_name: create 3, filter by name substring → correct subset
        - test_update_secret_metadata: create, update description, describe → new description
        - test_update_secret_value: create, update with new value, get → new value
        - test_list_secret_version_ids: create, put 2 more values, list versions → 3 versions
      </action>

      <verification>
        <command>cargo test --test secretsmanager_integration</command>
        <command>cargo test</command>
      </verification>

      <done>
        - All new integration tests pass with real aws-sdk-secretsmanager
        - All 208 existing tests still pass
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
    <criterion>All 4 operations work via AWS SDK</criterion>
    <criterion>ListSecrets filtering and pagination work</criterion>
    <criterion>All 208+ tests pass</criterion>
  </completion_criteria>
</plan>
