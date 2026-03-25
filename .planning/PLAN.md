<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: 4 -->

<plan>
  <metadata>
    <phase>4</phase>
    <name>Versioning &amp; Configuration Storage</name>
    <goal>Bucket versioning support and storage of bucket configurations (policies, ACLs, lifecycle) without enforcement</goal>
    <deliverable>Apps that use versioning work correctly; bucket configs are accepted and stored</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>Phase 3 complete — multipart, range, conditional, tagging, CORS, 133 tests</dependencies>
    <affected_areas>
      - src/storage/filesystem.rs — versioning state, versioned object storage, config storage
      - src/routes/object.rs — ?versionId on GET/HEAD/DELETE, version ID in PUT response
      - src/routes/bucket.rs — versioning/policy/ACL/lifecycle config handlers
      - src/server.rs — virtual-hosted-style routing (optional)
      - src/types/xml.rs — versioning, ListObjectVersions, policy, ACL, lifecycle XML types
      - src/types/object.rs — add version_id field to ObjectMetadata
      - src/error.rs — new error variants
    </affected_areas>
    <patterns_to_follow>
      - Versioning config at {bucket}/.versioning.json
      - When versioning enabled: each PUT generates UUID version_id, old versions preserved
      - Versions stored at {bucket}/.versions/{key}/{version_id}.data + {version_id}.meta.json
      - Delete on versioned bucket creates delete marker (version with is_delete_marker=true, no data)
      - DELETE with ?versionId permanently removes that specific version
      - GET/HEAD with ?versionId retrieves specific version
      - Config files: {bucket}/.policy.json, {bucket}/.acl.json, {bucket}/.lifecycle.json
      - Store-only configs: accept and return, no enforcement
      - Follow existing patterns: query param dispatch, xml_response(), S3Error
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>Bucket versioning and versioned object operations</name>
      <description>
        Implement PutBucketVersioning/GetBucketVersioning, versioned object storage with
        version IDs, ListObjectVersions, GET/HEAD/DELETE with ?versionId, and delete markers.
        This is the most complex task — it modifies the core object storage behavior.
      </description>

      <files>
        <modify>src/storage/filesystem.rs — versioning config, versioned put/get/head/delete, list versions</modify>
        <modify>src/routes/object.rs — ?versionId dispatch on GET/HEAD/DELETE, version headers on PUT</modify>
        <modify>src/routes/bucket.rs — versioning config handlers, ListObjectVersions handler</modify>
        <modify>src/types/xml.rs — versioning XML types, ListObjectVersions types</modify>
        <modify>src/types/object.rs — add version_id to ObjectMetadata</modify>
        <modify>src/error.rs — MethodNotAllowed for delete marker access</modify>
        <modify>src/server.rs — if any route changes needed</modify>
      </files>

      <action>
        1. Add to src/types/object.rs:
           - Add `version_id: Option&lt;String&gt;` field to ObjectMetadata (with serde default)
           - Add `is_delete_marker: bool` field (with serde default false)

        2. Add XML types to src/types/xml.rs:

           VersioningConfiguration (serialize + deserialize):
           - Status: Option&lt;String&gt; ("Enabled" or "Suspended")

           ListVersionsResult:
           - xmlns, Name, Prefix, MaxKeys, IsTruncated, KeyMarker, VersionIdMarker
           - Version entries: Vec with Key, VersionId, IsLatest, LastModified, ETag, Size
           - DeleteMarker entries: Vec with Key, VersionId, IsLatest, LastModified

        3. Storage layer — versioning config:
           - Store at {bucket}/.versioning.json: { "status": "Enabled" | "Suspended" }
           - put_bucket_versioning(bucket, status) → Result&lt;()&gt;
           - get_bucket_versioning(bucket) → Result&lt;Option&lt;String&gt;&gt; (None if never set)
           - is_versioning_enabled(bucket) → bool
           - Update is_internal_entry to match ".versioning.json" and ".versions"

        4. Storage layer — versioned object storage:

           When versioning is ENABLED:

           a) put_object: Generate UUID version_id. Store object normally at {bucket}/{key}
              AND save a copy at {bucket}/.versions/{key}/{version_id}.data with metadata
              at {bucket}/.versions/{key}/{version_id}.meta.json.
              Set version_id on the returned ObjectMetadata.
              The "current" object always lives at the normal path.

           b) get_object: If no ?versionId, return current object (same as before).
              If ?versionId specified, read from .versions/{key}/{version_id}.

           c) head_object: Same logic — with or without ?versionId.

           d) delete_object (no versionId): Create a delete marker.
              - Generate a new version_id for the delete marker
              - Save a delete marker entry in .versions/{key}/{version_id}.meta.json
                with is_delete_marker=true and no data file
              - Remove the "current" object file and metadata (so GET without versionId returns 404)
              - Return the delete marker's version_id and x-amz-delete-marker: true header

           e) delete_object (with versionId): Permanently remove that specific version.
              - Delete {version_id}.data and {version_id}.meta.json from .versions/
              - If deleting the version that was "current", the previous version becomes current
                (or if it was a delete marker, just remove it)

           When versioning is DISABLED or SUSPENDED:
           - put_object: Same as current behavior (no version tracking)
              If suspended: still generate version_id "null" for new puts
           - delete_object: Same as current (actual delete, no marker)

        5. list_object_versions(bucket, prefix, max_keys, key_marker, version_id_marker):
           - Scan .versions/{key}/ directories for all version metadata
           - Include current version too
           - Sort by key, then by last_modified descending
           - Mark latest version with IsLatest=true
           - Separate Version entries from DeleteMarker entries

        6. HTTP handlers:

           In routes/bucket.rs:
           - Add `versioning` field to BucketGetQuery
           - Add `versioning` field to BucketPutQuery
           - GET /{bucket}?versioning → GetBucketVersioning
           - PUT /{bucket}?versioning → PutBucketVersioning (parse XML body)
           - GET /{bucket}?versions → ListObjectVersions
           - Add `versions` field to BucketGetQuery

           In routes/object.rs:
           - Add `versionId` field to ObjectQuery
           - Modify get_object_handler: pass versionId to storage if present
           - Modify head_object_handler: pass versionId
           - Modify delete_object_handler: pass versionId
           - PUT response: add x-amz-version-id header when versioning enabled

        7. Unit tests: versioning config, versioned put/get, delete markers, list versions
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - Versioning enable/suspend/get works
        - PUT with versioning enabled returns version_id
        - GET/HEAD with ?versionId retrieves specific version
        - DELETE on versioned bucket creates delete marker
        - DELETE with ?versionId permanently removes version
        - ListObjectVersions returns all versions and delete markers
        - Non-versioned behavior unchanged
        - All existing 133 tests pass
      </done>
    </task>

    <task id="2" type="backend" complete="false">
      <name>Config storage: bucket policy, ACL, lifecycle</name>
      <description>
        Implement store-only configuration APIs for bucket policies, ACLs, and lifecycle rules.
        These accept and return configurations but do not enforce them — matching
        LocalStack's behavior for local development.
      </description>

      <files>
        <modify>src/storage/filesystem.rs — policy, ACL, lifecycle storage methods</modify>
        <modify>src/routes/bucket.rs — config handler dispatch</modify>
        <modify>src/types/xml.rs — ACL XML types (policy is JSON, lifecycle is XML)</modify>
        <modify>src/error.rs — NoSuchBucketPolicy, etc.</modify>
      </files>

      <action>
        1. Bucket Policy (JSON, not XML):
           Storage: {bucket}/.policy.json (raw JSON string, not parsed)
           - put_bucket_policy(bucket, policy_json: String) → Result&lt;()&gt;
           - get_bucket_policy(bucket) → Result&lt;String&gt; (returns raw JSON)
           - delete_bucket_policy(bucket) → Result&lt;()&gt;

           HTTP: PUT/GET/DELETE /{bucket}?policy
           - PUT body is raw JSON (Content-Type: application/json), store as-is
           - GET returns raw JSON
           - DELETE removes the file
           Add NoSuchBucketPolicy error variant (404).

        2. Bucket ACL (XML):
           Storage: {bucket}/.acl.json
           - put_bucket_acl(bucket, acl_xml: String) → Result&lt;()&gt; (store raw XML as string)
           - get_bucket_acl(bucket) → Result&lt;String&gt;

           HTTP: PUT/GET /{bucket}?acl
           - PUT body is XML, store as-is
           - GET returns stored XML, or default ACL if not set
           Default ACL: full control for owner (standard S3 default)

        3. Object ACL (XML):
           Storage: {bucket}/.acls/{key}.json
           - put_object_acl(bucket, key, acl_xml: String) → Result&lt;()&gt;
           - get_object_acl(bucket, key) → Result&lt;String&gt;

           HTTP: PUT/GET /{bucket}/{key}?acl
           Add ?acl dispatch to object handlers.

        4. Lifecycle Configuration (XML):
           Storage: {bucket}/.lifecycle.json (store raw XML as string)
           - put_bucket_lifecycle(bucket, lifecycle_xml: String) → Result&lt;()&gt;
           - get_bucket_lifecycle(bucket) → Result&lt;String&gt;
           - delete_bucket_lifecycle(bucket) → Result&lt;()&gt;

           HTTP: PUT/GET/DELETE /{bucket}?lifecycle
           Add NoSuchLifecycleConfiguration error variant.

        5. Update is_internal_entry for: .policy.json, .acl.json, .acls, .lifecycle.json

        6. For all config storage: these are "accept and store" — no parsing, validation, or enforcement.
           This makes implementation simple: just read/write files.

        7. Add query param dispatch in routes/bucket.rs for policy, acl, lifecycle.
           Add acl dispatch in routes/object.rs.

        8. Unit tests for each config storage method.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
      </verification>

      <done>
        - Bucket policy CRUD works (raw JSON storage)
        - Bucket ACL put/get works (raw XML storage)
        - Object ACL put/get works
        - Lifecycle config CRUD works (raw XML storage)
        - All store-only — no enforcement
        - All existing tests pass
      </done>
    </task>

    <task id="3" type="test" complete="false">
      <name>Integration tests for versioning and config storage</name>
      <description>
        AWS SDK integration tests for versioning operations and config storage APIs.
      </description>

      <files>
        <create>tests/versioning_integration.rs — versioning tests</create>
        <modify>tests/object_integration.rs — add config storage tests if needed</modify>
      </files>

      <action>
        1. Versioning tests:
           - test_enable_versioning: enable, verify status
           - test_versioned_put: enable versioning, put same key twice, verify both versions exist
           - test_get_specific_version: put 2 versions, get each by versionId
           - test_delete_creates_marker: enable versioning, put, delete, verify 404 on GET
           - test_delete_with_version_id: permanently remove specific version
           - test_list_object_versions: put multiple versions, list, verify order

        2. Config storage tests:
           - test_put_get_bucket_policy: put JSON policy, get back
           - test_put_get_bucket_acl: put ACL, get back
           - test_put_get_lifecycle: put lifecycle config, get back
      </action>

      <verification>
        <command>cargo test</command>
      </verification>

      <done>
        - Versioning lifecycle fully tested via AWS SDK
        - Config storage round-trip tested
        - All tests passing
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
    <criterion>All 3 tasks marked complete</criterion>
    <criterion>All verification commands pass</criterion>
    <criterion>Versioning lifecycle works via AWS SDK</criterion>
    <criterion>Config storage round-trips via AWS SDK</criterion>
    <criterion>No TODO comments left in new code</criterion>
  </completion_criteria>
</plan>
