<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: SM-1 (Secrets Manager Phase 1) -->

<plan>
  <metadata>
    <phase>SM-1</phase>
    <name>Foundation + Core CRUD</name>
    <goal>JSON protocol dispatcher, service multiplexing, and core secret CRUD operations</goal>
    <deliverable>Developers can create, read, update, delete secrets via any AWS SDK on port 4566</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>S3 service complete (189 tests, all passing). Existing server.rs router, AppState, FileSystemStorage.</dependencies>
    <affected_areas>
      - src/secretsmanager/ — entirely new module (dispatcher, handlers, storage, types, error)
      - src/server.rs — service multiplexer middleware to intercept X-Amz-Target requests
      - src/lib.rs — add secretsmanager module
      - Cargo.toml — add aws-sdk-secretsmanager dev-dependency for integration tests
      - tests/ — new secretsmanager integration test file
    </affected_areas>
    <patterns_to_follow>
      - AWS JSON 1.1: all requests POST /, dispatch via X-Amz-Target header
      - JSON errors: {"__type": "ErrorCode", "Message": "..."}, HTTP 400 for most client errors
      - Storage at {data-dir}/.secrets-manager/secrets/{name}/ with metadata.json + versions/{id}.json
      - ARN format: arn:aws:secretsmanager:{region}:{account}:secret:{name}-{6-random-chars}
      - Account ID: 000000000000, Region: us-east-1 (hardcoded defaults)
      - Timestamps: epoch floats (seconds with millisecond precision)
      - AWSCURRENT label on exactly one version per secret
      - AWSCURRENT/AWSPREVIOUS rotation on PutSecretValue
      - Multiplexing: if X-Amz-Target starts with "secretsmanager." → SM handler, else → S3
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>Service multiplexer + Secrets Manager module scaffold + storage engine</name>
      <description>
        Create the secretsmanager module with storage engine, types, and error handling.
        Add service multiplexing middleware to server.rs that intercepts requests with
        X-Amz-Target: secretsmanager.* and routes them to the new module.
        Implement the filesystem-backed secrets storage engine with versioning.
      </description>

      <files>
        <create>src/secretsmanager/mod.rs       — module declarations</create>
        <create>src/secretsmanager/error.rs      — SecretsManagerError enum with JSON serialization</create>
        <create>src/secretsmanager/types.rs      — request/response JSON structs (serde)</create>
        <create>src/secretsmanager/storage.rs    — SecretsStorage: filesystem-backed secret + version storage</create>
        <create>src/secretsmanager/dispatcher.rs — X-Amz-Target → handler routing</create>
        <modify>src/server.rs                    — add multiplexer middleware, pass data_dir to SecretsStorage</modify>
        <modify>src/lib.rs                       — add secretsmanager module</modify>
      </files>

      <action>
        1. Create src/secretsmanager/error.rs — SecretsManagerError enum:

           Variants (all return HTTP 400 except InternalServiceError which returns 500):
           - ResourceNotFoundException { message: String }
           - ResourceExistsException { message: String }
           - InvalidParameterException { message: String }
           - InvalidRequestException { message: String }
           - InternalServiceError { message: String }

           Implement a method to serialize to JSON error format:
           ```json
           {"__type": "ResourceNotFoundException", "Message": "..."}
           ```

           Implement IntoResponse for axum that returns the JSON with correct HTTP status
           and Content-Type: application/x-amz-json-1.1.

        2. Create src/secretsmanager/types.rs — JSON request/response structs:

           SecretMetadata (stored in metadata.json):
           - name: String
           - arn: String
           - description: Option&lt;String&gt;
           - kms_key_id: Option&lt;String&gt;
           - tags: Vec&lt;Tag&gt;
           - created_date: f64 (epoch)
           - last_changed_date: f64
           - last_accessed_date: Option&lt;f64&gt;
           - deleted_date: Option&lt;f64&gt; (set when scheduled for deletion)
           - rotation_enabled: bool
           - rotation_lambda_arn: Option&lt;String&gt;
           - rotation_rules: Option&lt;RotationRules&gt;

           SecretVersion (stored in versions/{id}.json):
           - version_id: String
           - secret_string: Option&lt;String&gt;
           - secret_binary: Option&lt;String&gt; (base64)
           - version_stages: Vec&lt;String&gt;
           - created_date: f64

           Tag: { key: String, value: String }

           Use #[serde(rename = "PascalCase")] for AWS JSON field names where needed.
           Define separate request/response structs for each operation (CreateSecretRequest,
           CreateSecretResponse, GetSecretValueRequest, etc.).

        3. Create src/secretsmanager/storage.rs — SecretsStorage:

           ```rust
           pub struct SecretsStorage {
               root_dir: PathBuf, // {data-dir}/.secrets-manager
               region: String,    // default "us-east-1"
               account_id: String, // default "000000000000"
           }
           ```

           Helper methods:
           - secret_dir(name) → root_dir/secrets/{url_encoded_name}/
           - metadata_path(name) → secret_dir/metadata.json
           - version_path(name, version_id) → secret_dir/versions/{version_id}.json
           - generate_arn(name) → arn:aws:secretsmanager:{region}:{account}:secret:{name}-{6-random}
           - resolve_secret_id(id) → finds secret by name OR by ARN
           - epoch_now() → current time as f64

           Core methods:
           a) create_secret(name, secret_string, secret_binary, description, kms_key_id, tags, client_token)
              → Result&lt;(String, String, String)&gt; (arn, name, version_id)
              - Check if secret exists → ResourceExistsException
              - Generate ARN with 6-char random suffix
              - Generate version_id from client_token or new UUID
              - Create metadata.json
              - Create initial version with AWSCURRENT label
              - Cannot have both secret_string and secret_binary

           b) get_secret_value(secret_id, version_id, version_stage)
              → Result&lt;SecretVersion + metadata&gt;
              - Resolve secret_id to name
              - Check not deleted → InvalidRequestException
              - If version_id: find that specific version
              - If version_stage: find version with that label (default AWSCURRENT)
              - If both: verify they match

           c) put_secret_value(secret_id, secret_string, secret_binary, client_token, version_stages)
              → Result&lt;(arn, name, version_id, stages)&gt;
              - Resolve secret_id
              - Create new version with given stages (default [AWSCURRENT])
              - Move AWSCURRENT from old version: old gets AWSPREVIOUS
              - Previous AWSPREVIOUS version loses that label
              - Idempotency: if client_token matches existing version, return existing

           d) delete_secret(secret_id, recovery_window_days, force_delete)
              → Result&lt;(arn, name, deletion_date)&gt;
              - If force_delete: remove everything immediately
              - If recovery_window: set deleted_date in metadata, block GetSecretValue
              - RecoveryWindowInDays and ForceDeleteWithoutRecovery are mutually exclusive

           e) restore_secret(secret_id) → Result&lt;(arn, name)&gt;
              - Clear deleted_date from metadata

           f) list_secrets_raw() → list all secret metadata (for ListSecrets handler later)

        4. Create src/secretsmanager/dispatcher.rs:

           A single async handler function:
           ```rust
           pub async fn dispatch(
               state: &amp;AppState,
               target: &amp;str,    // the operation name after "secretsmanager."
               body: Bytes,
           ) -> Response
           ```

           Match on target:
           - "CreateSecret" → parse CreateSecretRequest, call storage, return CreateSecretResponse
           - "GetSecretValue" → parse, call, return
           - "PutSecretValue" → parse, call, return
           - "DeleteSecret" → parse, call, return
           - "RestoreSecret" → parse, call, return
           - _ → return InvalidParameterException("Unknown operation")

           Each handler: deserialize JSON → call storage → serialize JSON response
           with Content-Type: application/x-amz-json-1.1

        5. Modify src/server.rs — add service multiplexer:

           Add a middleware layer that runs BEFORE the S3 routes. Check for:
           - `X-Amz-Target` header present AND starts with "secretsmanager."
           If matched:
           - Extract the operation name (part after "secretsmanager.")
           - Read the request body
           - Call secretsmanager::dispatcher::dispatch()
           - Return the response (bypass S3 routes entirely)

           Implementation: use axum middleware::from_fn that intercepts matching requests.
           The middleware must consume the body for SM requests but pass through for S3.

           Alternative simpler approach: add a POST "/" route that checks the header and
           dispatches. But this conflicts with the existing GET "/" (list_buckets). Since
           SM only uses POST and S3's "/" only uses GET, this should work:
           ```rust
           .route("/", get(bucket::list_buckets).post(secretsmanager_handler))
           ```

           The secretsmanager_handler checks X-Amz-Target, dispatches if SM, or returns 404.

        6. Update src/lib.rs to add: pub mod secretsmanager;

        7. Update AppState in server.rs to hold Arc&lt;SecretsStorage&gt; alongside Arc&lt;FileSystemStorage&gt;.
           Initialize SecretsStorage with data_dir in run_server.

        8. Add unit tests for SecretsStorage methods.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - secretsmanager module compiles with storage, types, error, dispatcher
        - Service multiplexer routes X-Amz-Target: secretsmanager.* to dispatcher
        - SecretsStorage creates/reads/deletes secrets with versioning
        - AWSCURRENT/AWSPREVIOUS rotation works on PutSecretValue
        - JSON error format with __type field
        - All 189 existing S3 tests still pass
        - Unit tests for storage methods pass
      </done>
    </task>

    <task id="2" type="backend" complete="false">
      <name>CreateSecret, GetSecretValue, PutSecretValue, DeleteSecret, RestoreSecret handlers</name>
      <description>
        Wire up the dispatcher to call all 5 core operation handlers end-to-end.
        Ensure JSON request parsing and response serialization matches AWS SDK expectations.
        Test with curl to verify the full HTTP flow works.
      </description>

      <files>
        <modify>src/secretsmanager/dispatcher.rs — complete handler implementations</modify>
        <modify>src/secretsmanager/types.rs      — refine request/response types to match AWS SDK</modify>
        <modify>src/secretsmanager/storage.rs    — fix any issues found during end-to-end testing</modify>
      </files>

      <action>
        This task may be largely complete from Task 1 if the dispatcher was fully wired.
        Focus on:

        1. Verify each operation's JSON request/response format matches the AWS SDK exactly.
           The AWS SDK for Secrets Manager uses PascalCase JSON field names:
           - Request: { "Name": "...", "SecretString": "...", "Tags": [...] }
           - Response: { "ARN": "...", "Name": "...", "VersionId": "..." }

           Use serde rename attributes:
           ```rust
           #[serde(rename = "Name")]
           pub name: String,
           #[serde(rename = "SecretString", skip_serializing_if = "Option::is_none")]
           pub secret_string: Option&lt;String&gt;,
           ```

        2. Handle SecretId resolution — can be name or ARN:
           - If input contains "arn:aws:secretsmanager:" → match by ARN
           - Else → match by name

        3. Deletion states:
           - Scheduled: metadata has deleted_date set, GetSecretValue returns InvalidRequestException
           - Force-deleted: directory removed entirely
           - Restored: deleted_date cleared

        4. Idempotency:
           - CreateSecret with ClientRequestToken: if token matches existing version, return success
           - PutSecretValue with ClientRequestToken: if token matches existing version, no-op

        5. Test with curl:
           ```bash
           # CreateSecret
           curl -X POST http://localhost:4566/ \
             -H "X-Amz-Target: secretsmanager.CreateSecret" \
             -H "Content-Type: application/x-amz-json-1.1" \
             -d '{"Name":"test-secret","SecretString":"my-password"}'

           # GetSecretValue
           curl -X POST http://localhost:4566/ \
             -H "X-Amz-Target: secretsmanager.GetSecretValue" \
             -H "Content-Type: application/x-amz-json-1.1" \
             -d '{"SecretId":"test-secret"}'
           ```

        6. Ensure all 189 S3 tests still pass — the multiplexer must not break S3.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test</command>
        <command>cargo clippy -- -D warnings</command>
        <manual>
          curl tests for all 5 operations:
          1. CreateSecret → returns ARN, Name, VersionId
          2. GetSecretValue → returns SecretString + metadata
          3. PutSecretValue → returns new VersionId, old version becomes AWSPREVIOUS
          4. DeleteSecret with ForceDeleteWithoutRecovery → secret gone
          5. Create + Delete (with window) + RestoreSecret → secret accessible again
        </manual>
      </verification>

      <done>
        - All 5 operations work end-to-end via HTTP
        - JSON field names match AWS SDK expectations (PascalCase)
        - SecretId resolution works for both name and ARN
        - Deletion states (scheduled, forced, restored) work correctly
        - All 189 S3 tests still pass
      </done>
    </task>

    <task id="3" type="test" complete="false">
      <name>AWS SDK integration tests for Secrets Manager core operations</name>
      <description>
        Integration tests using aws-sdk-secretsmanager that verify all 5 core operations
        work correctly through the real AWS SDK client.
      </description>

      <files>
        <create>tests/secretsmanager_integration.rs — integration tests</create>
        <modify>Cargo.toml — add aws-sdk-secretsmanager to dev-dependencies</modify>
      </files>

      <action>
        1. Add to Cargo.toml dev-dependencies:
           aws-sdk-secretsmanager = "1"

        2. Create tests/secretsmanager_integration.rs with TestServer pattern
           (same as existing tests: TcpListener:0, set_nonblocking, force_path_style client).

           Create SM client:
           ```rust
           let sm_config = aws_sdk_secretsmanager::Config::builder()
               .behavior_version(BehaviorVersion::latest())
               .region(Region::new("us-east-1"))
               .credentials_provider(creds)
               .endpoint_url(format!("http://127.0.0.1:{port}"))
               .retry_config(RetryConfig::disabled())
               .build();
           let sm_client = aws_sdk_secretsmanager::Client::from_conf(sm_config);
           ```

        3. Tests:
           - test_create_and_get_secret: create, get → verify SecretString matches
           - test_create_secret_with_tags: create with tags, verify (via future DescribeSecret or re-get)
           - test_put_secret_value_new_version: create, put new value, get → verify new value
           - test_get_secret_by_version_stage: create, put, get AWSPREVIOUS → old value
           - test_delete_secret_force: create, force delete, get → error
           - test_delete_and_restore_secret: create, delete with window, restore, get → works
           - test_create_duplicate_secret: create same name twice → ResourceExistsException
           - test_get_nonexistent_secret: get missing → ResourceNotFoundException
           - test_s3_still_works: create S3 bucket + put object → verify S3 unaffected

        4. All existing 189 S3 tests must still pass alongside new SM tests.
      </action>

      <verification>
        <command>cargo test --test secretsmanager_integration</command>
        <command>cargo test</command>
        <command>cargo clippy -- -D warnings</command>
      </verification>

      <done>
        - All SM integration tests pass with real aws-sdk-secretsmanager client
        - All 189 S3 tests still pass
        - S3 and Secrets Manager coexist on same port
        - Significant test count increase
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
    <manual>
      1. Start server: cargo run -- --port 4566 --data-dir ./data
      2. Create secret via curl with X-Amz-Target header
      3. Get secret value via curl → verify correct JSON response
      4. S3 operations still work: curl PUT/GET bucket/object
      5. Both services on same port, zero conflicts
    </manual>
  </phase_verification>

  <completion_criteria>
    <criterion>All 3 tasks marked complete</criterion>
    <criterion>All verification commands pass</criterion>
    <criterion>5 core SM operations work via AWS SDK</criterion>
    <criterion>S3 and SM coexist on port 4566</criterion>
    <criterion>All 189 S3 tests still pass</criterion>
    <criterion>No TODO comments left in new code</criterion>
  </completion_criteria>
</plan>
