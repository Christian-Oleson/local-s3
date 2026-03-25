<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: 3 -->

<plan>
  <metadata>
    <phase>3</phase>
    <name>Multipart Upload &amp; Advanced Features</name>
    <goal>Support large file uploads and commonly-used S3 features (tagging, range requests, CORS, conditional requests)</goal>
    <deliverable>Full multipart upload lifecycle, object tagging, range reads, and CORS — covers P1 requirements</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>Phase 2 complete — full object CRUD, copy, batch delete, list operations, 71 tests</dependencies>
    <affected_areas>
      - src/storage/filesystem.rs — multipart upload state, tagging, CORS config storage
      - src/routes/object.rs — multipart handlers, tagging handlers, range/conditional on GET
      - src/routes/bucket.rs — CORS config handlers, multipart upload initiation
      - src/server.rs — new POST routes for multipart, tagging sub-resources
      - src/types/xml.rs — multipart XML types, tagging XML types
      - src/error.rs — new error variants (NoSuchUpload, InvalidPart, etc.)
      - src/middleware.rs — CORS preflight handling
    </affected_areas>
    <patterns_to_follow>
      - Multipart upload state stored at {bucket}/.uploads/{upload_id}/ with part files and state.json
      - Composite ETag for multipart: MD5 of concatenated part MD5s, suffixed with "-{part_count}"
      - Tagging stored in metadata sidecar (extend ObjectMetadata with tags field)
      - CORS config stored as {bucket}/.cors.json
      - Follow existing patterns: xml_response(), S3Error, State(AppState) extractors
      - POST /{bucket}/{key}?uploads → CreateMultipartUpload
      - PUT /{bucket}/{key}?partNumber=N&amp;uploadId=X → UploadPart
      - POST /{bucket}/{key}?uploadId=X → CompleteMultipartUpload
      - DELETE /{bucket}/{key}?uploadId=X → AbortMultipartUpload
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="backend" complete="false">
      <name>Multipart upload lifecycle: create, upload parts, complete, abort, list</name>
      <description>
        Implement the full multipart upload lifecycle: CreateMultipartUpload, UploadPart,
        CompleteMultipartUpload, AbortMultipartUpload, ListParts, ListMultipartUploads.
        Storage layer stores parts as individual files, assembly concatenates them.
        Includes all XML types and HTTP handlers.
      </description>

      <files>
        <modify>src/storage/filesystem.rs — multipart methods + upload state management</modify>
        <modify>src/routes/object.rs — multipart handlers (dispatch on query params)</modify>
        <modify>src/routes/bucket.rs — ListMultipartUploads handler (GET ?uploads)</modify>
        <modify>src/server.rs — POST routes for /{bucket}/{*key} (multipart create + complete)</modify>
        <modify>src/types/xml.rs — multipart XML types</modify>
        <modify>src/error.rs — NoSuchUpload, InvalidPart error variants</modify>
      </files>

      <action>
        1. Add error variants to src/error.rs:
           - NoSuchUpload { upload_id: String } → 404
           - InvalidPart { message: String } → 400
           Add corresponding code(), status_code(), resource() implementations.

        2. Add XML types to src/types/xml.rs:

           InitiateMultipartUploadResult:
           - Bucket, Key, UploadId

           CompleteMultipartUploadResult:
           - Location, Bucket, Key, ETag

           CompleteMultipartUploadRequest (deserialize):
           - Part entries: Vec with PartNumber and ETag

           ListPartsResult:
           - Bucket, Key, UploadId, PartNumberMarker, NextPartNumberMarker,
             MaxParts, IsTruncated, Parts (Vec with PartNumber, LastModified, ETag, Size)

           ListMultipartUploadsResult:
           - Bucket, KeyMarker, UploadIdMarker, NextKeyMarker, NextUploadIdMarker,
             MaxUploads, IsTruncated, Uploads (Vec with Key, UploadId, Initiated)

        3. Storage layer — multipart state management:

           Upload state stored at: {bucket}/.uploads/{upload_id}/
           - state.json: { key, upload_id, initiated, parts: {} }
           - part files: {part_number}.part (raw bytes)

           Methods:
           a) create_multipart_upload(bucket, key) → Result&lt;String&gt; (returns upload_id)
              - Verify bucket exists
              - Generate UUID upload_id
              - Create upload directory + state.json
              - Return upload_id

           b) upload_part(bucket, key, upload_id, part_number, body) → Result&lt;String&gt; (returns ETag)
              - Verify upload exists
              - Write part body to {part_number}.part
              - Compute MD5 ETag for the part
              - Update state.json with part info
              - Return part ETag

           c) complete_multipart_upload(bucket, key, upload_id, parts) → Result&lt;ObjectMetadata&gt;
              - Verify upload exists and all specified parts exist
              - Concatenate parts in order to create final object body
              - Compute composite ETag: MD5(concat(part_md5_bytes)) + "-{part_count}"
              - Write assembled object via put_object
              - Clean up upload directory
              - Return metadata with composite ETag

           d) abort_multipart_upload(bucket, key, upload_id) → Result&lt;()&gt;
              - Verify upload exists
              - Remove entire upload directory
              - Return Ok

           e) list_parts(bucket, upload_id, max_parts, part_number_marker) → Result&lt;...&gt;
              - Read state.json, return part info

           f) list_multipart_uploads(bucket, prefix, max_uploads) → Result&lt;...&gt;
              - Scan .uploads/ directory for all active uploads
              - Return upload info

           Also update delete_bucket to check for active uploads (or allow .uploads dir in empty check).

        4. HTTP handlers in routes/object.rs:

           The key challenge: S3 uses query params to distinguish multipart operations on the same path.
           Modify put_object_handler to check for ?partNumber and ?uploadId → dispatch to upload_part.
           Add POST handler for /{bucket}/{*key}:
           - If ?uploads query param → create_multipart_upload
           - If ?uploadId query param → complete_multipart_upload
           Add DELETE handler modification: if ?uploadId → abort_multipart_upload

           For GET /{bucket}/{*key}: if ?uploadId → list_parts

           For GET /{bucket}: if ?uploads query param → list_multipart_uploads (in bucket.rs)

        5. Router updates in server.rs:
           Add .post() to the /{bucket_name}/{*key} route for multipart create/complete.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - Full multipart lifecycle works: create → upload parts → complete → object exists
        - Abort cleans up all state
        - ListParts returns correct part info
        - ListMultipartUploads shows active uploads
        - Composite ETag format: "md5hex-partcount"
        - All existing 71 tests still pass
      </done>
    </task>

    <task id="2" type="backend" complete="false">
      <name>Range requests, conditional requests, object tagging, CORS</name>
      <description>
        Implement Range header support on GetObject, conditional requests (If-None-Match,
        If-Modified-Since), object tagging CRUD, and bucket CORS configuration with
        OPTIONS preflight handling.
      </description>

      <files>
        <modify>src/routes/object.rs — range parsing, conditional headers, tagging handlers</modify>
        <modify>src/routes/bucket.rs — CORS config handlers</modify>
        <modify>src/storage/filesystem.rs — tagging storage, CORS config storage</modify>
        <modify>src/types/xml.rs — tagging and CORS XML types</modify>
        <modify>src/types/object.rs — add tags field to ObjectMetadata</modify>
        <modify>src/server.rs — tagging and CORS routes</modify>
        <modify>src/middleware.rs — CORS preflight response</modify>
      </files>

      <action>
        1. Range requests on GetObject (routes/object.rs):
           - Parse Range header: "bytes=start-end" or "bytes=start-" or "bytes=-suffix"
           - Return 206 Partial Content with Content-Range header
           - Accept-Ranges: bytes header on all GetObject responses
           - If range is invalid or unsatisfiable: 416 Range Not Satisfiable

        2. Conditional requests (routes/object.rs):
           - If-None-Match: compare with ETag → 304 Not Modified if match
           - If-Modified-Since: compare with Last-Modified → 304 if not modified
           - Apply to both GetObject and HeadObject

        3. Object tagging storage (storage/filesystem.rs):
           Store tags in a separate sidecar: {bucket}/.tags/{key}.json
           (Don't embed in ObjectMetadata to avoid re-writing metadata on tag changes)

           Methods:
           - put_object_tagging(bucket, key, tags: HashMap&lt;String,String&gt;) → Result&lt;()&gt;
           - get_object_tagging(bucket, key) → Result&lt;HashMap&lt;String,String&gt;&gt;
           - delete_object_tagging(bucket, key) → Result&lt;()&gt;

        4. Tagging XML types (types/xml.rs):
           - Tagging { TagSet: { Tag: Vec&lt;{Key, Value}&gt; } } (both serialize and deserialize)

        5. Tagging HTTP handlers (routes/object.rs):
           - PUT /{bucket}/{key}?tagging → PutObjectTagging (parse XML body)
           - GET /{bucket}/{key}?tagging → GetObjectTagging (return XML)
           - DELETE /{bucket}/{key}?tagging → DeleteObjectTagging

           Dispatch: modify get/put/delete handlers to check for ?tagging query param.

        6. CORS configuration storage (storage/filesystem.rs):
           Store as {bucket}/.cors.json

           Methods:
           - put_bucket_cors(bucket, config) → Result&lt;()&gt;
           - get_bucket_cors(bucket) → Result&lt;CorsConfiguration&gt;
           - delete_bucket_cors(bucket) → Result&lt;()&gt;

        7. CORS XML types (types/xml.rs):
           - CORSConfiguration { CORSRule: Vec with AllowedOrigin, AllowedMethod, AllowedHeader, MaxAgeSeconds, ExposeHeader }

        8. CORS HTTP handlers (routes/bucket.rs):
           - PUT /{bucket}?cors → PutBucketCors
           - GET /{bucket}?cors → GetBucketCors
           - DELETE /{bucket}?cors → DeleteBucketCors
           Add ?cors dispatch to bucket get/put/delete handlers.

        9. CORS preflight (middleware.rs or routes):
           Handle OPTIONS requests: read bucket's CORS config, return appropriate
           Access-Control-Allow-Origin, Access-Control-Allow-Methods, etc. headers.
           For local dev, can be permissive (allow all origins if CORS is configured).

        10. Update delete_bucket empty check to allow .tags/ and .cors.json.
      </action>

      <verification>
        <command>cargo build</command>
        <command>cargo test --lib</command>
        <command>cargo clippy -- -D warnings</command>
        <command>cargo fmt -- --check</command>
      </verification>

      <done>
        - Range requests return 206 with correct Content-Range
        - If-None-Match returns 304 when ETag matches
        - If-Modified-Since returns 304 when not modified
        - Tagging CRUD works (put, get, delete)
        - CORS config CRUD works (put, get, delete)
        - OPTIONS preflight returns CORS headers
        - All existing tests still pass
      </done>
    </task>

    <task id="3" type="test" complete="false">
      <name>Integration tests for multipart, tagging, range, CORS, conditional</name>
      <description>
        Comprehensive AWS SDK integration tests for all Phase 3 features.
      </description>

      <files>
        <create>tests/multipart_integration.rs — multipart upload lifecycle tests</create>
        <create>tests/advanced_integration.rs — tagging, range, conditional, CORS tests</create>
      </files>

      <action>
        1. Multipart upload tests (tests/multipart_integration.rs):
           - test_multipart_upload_lifecycle: create → upload 3 parts → complete → get object → verify body
           - test_multipart_upload_abort: create → upload parts → abort → verify no object
           - test_list_parts: create → upload parts → list parts → verify part info
           - test_list_multipart_uploads: create multiple uploads → list → verify all shown
           - test_multipart_etag_format: verify composite ETag has "-N" suffix

        2. Range request tests (tests/advanced_integration.rs):
           - test_range_request: PUT 1KB, GET with Range: bytes=0-9 → 10 bytes
           - test_range_request_suffix: GET with Range: bytes=-5 → last 5 bytes

        3. Conditional request tests:
           - test_if_none_match: GET with matching ETag → 304
           - test_if_modified_since: GET with future date → 304

        4. Tagging tests:
           - test_put_and_get_tagging: PUT tags, GET tags, verify round-trip
           - test_delete_tagging: PUT tags, DELETE, GET → empty

        5. CORS tests:
           - test_put_and_get_cors: PUT CORS config, GET config, verify
           - test_delete_cors: PUT CORS, DELETE, GET → error
      </action>

      <verification>
        <command>cargo test --test multipart_integration</command>
        <command>cargo test --test advanced_integration</command>
        <command>cargo test</command>
      </verification>

      <done>
        - All multipart lifecycle tests pass with real AWS SDK
        - Range, conditional, tagging, CORS tests pass
        - All existing tests still pass
        - Significant test count increase from baseline 71
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
      Full multipart workflow:
      1. Create bucket, initiate multipart upload
      2. Upload 3 parts (each 5MB+)
      3. Complete upload
      4. GET object → verify full body is correct
      5. Verify composite ETag format

      Range + conditional:
      6. GET with Range header → 206 with partial body
      7. GET with If-None-Match: "matching-etag" → 304

      Tagging:
      8. PUT tagging, GET tagging → round-trip
    </manual>
  </phase_verification>

  <completion_criteria>
    <criterion>All 3 tasks marked complete</criterion>
    <criterion>All verification commands pass</criterion>
    <criterion>Multipart upload lifecycle fully functional via AWS SDK</criterion>
    <criterion>Range requests return 206 with correct partial content</criterion>
    <criterion>No TODO comments left in new code</criterion>
  </completion_criteria>
</plan>
