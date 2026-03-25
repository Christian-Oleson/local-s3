<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: 1 -->

<plan>
  <metadata>
    <phase>1</phase>
    <name>Foundation</name>
    <goal>Establish Rust project structure, HTTP server, filesystem storage layer, and basic bucket operations</goal>
    <deliverable>A running server that can create/delete/list buckets and return proper S3 XML responses</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>None — greenfield Rust project</dependencies>
    <affected_areas>Entire codebase (new project)</affected_areas>
    <patterns_to_follow>
      - Rust 2024 edition, async/await throughout
      - axum for HTTP with tokio runtime
      - quick-xml with serde for S3 XML format
      - thiserror for error types mapping to S3 error codes
      - Filesystem storage: buckets = directories, metadata = sidecar .meta.json files
      - Path-style URL routing: /{bucket} and /{bucket}/{key...}
      - All responses include x-amz-request-id, x-amz-id-2, Server headers
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="setup" complete="false">
      <name>Project scaffold, storage engine, and core types</name>
      <description>
        Initialize the Rust project with all dependencies, define the core domain types
        (S3 errors, XML request/response structs, storage traits), and implement the
        filesystem-backed storage engine that manages buckets as directories with metadata.
      </description>

      <files>
        <create>Cargo.toml</create>
        <create>src/main.rs              — entrypoint, tokio::main, placeholder server start</create>
        <create>src/lib.rs               — module declarations</create>
        <create>src/storage/mod.rs       — storage module, Storage trait</create>
        <create>src/storage/filesystem.rs — FileSystemStorage implementation</create>
        <create>src/error.rs             — S3Error enum with thiserror, HTTP status mapping</create>
        <create>src/types/mod.rs         — types module</create>
        <create>src/types/bucket.rs      — Bucket struct, BucketMetadata</create>
        <create>src/types/xml.rs         — S3 XML request/response structs with serde + quick-xml</create>
        <create>src/types/headers.rs     — S3 common header constants and helpers</create>
      </files>

      <action>
        1. Create Cargo.toml with these dependencies:
           - axum = "0.8" (with "macros" feature)
           - tokio = { version = "1", features = ["full"] }
           - quick-xml = { version = "0.37", features = ["serde", "serialize"] }
           - serde = { version = "1", features = ["derive"] }
           - md-5 = "0.10" (for ETag generation — note: crate name is md-5, not md5)
           - chrono = { version = "0.4", features = ["serde"] }
           - uuid = { version = "1", features = ["v4"] }
           - thiserror = "2"
           - tower-http = { version = "0.6", features = ["trace"] }
           - tracing = "0.1"
           - tracing-subscriber = { version = "0.3", features = ["env-filter"] }
           - hex = "0.4"
           Set edition = "2024", name = "local-s3".

        2. Define S3Error enum in error.rs with variants:
           - NoSuchBucket { bucket_name: String }
           - NoSuchKey { key: String }
           - BucketAlreadyOwnedByYou { bucket_name: String }
           - BucketAlreadyExists { bucket_name: String }
           - BucketNotEmpty { bucket_name: String }
           - InvalidBucketName { bucket_name: String }
           - InternalError { message: String }
           Each variant must map to:
           - An S3 error code string (e.g., "NoSuchBucket")
           - An HTTP status code (404, 409, 400, 500)
           Implement Into&lt;axum::response::Response&gt; that returns XML:
           &lt;Error&gt;&lt;Code&gt;...&lt;/Code&gt;&lt;Message&gt;...&lt;/Message&gt;&lt;Resource&gt;...&lt;/Resource&gt;&lt;RequestId&gt;...&lt;/RequestId&gt;&lt;/Error&gt;

        3. Define XML types in types/xml.rs using serde + quick-xml derives:
           - ListAllMyBucketsResult { owner: Owner, buckets: Vec&lt;BucketEntry&gt; }
           - BucketEntry { name: String, creation_date: String }
           - Owner { id: String, display_name: String }
           - CreateBucketConfiguration { location_constraint: Option&lt;String&gt; }
           - LocationConstraint (wrapper for GetBucketLocation response)
           - ErrorResponse { code: String, message: String, resource: String, request_id: String }
           All XML root elements must use the correct S3 namespace: "http://s3.amazonaws.com/doc/2006-03-01/"
           Use #[serde(rename = "...")] to match exact S3 element names (PascalCase).

        4. Define Bucket struct in types/bucket.rs:
           - name: String
           - creation_date: chrono::DateTime&lt;Utc&gt;
           - region: String (default "us-east-1")

        5. Define headers.rs with constants:
           - X_AMZ_REQUEST_ID = "x-amz-request-id"
           - X_AMZ_ID_2 = "x-amz-id-2"
           - SERVER_HEADER_VALUE = "local-s3"
           - Helper fn generate_request_id() -> String using uuid::Uuid::new_v4()
           - Helper fn s3_headers(request_id: &amp;str) -> HeaderMap that builds the common header set

        6. Implement FileSystemStorage in storage/filesystem.rs:
           - new(root_dir: PathBuf) -> Self — creates root dir if not exists
           - create_bucket(name: &amp;str, region: &amp;str) -> Result&lt;Bucket&gt;
             Creates directory at {root}/{bucket_name}/
             Writes .bucket-metadata.json with Bucket struct
             Returns BucketAlreadyOwnedByYou if dir exists
           - delete_bucket(name: &amp;str) -> Result&lt;()&gt;
             Checks bucket exists (NoSuchBucket if not)
             Checks bucket is empty — only .bucket-metadata.json allowed (BucketNotEmpty if objects exist)
             Removes directory
           - list_buckets() -> Result&lt;Vec&lt;Bucket&gt;&gt;
             Reads all subdirectories of root, loads each .bucket-metadata.json
           - head_bucket(name: &amp;str) -> Result&lt;()&gt;
             Returns Ok if bucket dir exists, NoSuchBucket otherwise
           - get_bucket_location(name: &amp;str) -> Result&lt;String&gt;
             Returns region from .bucket-metadata.json
           - bucket_exists(name: &amp;str) -> bool
             Quick path check
           All file I/O must use tokio::fs (async). The trait itself should be async.

        7. In main.rs, just set up basic tracing_subscriber init and a placeholder
           "server starting" message. The actual axum server comes in Task 2.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo fmt -- --check</command>
        <command>cargo clippy -- -D warnings</command>
        <manual>Verify project structure exists with all files listed above</manual>
      </verification>

      <done>
        - cargo build succeeds with zero errors
        - cargo clippy passes with no warnings
        - All types compile with correct serde/quick-xml derives
        - FileSystemStorage compiles with all 5 bucket methods
        - S3Error converts to proper XML error responses
      </done>
    </task>

    <task id="2" type="backend" complete="false">
      <name>HTTP server, routing, and bucket operation handlers</name>
      <description>
        Wire up the axum HTTP server with path-style URL routing and implement all 5 bucket
        operation handlers (CreateBucket, DeleteBucket, ListBuckets, HeadBucket, GetBucketLocation)
        that call through to the storage engine and return proper S3 XML responses with correct headers.
      </description>

      <files>
        <create>src/server.rs           — axum server setup, shared state, router construction</create>
        <create>src/routes/mod.rs       — routes module</create>
        <create>src/routes/bucket.rs    — bucket operation handlers</create>
        <create>src/middleware.rs        — S3 common response headers middleware</create>
        <modify>src/main.rs             — wire up server with CLI args (port, data-dir)</modify>
        <modify>src/lib.rs              — add new modules</modify>
      </files>

      <action>
        1. Create server.rs:
           - Define AppState struct holding Arc&lt;FileSystemStorage&gt;
           - Fn build_router(state: AppState) -> Router that defines all routes
           - Fn run_server(port: u16, data_dir: PathBuf) -> Result&lt;()&gt;
             Binds to 0.0.0.0:{port}, serves the router

        2. Create middleware.rs:
           - An axum middleware layer (or tower Layer) that adds these headers to EVERY response:
             x-amz-request-id: {uuid}
             x-amz-id-2: {base64-ish random}
             Server: local-s3
           - This ensures all responses (success AND error) have S3-standard headers

        3. Create routes/bucket.rs with these handlers:

           a) PUT /{bucket} → create_bucket
              - Parse optional XML body for CreateBucketConfiguration (region)
              - Default region to "us-east-1" if no body or no LocationConstraint
              - Call storage.create_bucket()
              - Return 200 with Location header: "/{bucket_name}"
              - On conflict: return S3Error::BucketAlreadyOwnedByYou

           b) DELETE /{bucket} → delete_bucket
              - Call storage.delete_bucket()
              - Return 204 No Content on success
              - On not found: return S3Error::NoSuchBucket
              - On not empty: return S3Error::BucketNotEmpty

           c) GET / → list_buckets
              - Call storage.list_buckets()
              - Serialize to ListAllMyBucketsResult XML
              - Return 200 with Content-Type: application/xml
              - Owner id and display_name can be hardcoded dummy values

           d) HEAD /{bucket} → head_bucket
              - Call storage.head_bucket()
              - Return 200 with x-amz-bucket-region header
              - On not found: return 404 (no body for HEAD)

           e) GET /{bucket}?location → get_bucket_location
              - Detect ?location query param
              - Call storage.get_bucket_location()
              - Return XML: &lt;LocationConstraint&gt;{region}&lt;/LocationConstraint&gt;
              - If region is "us-east-1", return empty LocationConstraint (S3 behavior)

        4. Set up the router in server.rs:
           Route structure for path-style S3:
           - GET  /                    → list_buckets
           - PUT  /:bucket             → create_bucket
           - DELETE /:bucket           → delete_bucket
           - HEAD /:bucket             → head_bucket
           - GET  /:bucket             → needs to disambiguate:
               if ?location param present → get_bucket_location
               else → list_objects (Phase 2, return empty for now)

           IMPORTANT: The /:bucket GET route must check query params to dispatch correctly.
           Use axum's Query extractor or manually inspect the query string.
           For Phase 1, any GET /:bucket without ?location returns 200 with an empty
           ListBucketResult (this will be replaced in Phase 2).

        5. Update main.rs:
           - Parse CLI args: --port (default 4566), --data-dir (default "./data")
           - Use std::env::args() or clap for argument parsing. For Phase 1,
             just use simple env var or hardcoded defaults — clap can come in Phase 5.
           - Call run_server(port, data_dir).await

        6. Ensure all XML responses set Content-Type: application/xml header.
           S3 uses application/xml, NOT text/xml.

        7. Handle the AWS SDK's Authorization header gracefully:
           The AWS SDK will send Authorization, x-amz-date, x-amz-content-sha256 headers.
           Simply IGNORE these — do not reject requests that have them, and do not validate them.
           This is critical for SDK compatibility.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo fmt -- --check</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo run -- --port 4566 --data-dir ./test-data &amp;</command>
        <manual>
          Test with curl:
          1. curl -X PUT http://localhost:4566/test-bucket → 200
          2. curl http://localhost:4566 → XML with test-bucket listed
          3. curl -I http://localhost:4566/test-bucket → 200 with x-amz-bucket-region
          4. curl "http://localhost:4566/test-bucket?location" → LocationConstraint XML
          5. curl -X DELETE http://localhost:4566/test-bucket → 204
          6. curl http://localhost:4566 → empty bucket list
          7. All responses have x-amz-request-id header
        </manual>
      </verification>

      <done>
        - Server starts on configured port
        - All 5 bucket operations work via curl with proper XML responses
        - Error responses return correct S3 XML format with correct HTTP status codes
        - Every response includes x-amz-request-id, x-amz-id-2, Server headers
        - AWS SDK auth headers are silently ignored (not rejected)
        - Content-Type: application/xml on all XML responses
      </done>
    </task>

    <task id="3" type="test" complete="false">
      <name>Unit tests and AWS SDK integration tests for bucket operations</name>
      <description>
        Write unit tests for the storage engine and XML serialization, plus integration tests
        that use the official aws-sdk-rust to verify bucket operations work exactly as a real
        S3 client expects.
      </description>

      <files>
        <create>tests/integration/mod.rs       — test module setup</create>
        <create>tests/integration/bucket.rs     — bucket operation integration tests</create>
        <create>tests/integration/helpers.rs    — shared test utilities (start server, create client)</create>
        <modify>Cargo.toml                      — add dev-dependencies for aws-sdk-s3</modify>
      </files>

      <action>
        1. Add dev-dependencies to Cargo.toml:
           - aws-config = "1"
           - aws-sdk-s3 = "1"
           - aws-credential-types = "1"
           - tempfile = "3"   (for isolated test data dirs)
           - tokio = { version = "1", features = ["full", "test-util"] }

        2. Create tests/integration/helpers.rs:
           - Fn start_test_server() -> (u16, tempfile::TempDir)
             Picks a random available port (bind to port 0, get assigned port)
             Creates a TempDir for isolated storage
             Spawns the server in a background tokio task
             Returns the port and temp dir handle
           - Fn create_s3_client(port: u16) -> aws_sdk_s3::Client
             Configures aws-sdk-s3 client with:
               endpoint_url: http://localhost:{port}
               region: us-east-1
               credentials: dummy static credentials (access_key: "test", secret: "test")
               force_path_style: true
             IMPORTANT: The client MUST use path-style addressing, not virtual-hosted.

        3. Create unit tests (in src/ files as #[cfg(test)] mod tests):

           In storage/filesystem.rs:
           - test_create_bucket: creates bucket, verifies directory + metadata file exist
           - test_create_bucket_duplicate: second create returns BucketAlreadyOwnedByYou
           - test_delete_bucket: creates then deletes, verifies directory removed
           - test_delete_nonexistent_bucket: returns NoSuchBucket
           - test_list_buckets_empty: returns empty vec
           - test_list_buckets_multiple: creates 3 buckets, lists all 3
           - test_head_bucket_exists: returns Ok
           - test_head_bucket_missing: returns NoSuchBucket
           - test_get_bucket_location: returns configured region

           In types/xml.rs:
           - test_serialize_list_buckets_result: verify XML output matches S3 format
           - test_serialize_error_response: verify error XML format
           - test_deserialize_create_bucket_config: parse LocationConstraint from XML
           - test_serialize_location_constraint: verify GetBucketLocation response format

        4. Create integration tests in tests/integration/bucket.rs:
           Each test starts its own server on a random port with isolated temp storage.

           - test_create_and_list_bucket:
             Create bucket "test-bucket" via SDK
             List buckets via SDK
             Assert "test-bucket" appears in list with valid creation date

           - test_create_bucket_with_region:
             Create bucket with LocationConstraint "us-west-2"
             Get bucket location via SDK
             Assert returns "us-west-2"

           - test_create_duplicate_bucket:
             Create bucket "dup-bucket"
             Create "dup-bucket" again
             Assert error is BucketAlreadyOwnedByYou (or BucketAlreadyExists)

           - test_delete_bucket:
             Create bucket, verify it exists (head_bucket)
             Delete bucket
             Verify it's gone (list_buckets returns empty, head_bucket returns 404)

           - test_delete_nonexistent_bucket:
             Delete bucket that doesn't exist
             Assert NoSuchBucket error

           - test_head_bucket:
             Create bucket
             Head bucket → succeeds
             Head nonexistent → 404

           - test_list_buckets_empty:
             Fresh server, list buckets → empty list

           - test_create_multiple_buckets:
             Create 5 buckets with different names
             List all → all 5 present
             Delete 2
             List → 3 remaining

        5. All integration tests must clean up after themselves (TempDir handles this).
           Use #[tokio::test] for async tests.
      </action>

      <verification>
        <command>cargo test --lib</command>
        <command>cargo test --test integration</command>
        <command>cargo test</command>
        <command>cargo fmt -- --check</command>
        <command>cargo clippy -- -D warnings</command>
      </verification>

      <done>
        - All unit tests pass (storage engine + XML serialization)
        - All integration tests pass using real aws-sdk-s3 client
        - Tests are isolated (random ports, temp directories)
        - cargo clippy and fmt pass on test code too
        - No test relies on external state or ordering
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
      Start server with: cargo run -- --port 4566 --data-dir ./data
      Create bucket:     curl -X PUT http://localhost:4566/my-bucket
      List buckets:      curl http://localhost:4566
      Head bucket:       curl -I http://localhost:4566/my-bucket
      Get location:      curl "http://localhost:4566/my-bucket?location"
      Delete bucket:     curl -X DELETE http://localhost:4566/my-bucket
      Verify all responses have XML format and x-amz-request-id header.
    </manual>
  </phase_verification>

  <completion_criteria>
    <criterion>All 3 tasks marked complete</criterion>
    <criterion>cargo build, test, fmt, clippy all pass</criterion>
    <criterion>Bucket CRUD works with real AWS SDK client (integration tests prove it)</criterion>
    <criterion>S3 XML responses match expected format</criterion>
    <criterion>All responses include standard S3 headers</criterion>
    <criterion>No TODO comments left in new code</criterion>
  </completion_criteria>
</plan>
