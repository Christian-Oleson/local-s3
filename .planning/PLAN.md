<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: 2 -->

<plan>
  <metadata>
    <phase>2</phase>
    <name>Core Object Operations</name>
    <goal>Full object CRUD with metadata, ETags, listing, and batch operations</goal>
    <deliverable>Developers can PUT, GET, DELETE, COPY, and LIST objects — enough to run most S3-dependent apps</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>Phase 1 complete — bucket CRUD, storage engine, XML types, router, AWS SDK tests</dependencies>
    <affected_areas>
      - src/storage/filesystem.rs — add all object methods
      - src/routes/ — new object.rs handler module
      - src/server.rs — add object routes with wildcard key paths
      - src/types/xml.rs — add ListObjectsV2/V1, DeleteResult, CopyResult XML types
      - src/types/ — new object.rs for ObjectMetadata struct
      - src/error.rs — already has NoSuchKey, may need new variants
      - tests/ — new object integration tests
    </affected_areas>
    <patterns_to_follow>
      - Filesystem layout: objects at {root}/{bucket}/{key}, metadata at {root}/{bucket}/.meta/{key}.json
      - Keys can contain "/" — requires creating intermediate directories
      - ETag = quoted hex MD5 of object content (e.g., "d41d8cd98f00b204e9800998ecf8427e")
      - Metadata sidecar JSON stores: content_type, content_length, etag, last_modified, custom x-amz-meta-* headers
      - .meta/ directory and .bucket-metadata.json are invisible to list/delete operations
      - Follow existing patterns: xml_response(), S3Error with IntoResponse, State(AppState) extractors
      - AWS SDK sends trailing slashes — routes must handle both /{bucket}/{key} and /{bucket}/{key}/
      - SigV4 headers (Authorization, x-amz-content-sha256, x-amz-date) silently ignored
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>Object storage engine, metadata, ETag, and basic CRUD handlers</name>
      <description>
        Extend FileSystemStorage with put_object, get_object, head_object, delete_object methods.
        Implement metadata sidecar files and ETag generation. Create HTTP handlers for PUT, GET,
        HEAD, DELETE on /{bucket}/{key+}. Wire up routes in the router.
      </description>

      <files>
        <create>src/types/object.rs      — ObjectMetadata struct with serde derives</create>
        <create>src/routes/object.rs     — HTTP handlers for object CRUD</create>
        <modify>src/storage/filesystem.rs — add object methods + update delete_bucket to allow .meta dir</modify>
        <modify>src/types/mod.rs         — add object module</modify>
        <modify>src/routes/mod.rs        — add object module</modify>
        <modify>src/server.rs            — add object routes with wildcard key capture</modify>
      </files>

      <action>
        1. Create src/types/object.rs — ObjectMetadata struct:
           ```
           struct ObjectMetadata {
               key: String,
               content_type: String,          // default "application/octet-stream"
               content_length: u64,
               etag: String,                  // quoted MD5 hex, e.g. "\"abc123\""
               last_modified: DateTime&lt;Utc&gt;,
               custom_metadata: HashMap&lt;String, String&gt;,  // x-amz-meta-* headers
               content_disposition: Option&lt;String&gt;,
               cache_control: Option&lt;String&gt;,
               content_encoding: Option&lt;String&gt;,
               expires: Option&lt;String&gt;,
           }
           ```
           Derive Serialize, Deserialize, Debug, Clone.

        2. Extend FileSystemStorage with helper methods:
           - fn object_path(&amp;self, bucket: &amp;str, key: &amp;str) -> PathBuf
             Returns {root}/{bucket}/{key}
           - fn object_metadata_path(&amp;self, bucket: &amp;str, key: &amp;str) -> PathBuf
             Returns {root}/{bucket}/.meta/{key}.json
           - fn ensure_parent_dirs(path: &amp;Path) — creates parent directories for nested keys
           - fn is_internal_entry(name: &amp;str) -> bool
             Returns true for ".bucket-metadata.json" and ".meta"

        3. Implement put_object(&amp;self, bucket: &amp;str, key: &amp;str, body: Bytes, metadata: ObjectMetadata) -> Result&lt;ObjectMetadata&gt;:
           - Verify bucket exists (NoSuchBucket if not)
           - Compute ETag: hex(MD5(body)), wrap in quotes
           - Write body to object_path(bucket, key) — create parent dirs first
           - Write metadata to object_metadata_path(bucket, key) — create parent dirs first
           - Return the metadata with computed ETag and content_length

        4. Implement get_object(&amp;self, bucket: &amp;str, key: &amp;str) -> Result&lt;(ObjectMetadata, Vec&lt;u8&gt;)&gt;:
           - Verify bucket exists
           - Read metadata from sidecar (NoSuchKey if not found)
           - Read object body from object_path
           - Return (metadata, body)

        5. Implement head_object(&amp;self, bucket: &amp;str, key: &amp;str) -> Result&lt;ObjectMetadata&gt;:
           - Verify bucket exists
           - Read and return metadata only (NoSuchKey if not found)

        6. Implement delete_object(&amp;self, bucket: &amp;str, key: &amp;str) -> Result&lt;()&gt;:
           - Verify bucket exists
           - Remove object file if it exists (S3 returns 204 even for non-existent keys — NO error)
           - Remove metadata sidecar if it exists
           - Clean up empty parent directories (but not the bucket dir itself)
           - Always return Ok (S3 behavior: delete is idempotent)

        7. Update delete_bucket to allow .meta/ directory in the empty check:
           Currently only allows ".bucket-metadata.json". Change to also allow ".meta".
           When checking if bucket is empty, skip both internal entries.

        8. Create src/routes/object.rs with handlers:

           a) PUT /{bucket}/{key+} → put_object_handler
              - Extract metadata from request headers:
                Content-Type (default "application/octet-stream")
                x-amz-meta-* custom headers (strip prefix, store key/value)
                Content-Disposition, Cache-Control, Content-Encoding, Expires
              - Read body as Bytes
              - Call storage.put_object()
              - Return 200 with ETag header

           b) GET /{bucket}/{key+} → get_object_handler
              - Call storage.get_object()
              - Return body with headers: Content-Type, Content-Length, ETag, Last-Modified,
                x-amz-meta-* (re-add prefix), Content-Disposition, Cache-Control, etc.

           c) HEAD /{bucket}/{key+} → head_object_handler
              - Call storage.head_object()
              - Return same headers as GET but no body

           d) DELETE /{bucket}/{key+} → delete_object_handler
              - Call storage.delete_object()
              - Return 204 No Content (always, even if key didn't exist)

           IMPORTANT: Distinguish PUT-with-copy-source from regular PUT.
           If x-amz-copy-source header is present, this is a CopyObject — handle in Task 2.
           For now, if x-amz-copy-source is present, return 501 Not Implemented.

        9. Wire up routes in server.rs:
           The key challenge is S3's URL structure:
           - /{bucket} → bucket operations (already exists)
           - /{bucket}/{key+} → object operations (new, key can contain slashes)

           In axum 0.8, use a wildcard: "/{bucket}/{*key}"
           This captures everything after the bucket name as the key.

           Route disambiguation:
           - POST /{bucket}?delete → batch delete (Task 2 — return 501 for now)
           - GET /{bucket} without key → list objects (already exists, returns placeholder)
           - PUT/GET/HEAD/DELETE /{bucket}/{*key} → object handlers

           Router addition:
           ```
           .route("/{bucket}/{*key}",
               put(object::put_object_handler)
                   .get(object::get_object_handler)
                   .head(object::head_object_handler)
                   .delete(object::delete_object_handler)
           )
           ```

        10. Handle presigned URL acceptance:
            The existing middleware already passes through SigV4 headers without validation.
            Presigned URLs include ?X-Amz-Signature, ?X-Amz-Credential, etc. as query params.
            These should be silently ignored — axum will just pass them through to the handler.
            No special code needed; the query params won't interfere with our handlers.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
        <manual>
          Start server: cargo run -- --port 4566 --data-dir ./test-data
          1. curl -X PUT http://localhost:4566/my-bucket
          2. curl -X PUT -d "hello world" -H "Content-Type: text/plain" http://localhost:4566/my-bucket/hello.txt
             → 200 with ETag header
          3. curl http://localhost:4566/my-bucket/hello.txt
             → "hello world" with Content-Type: text/plain, ETag, Content-Length headers
          4. curl -I http://localhost:4566/my-bucket/hello.txt
             → Same headers, no body
          5. curl -X DELETE http://localhost:4566/my-bucket/hello.txt → 204
          6. curl http://localhost:4566/my-bucket/hello.txt → 404 NoSuchKey XML
          7. curl -X DELETE http://localhost:4566/my-bucket/nonexistent → 204 (idempotent)
        </manual>
      </verification>

      <done>
        - PutObject stores file + metadata sidecar, returns ETag
        - GetObject returns file body with all metadata headers
        - HeadObject returns metadata headers without body
        - DeleteObject returns 204 always (idempotent)
        - Keys with slashes work (nested directories created automatically)
        - x-amz-meta-* custom metadata preserved round-trip
        - Content-Type, Content-Disposition, Cache-Control headers preserved
        - Existing bucket tests still pass
        - cargo build/clippy/fmt clean
      </done>
    </task>

    <task id="2" type="backend" complete="false">
      <name>CopyObject, DeleteObjects batch, ListObjectsV2, ListObjects V1</name>
      <description>
        Implement the remaining object operations: copy, batch delete, and both list operations.
        Add all required XML types for request/response serialization. Implement CommonPrefixes
        logic for delimiter-based "folder" simulation in list operations.
      </description>

      <files>
        <create>(none — all files exist from Task 1)</create>
        <modify>src/storage/filesystem.rs — add copy_object, delete_objects, list_objects_v2, list_objects methods</modify>
        <modify>src/routes/object.rs     — add copy_object_handler, update put to dispatch on x-amz-copy-source</modify>
        <modify>src/routes/bucket.rs     — replace placeholder get_bucket with real ListObjects, add POST handler for batch delete</modify>
        <modify>src/types/xml.rs         — add ListObjectsV2Result, ListObjectsResult, DeleteRequest, DeleteResult, CopyObjectResult XML types</modify>
        <modify>src/server.rs            — add POST route for /{bucket} (batch delete)</modify>
      </files>

      <action>
        1. Add XML types in types/xml.rs:

           ListObjectsV2Result:
           - xmlns, Name, Prefix, MaxKeys, IsTruncated, KeyCount
           - Delimiter (optional), StartAfter (optional), ContinuationToken (optional), NextContinuationToken (optional)
           - Contents: Vec&lt;ObjectEntry&gt; — each with Key, LastModified, ETag, Size, StorageClass
           - CommonPrefixes: Vec&lt;CommonPrefix&gt; — each with Prefix

           ListObjectsResult (V1 — similar but uses Marker/NextMarker instead of ContinuationToken):
           - xmlns, Name, Prefix, Marker, NextMarker, MaxKeys, IsTruncated, Delimiter
           - Contents: Vec&lt;ObjectEntry&gt;
           - CommonPrefixes: Vec&lt;CommonPrefix&gt;

           ObjectEntry:
           - Key, LastModified, ETag, Size, StorageClass ("STANDARD")

           CommonPrefix:
           - Prefix

           DeleteRequest (deserialize from POST body):
           - Object: Vec&lt;DeleteObjectEntry&gt; with Key and optional VersionId
           - Quiet: bool (optional)

           DeleteResult:
           - Deleted: Vec&lt;DeletedEntry&gt; with Key
           - Error: Vec&lt;DeleteErrorEntry&gt; with Key, Code, Message

           CopyObjectResult:
           - ETag, LastModified

        2. Implement storage.copy_object(src_bucket, src_key, dst_bucket, dst_key, metadata_directive):
           - Verify src bucket and key exist (NoSuchBucket, NoSuchKey)
           - Verify dst bucket exists (NoSuchBucket)
           - Read source object body and metadata
           - If metadata_directive is "REPLACE", use supplied new metadata
           - If metadata_directive is "COPY" (default), use source metadata
           - Write to destination using put_object logic
           - Return new ETag and LastModified

        3. Implement storage.delete_objects(bucket, keys: Vec&lt;String&gt;) -> Result&lt;(Vec&lt;String&gt;, Vec&lt;(String, String, String)&gt;)&gt;:
           - Verify bucket exists
           - For each key, call delete_object (which is idempotent)
           - Return (deleted_keys, errors) tuple
           - In practice, errors are rare since delete is idempotent

        4. Implement storage.list_objects(bucket, prefix, delimiter, max_keys, start_after) -> Result&lt;ListObjectsOutput&gt;:
           Define ListObjectsOutput struct:
           ```
           struct ListObjectsOutput {
               objects: Vec&lt;ObjectInfo&gt;,       // Key, Size, ETag, LastModified
               common_prefixes: Vec&lt;String&gt;,    // "folder/" prefixes when delimiter="/"
               is_truncated: bool,
               next_continuation_token: Option&lt;String&gt;,
           }
           ```

           Implementation:
           - Walk the .meta/ directory tree recursively to find all object metadata files
           - Build a sorted list of all keys
           - Apply prefix filter: only include keys starting with prefix
           - Apply delimiter logic:
             For each key matching prefix, find the next occurrence of delimiter after prefix
             If found, the portion up to and including the delimiter is a CommonPrefix
             Collect unique CommonPrefixes, don't include those keys in Contents
             If no delimiter occurrence after prefix, include key in Contents
           - Apply start_after filter (skip keys &lt;= start_after)
           - Apply max_keys limit (default 1000)
           - If more keys remain after max_keys, set is_truncated=true and next_continuation_token
             Use the last returned key as the token (base64-encoded)
           - ContinuationToken: if provided, decode and use as start_after

           CRITICAL EDGE CASES:
           - Delimiter "/" with prefix "folder/" should list objects and subfolders
           - Empty prefix lists everything
           - Keys with leading slashes are valid (rare but possible)
           - CommonPrefixes must be unique and sorted
           - MaxKeys applies to total of Contents + CommonPrefixes

        5. Update routes/object.rs — put_object_handler:
           Check for x-amz-copy-source header. If present:
           - Parse header value: "/{source_bucket}/{source_key}" or "/source_bucket/source_key"
             (may or may not have leading slash, may be URL-encoded)
           - Check x-amz-metadata-directive header (default "COPY")
           - Call storage.copy_object()
           - Return 200 with CopyObjectResult XML (ETag + LastModified)

        6. Update routes/bucket.rs — replace placeholder get_bucket with real list logic:
           Query params to extract:
           - list-type: if "2" → ListObjectsV2, else V1
           - prefix, delimiter, max-keys (default 1000)
           - V2: start-after, continuation-token
           - V1: marker
           - Call storage.list_objects() with appropriate params
           - Serialize to ListObjectsV2Result or ListObjectsResult XML

        7. Add POST /{bucket} handler in routes/bucket.rs for batch delete:
           - Query param ?delete must be present
           - Parse XML body as DeleteRequest
           - Call storage.delete_objects()
           - Return DeleteResult XML
           - Add POST route to server.rs

        8. Update server.rs router:
           - Add POST method to the /{bucket} route for batch delete
           - The /{bucket} POST handler should check for ?delete query param
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
        <manual>
          Test CopyObject:
          1. PUT object to bucket-a/original.txt
          2. curl -X PUT -H "x-amz-copy-source: /bucket-a/original.txt" http://localhost:4566/bucket-a/copy.txt
          3. GET copy.txt → same content

          Test ListObjectsV2:
          4. PUT multiple objects: folder/a.txt, folder/b.txt, other.txt
          5. GET /{bucket}?list-type=2 → all 3 in Contents
          6. GET /{bucket}?list-type=2&amp;prefix=folder/&amp;delimiter=/ → folder/a.txt, folder/b.txt in Contents
          7. GET /{bucket}?list-type=2&amp;delimiter=/ → other.txt in Contents, "folder/" in CommonPrefixes

          Test DeleteObjects:
          8. POST /{bucket}?delete with XML body listing keys → DeleteResult XML
        </manual>
      </verification>

      <done>
        - CopyObject works same-bucket and cross-bucket
        - CopyObject respects x-amz-metadata-directive (COPY/REPLACE)
        - DeleteObjects batch works with proper XML request/response
        - ListObjectsV2 with prefix, delimiter, max-keys, continuation-token
        - ListObjects V1 with prefix, delimiter, max-keys, marker
        - CommonPrefixes correctly simulate folders with delimiter="/"
        - Pagination works (IsTruncated, NextContinuationToken)
        - All existing tests still pass
      </done>
    </task>

    <task id="3" type="test" complete="false">
      <name>AWS SDK integration tests for all object operations</name>
      <description>
        Comprehensive integration tests using aws-sdk-s3 for PutObject, GetObject, HeadObject,
        DeleteObject, CopyObject, DeleteObjects, ListObjectsV2, and ListObjects V1.
        Tests must verify metadata round-trip, ETag correctness, listing with prefixes/delimiters,
        and pagination.
      </description>

      <files>
        <create>tests/object_integration.rs — all object operation integration tests</create>
        <modify>tests/bucket_integration.rs — add test for delete non-empty bucket (should fail)</modify>
      </files>

      <action>
        1. Create tests/object_integration.rs with same TestServer helper pattern as bucket tests.

        2. Basic CRUD tests:
           - test_put_and_get_object: PUT text, GET it back, verify body + Content-Type + ETag
           - test_put_object_with_metadata: PUT with x-amz-meta-* headers, GET back, verify custom metadata
           - test_head_object: PUT object, HEAD it, verify all headers match GET (no body)
           - test_delete_object: PUT, verify exists, DELETE, verify 404
           - test_delete_nonexistent_object: DELETE key that doesn't exist → 204 (no error)
           - test_put_object_overwrite: PUT same key twice, verify second content replaces first

        3. ETag tests:
           - test_etag_is_md5: PUT known content, verify ETag matches MD5 hex
           - test_etag_changes_on_overwrite: PUT different content to same key, verify ETag changes

        4. Nested key tests:
           - test_nested_key: PUT "folder/subfolder/file.txt", GET it back
           - test_keys_with_special_chars: PUT keys with spaces, dots, dashes

        5. CopyObject tests:
           - test_copy_object_same_bucket: PUT src, COPY to dst in same bucket, verify content
           - test_copy_object_cross_bucket: COPY from bucket-a to bucket-b
           - test_copy_nonexistent_object: COPY from missing key → error

        6. DeleteObjects batch tests:
           - test_delete_objects_batch: PUT 5 objects, DELETE 3 via batch, verify 2 remain
           - test_delete_objects_some_missing: batch delete includes non-existent keys → still succeeds

        7. ListObjectsV2 tests:
           - test_list_objects_basic: PUT 3 objects, list all, verify all 3 in Contents
           - test_list_objects_with_prefix: PUT objects with different prefixes, filter by prefix
           - test_list_objects_with_delimiter: PUT "a.txt", "dir/b.txt", "dir/c.txt", list with delimiter="/"
             → "a.txt" in Contents, "dir/" in CommonPrefixes
           - test_list_objects_pagination: PUT 5 objects, list with max-keys=2, verify IsTruncated + ContinuationToken,
             continue with token, verify remaining objects
           - test_list_objects_empty_bucket: list objects in empty bucket → empty Contents

        8. Bucket-not-empty test (add to bucket_integration.rs):
           - test_delete_bucket_with_objects: PUT object in bucket, try delete bucket → BucketNotEmpty error

        9. Presigned URL test:
           - test_get_with_query_params: Send GET with extra query params (simulating presigned URL params)
             → should still work and return the object
      </action>

      <verification>
        <command>cargo test --test object_integration</command>
        <command>cargo test --test bucket_integration</command>
        <command>cargo test</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - All object integration tests pass with real aws-sdk-s3 client
        - Metadata round-trip verified (custom headers, content-type, etc.)
        - ETag correctness verified against known MD5 values
        - ListObjectsV2 with prefix/delimiter/pagination fully tested
        - CopyObject same-bucket and cross-bucket tested
        - DeleteObjects batch tested
        - All existing bucket tests still pass
        - Total test count significantly increased from baseline 22
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
      Start server: cargo run -- --port 4566 --data-dir ./data
      Full workflow test:
      1. Create bucket
      2. PUT object with custom metadata
      3. GET object → verify body + headers
      4. HEAD object → verify headers
      5. COPY object to new key
      6. LIST objects → verify both keys appear
      7. LIST with delimiter → verify CommonPrefixes
      8. Batch DELETE both
      9. LIST → empty
      10. Delete bucket → succeeds
    </manual>
  </phase_verification>

  <completion_criteria>
    <criterion>All 3 tasks marked complete</criterion>
    <criterion>All verification commands pass (build, test, clippy, fmt)</criterion>
    <criterion>AWS SDK integration tests pass for all object operations</criterion>
    <criterion>No TODO comments left in new code</criterion>
    <criterion>Existing Phase 1 tests still pass</criterion>
  </completion_criteria>
</plan>
