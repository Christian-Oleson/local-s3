use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::CompletedMultipartUpload;
use aws_sdk_s3::types::CompletedPart;
use tempfile::TempDir;

use local_s3::secretsmanager::storage::SecretsStorage;
use local_s3::server::{AppState, build_router};
use local_s3::storage::FileSystemStorage;

struct TestServer {
    client: Client,
    _tmp_dir: TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let tmp_dir = TempDir::new().expect("Failed to create temp dir");
        let storage = FileSystemStorage::new(tmp_dir.path().to_path_buf())
            .await
            .expect("Failed to create storage");
        let secrets_storage = SecretsStorage::new(tmp_dir.path().to_path_buf())
            .await
            .expect("Failed to create secrets storage");

        let state = AppState {
            storage: Arc::new(storage),
            secrets_storage: Arc::new(secrets_storage),
        };

        let app = build_router(state);

        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();
        listener
            .set_nonblocking(true)
            .expect("Failed to set non-blocking");

        let tokio_listener =
            tokio::net::TcpListener::from_std(listener).expect("Failed to convert listener");

        tokio::spawn(async move {
            axum::serve(tokio_listener, app).await.unwrap();
        });

        let creds = Credentials::new("test", "test", None, None, "test");
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(creds)
            .endpoint_url(format!("http://127.0.0.1:{port}"))
            .force_path_style(true)
            .retry_config(RetryConfig::disabled())
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .timeout_config(
                aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                    .operation_timeout(Duration::from_secs(5))
                    .operation_attempt_timeout(Duration::from_secs(5))
                    .connect_timeout(Duration::from_secs(2))
                    .build(),
            )
            .build();

        let client = Client::from_conf(config);

        Self {
            client,
            _tmp_dir: tmp_dir,
        }
    }

    async fn create_bucket(&self, name: &str) {
        self.client
            .create_bucket()
            .bucket(name)
            .send()
            .await
            .unwrap();
    }

    async fn put_object(&self, bucket: &str, key: &str, body: &[u8]) {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(body.to_vec()))
            .send()
            .await
            .unwrap();
    }
}

// --- Basic Object CRUD tests ---

#[tokio::test]
async fn test_put_and_get_object() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let put_result = s
        .client
        .put_object()
        .bucket("test-bucket")
        .key("hello.txt")
        .content_type("text/plain")
        .body(ByteStream::from(b"hello world".to_vec()))
        .send()
        .await
        .unwrap();

    assert!(put_result.e_tag().is_some());

    let get_result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("hello.txt")
        .send()
        .await
        .unwrap();

    assert_eq!(get_result.content_type(), Some("text/plain"));
    assert_eq!(get_result.content_length(), Some(11));
    assert!(get_result.e_tag().is_some());

    let body = get_result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"hello world");
}

#[tokio::test]
async fn test_head_object() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.client
        .put_object()
        .bucket("test-bucket")
        .key("data.bin")
        .content_type("application/octet-stream")
        .body(ByteStream::from(vec![1, 2, 3, 4, 5]))
        .send()
        .await
        .unwrap();

    let head = s
        .client
        .head_object()
        .bucket("test-bucket")
        .key("data.bin")
        .send()
        .await
        .unwrap();

    assert_eq!(head.content_length(), Some(5));
    assert!(head.e_tag().is_some());
    assert!(head.last_modified().is_some());
}

#[tokio::test]
async fn test_delete_object() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;
    s.put_object("test-bucket", "del.txt", b"delete me").await;

    s.client
        .delete_object()
        .bucket("test-bucket")
        .key("del.txt")
        .send()
        .await
        .unwrap();

    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("del.txt")
        .send()
        .await;
    assert!(result.is_err(), "Object should be gone after delete");
}

#[tokio::test]
async fn test_delete_nonexistent_object() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    // Should not error — S3 delete is idempotent
    s.client
        .delete_object()
        .bucket("test-bucket")
        .key("nope.txt")
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_put_object_overwrite() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "file.txt", b"version 1").await;
    s.put_object("test-bucket", "file.txt", b"version 2").await;

    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("file.txt")
        .send()
        .await
        .unwrap();

    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"version 2");
}

#[tokio::test]
async fn test_nested_key() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "a/b/c/deep.txt", b"deep content")
        .await;

    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("a/b/c/deep.txt")
        .send()
        .await
        .unwrap();

    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"deep content");
}

#[tokio::test]
async fn test_etag_is_md5() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let content = b"test content for md5";
    let expected_md5 = format!("\"{}\"", md5_hex(content));

    let put_result = s
        .client
        .put_object()
        .bucket("test-bucket")
        .key("md5test.txt")
        .body(ByteStream::from(content.to_vec()))
        .send()
        .await
        .unwrap();

    assert_eq!(put_result.e_tag(), Some(expected_md5.as_str()));
}

fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[tokio::test]
async fn test_delete_bucket_with_objects_fails() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;
    s.put_object("test-bucket", "file.txt", b"content").await;

    let result = s.client.delete_bucket().bucket("test-bucket").send().await;
    assert!(
        result.is_err(),
        "Should not be able to delete non-empty bucket"
    );
}

// --- ListObjectsV2 tests ---

#[tokio::test]
async fn test_list_objects_v2_empty() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let result = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    assert_eq!(result.key_count(), Some(0));
    assert_eq!(result.is_truncated(), Some(false));
    assert!(result.contents().is_empty());
}

#[tokio::test]
async fn test_list_objects_v2_with_objects() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "a.txt", b"aaa").await;
    s.put_object("test-bucket", "b.txt", b"bbb").await;
    s.put_object("test-bucket", "c.txt", b"ccc").await;

    let result = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    assert_eq!(result.key_count(), Some(3));
    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["a.txt", "b.txt", "c.txt"]);
}

#[tokio::test]
async fn test_list_objects_v2_with_prefix() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "photos/a.jpg", b"img1").await;
    s.put_object("test-bucket", "photos/b.jpg", b"img2").await;
    s.put_object("test-bucket", "docs/readme.txt", b"doc").await;

    let result = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("photos/")
        .send()
        .await
        .unwrap();

    assert_eq!(result.key_count(), Some(2));
    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["photos/a.jpg", "photos/b.jpg"]);
}

#[tokio::test]
async fn test_list_objects_v2_with_delimiter() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "photos/a.jpg", b"img1").await;
    s.put_object("test-bucket", "docs/readme.txt", b"doc").await;
    s.put_object("test-bucket", "root.txt", b"root").await;

    let result = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .delimiter("/")
        .send()
        .await
        .unwrap();

    // Only root.txt in contents
    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["root.txt"]);

    // Common prefixes
    let prefixes: Vec<&str> = result
        .common_prefixes()
        .iter()
        .filter_map(|cp| cp.prefix())
        .collect();
    assert_eq!(prefixes, vec!["docs/", "photos/"]);
}

#[tokio::test]
async fn test_list_objects_v2_max_keys() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "a.txt", b"a").await;
    s.put_object("test-bucket", "b.txt", b"b").await;
    s.put_object("test-bucket", "c.txt", b"c").await;

    let result = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .max_keys(2)
        .send()
        .await
        .unwrap();

    assert_eq!(result.key_count(), Some(2));
    assert_eq!(result.is_truncated(), Some(true));
    assert!(result.next_continuation_token().is_some());

    // Fetch the rest using continuation token
    let result2 = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .max_keys(2)
        .continuation_token(result.next_continuation_token().unwrap())
        .send()
        .await
        .unwrap();

    assert_eq!(result2.key_count(), Some(1));
    assert_eq!(result2.is_truncated(), Some(false));
    let keys: Vec<&str> = result2.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["c.txt"]);
}

// --- CopyObject tests ---

#[tokio::test]
async fn test_copy_object() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "original.txt", b"hello copy")
        .await;

    s.client
        .copy_object()
        .bucket("test-bucket")
        .key("copy.txt")
        .copy_source("test-bucket/original.txt")
        .send()
        .await
        .unwrap();

    // Verify the copy exists
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("copy.txt")
        .send()
        .await
        .unwrap();

    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"hello copy");
}

#[tokio::test]
async fn test_copy_object_cross_bucket() {
    let s = TestServer::start().await;
    s.create_bucket("source-bucket").await;
    s.create_bucket("dest-bucket").await;

    s.put_object("source-bucket", "file.txt", b"cross bucket copy")
        .await;

    s.client
        .copy_object()
        .bucket("dest-bucket")
        .key("file.txt")
        .copy_source("source-bucket/file.txt")
        .send()
        .await
        .unwrap();

    let result = s
        .client
        .get_object()
        .bucket("dest-bucket")
        .key("file.txt")
        .send()
        .await
        .unwrap();

    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"cross bucket copy");
}

// --- DeleteObjects batch tests ---

#[tokio::test]
async fn test_delete_objects_batch() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "a.txt", b"aaa").await;
    s.put_object("test-bucket", "b.txt", b"bbb").await;
    s.put_object("test-bucket", "c.txt", b"ccc").await;

    let objects_to_delete = vec![
        aws_sdk_s3::types::ObjectIdentifier::builder()
            .key("a.txt")
            .build()
            .unwrap(),
        aws_sdk_s3::types::ObjectIdentifier::builder()
            .key("c.txt")
            .build()
            .unwrap(),
    ];

    let delete = aws_sdk_s3::types::Delete::builder()
        .set_objects(Some(objects_to_delete))
        .build()
        .unwrap();

    let result = s
        .client
        .delete_objects()
        .bucket("test-bucket")
        .delete(delete)
        .send()
        .await
        .unwrap();

    let deleted: Vec<&str> = result.deleted().iter().filter_map(|d| d.key()).collect();
    assert_eq!(deleted.len(), 2);

    // Only b.txt should remain
    let list = s
        .client
        .list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();
    let remaining: Vec<&str> = list.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(remaining, vec!["b.txt"]);
}

// --- ListObjects V1 tests ---

#[tokio::test]
async fn test_list_objects_v1() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "x.txt", b"xxx").await;
    s.put_object("test-bucket", "y.txt", b"yyy").await;

    let result = s
        .client
        .list_objects()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["x.txt", "y.txt"]);
}

#[tokio::test]
async fn test_list_objects_v1_with_delimiter() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "dir/a.txt", b"a").await;
    s.put_object("test-bucket", "dir/b.txt", b"b").await;
    s.put_object("test-bucket", "file.txt", b"f").await;

    let result = s
        .client
        .list_objects()
        .bucket("test-bucket")
        .delimiter("/")
        .send()
        .await
        .unwrap();

    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert_eq!(keys, vec!["file.txt"]);

    let prefixes: Vec<&str> = result
        .common_prefixes()
        .iter()
        .filter_map(|cp| cp.prefix())
        .collect();
    assert_eq!(prefixes, vec!["dir/"]);
}

// --- Multipart Upload tests ---

#[tokio::test]
async fn test_multipart_upload_complete() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    // Create multipart upload
    let create_result = s
        .client
        .create_multipart_upload()
        .bucket("test-bucket")
        .key("multipart.bin")
        .send()
        .await
        .unwrap();

    let upload_id = create_result.upload_id().unwrap().to_string();
    assert!(!upload_id.is_empty());

    // Upload part 1
    let part1_data = b"hello ";
    let part1_result = s
        .client
        .upload_part()
        .bucket("test-bucket")
        .key("multipart.bin")
        .upload_id(&upload_id)
        .part_number(1)
        .body(ByteStream::from(part1_data.to_vec()))
        .send()
        .await
        .unwrap();
    let etag1 = part1_result.e_tag().unwrap().to_string();

    // Upload part 2
    let part2_data = b"world";
    let part2_result = s
        .client
        .upload_part()
        .bucket("test-bucket")
        .key("multipart.bin")
        .upload_id(&upload_id)
        .part_number(2)
        .body(ByteStream::from(part2_data.to_vec()))
        .send()
        .await
        .unwrap();
    let etag2 = part2_result.e_tag().unwrap().to_string();

    // Complete multipart upload
    let completed = CompletedMultipartUpload::builder()
        .parts(
            CompletedPart::builder()
                .part_number(1)
                .e_tag(&etag1)
                .build(),
        )
        .parts(
            CompletedPart::builder()
                .part_number(2)
                .e_tag(&etag2)
                .build(),
        )
        .build();

    let complete_result = s
        .client
        .complete_multipart_upload()
        .bucket("test-bucket")
        .key("multipart.bin")
        .upload_id(&upload_id)
        .multipart_upload(completed)
        .send()
        .await
        .unwrap();

    // Verify the ETag has composite format (hex-N)
    let etag = complete_result.e_tag().unwrap();
    assert!(
        etag.contains('-'),
        "Composite ETag should contain dash: {etag}"
    );

    // Verify the assembled content
    let get_result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("multipart.bin")
        .send()
        .await
        .unwrap();

    let body = get_result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"hello world");
}

#[tokio::test]
async fn test_multipart_upload_abort() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    // Create multipart upload
    let create_result = s
        .client
        .create_multipart_upload()
        .bucket("test-bucket")
        .key("abort-me.bin")
        .send()
        .await
        .unwrap();

    let upload_id = create_result.upload_id().unwrap().to_string();

    // Upload a part
    s.client
        .upload_part()
        .bucket("test-bucket")
        .key("abort-me.bin")
        .upload_id(&upload_id)
        .part_number(1)
        .body(ByteStream::from(b"data".to_vec()))
        .send()
        .await
        .unwrap();

    // Abort the upload
    s.client
        .abort_multipart_upload()
        .bucket("test-bucket")
        .key("abort-me.bin")
        .upload_id(&upload_id)
        .send()
        .await
        .unwrap();

    // Verify the object was not created
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("abort-me.bin")
        .send()
        .await;
    assert!(result.is_err(), "Object should not exist after abort");
}

#[tokio::test]
async fn test_multipart_upload_list_parts() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let create_result = s
        .client
        .create_multipart_upload()
        .bucket("test-bucket")
        .key("parts-test.bin")
        .send()
        .await
        .unwrap();

    let upload_id = create_result.upload_id().unwrap().to_string();

    // Upload 3 parts
    for i in 1..=3 {
        s.client
            .upload_part()
            .bucket("test-bucket")
            .key("parts-test.bin")
            .upload_id(&upload_id)
            .part_number(i)
            .body(ByteStream::from(format!("part{i}").into_bytes()))
            .send()
            .await
            .unwrap();
    }

    // List parts
    let list_result = s
        .client
        .list_parts()
        .bucket("test-bucket")
        .key("parts-test.bin")
        .upload_id(&upload_id)
        .send()
        .await
        .unwrap();

    let parts = list_result.parts();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].part_number(), Some(1));
    assert_eq!(parts[1].part_number(), Some(2));
    assert_eq!(parts[2].part_number(), Some(3));

    // Clean up
    s.client
        .abort_multipart_upload()
        .bucket("test-bucket")
        .key("parts-test.bin")
        .upload_id(&upload_id)
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_multipart_upload_list_uploads() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    // Create two multipart uploads
    let create1 = s
        .client
        .create_multipart_upload()
        .bucket("test-bucket")
        .key("file1.bin")
        .send()
        .await
        .unwrap();
    let id1 = create1.upload_id().unwrap().to_string();

    let create2 = s
        .client
        .create_multipart_upload()
        .bucket("test-bucket")
        .key("file2.bin")
        .send()
        .await
        .unwrap();
    let id2 = create2.upload_id().unwrap().to_string();

    // List multipart uploads
    let list_result = s
        .client
        .list_multipart_uploads()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    let uploads = list_result.uploads();
    assert_eq!(uploads.len(), 2);

    let upload_ids: Vec<&str> = uploads.iter().filter_map(|u| u.upload_id()).collect();
    assert!(upload_ids.contains(&id1.as_str()));
    assert!(upload_ids.contains(&id2.as_str()));

    // Clean up
    s.client
        .abort_multipart_upload()
        .bucket("test-bucket")
        .key("file1.bin")
        .upload_id(&id1)
        .send()
        .await
        .unwrap();
    s.client
        .abort_multipart_upload()
        .bucket("test-bucket")
        .key("file2.bin")
        .upload_id(&id2)
        .send()
        .await
        .unwrap();
}

// --- Range request tests ---

#[tokio::test]
async fn test_range_request() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let content = b"abcdefghijklmnopqrstuvwxyz";
    s.put_object("test-bucket", "alphabet.txt", content).await;

    // GET with range bytes=0-4 -> "abcde"
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("alphabet.txt")
        .range("bytes=0-4")
        .send()
        .await
        .unwrap();

    assert_eq!(result.content_length(), Some(5));
    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"abcde");
}

#[tokio::test]
async fn test_range_request_suffix() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let content = b"abcdefghijklmnopqrstuvwxyz";
    s.put_object("test-bucket", "alphabet.txt", content).await;

    // GET with range bytes=-5 -> last 5 bytes -> "vwxyz"
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("alphabet.txt")
        .range("bytes=-5")
        .send()
        .await
        .unwrap();

    assert_eq!(result.content_length(), Some(5));
    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"vwxyz");
}

// --- Conditional request tests ---

#[tokio::test]
async fn test_if_none_match_304() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    let put_result = s
        .client
        .put_object()
        .bucket("test-bucket")
        .key("cond.txt")
        .body(ByteStream::from(b"conditional data".to_vec()))
        .send()
        .await
        .unwrap();

    let etag = put_result.e_tag().unwrap().to_string();

    // GET with If-None-Match set to the object's ETag should result in 304.
    // The SDK surfaces a 304 as an error because there is no response body.
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("cond.txt")
        .if_none_match(&etag)
        .send()
        .await;

    assert!(
        result.is_err(),
        "Expected 304 Not Modified to be surfaced as an error by the SDK"
    );
}

#[tokio::test]
async fn test_if_none_match_different_etag() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;

    s.put_object("test-bucket", "cond2.txt", b"some data").await;

    // GET with a non-matching ETag should succeed normally
    let result = s
        .client
        .get_object()
        .bucket("test-bucket")
        .key("cond2.txt")
        .if_none_match("\"not-the-real-etag\"")
        .send()
        .await
        .unwrap();

    let body = result.body.collect().await.unwrap().into_bytes();
    assert_eq!(body.as_ref(), b"some data");
}

// --- Object Tagging tests ---

#[tokio::test]
async fn test_put_and_get_object_tagging() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;
    s.put_object("test-bucket", "tagged.txt", b"tagged content")
        .await;

    let tagging = aws_sdk_s3::types::Tagging::builder()
        .tag_set(
            aws_sdk_s3::types::Tag::builder()
                .key("env")
                .value("production")
                .build()
                .unwrap(),
        )
        .tag_set(
            aws_sdk_s3::types::Tag::builder()
                .key("team")
                .value("backend")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    s.client
        .put_object_tagging()
        .bucket("test-bucket")
        .key("tagged.txt")
        .tagging(tagging)
        .send()
        .await
        .unwrap();

    let get_result = s
        .client
        .get_object_tagging()
        .bucket("test-bucket")
        .key("tagged.txt")
        .send()
        .await
        .unwrap();

    let tags = get_result.tag_set();
    assert_eq!(tags.len(), 2);

    let mut tag_map: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for tag in tags {
        tag_map.insert(tag.key(), tag.value());
    }
    assert_eq!(tag_map.get("env"), Some(&"production"));
    assert_eq!(tag_map.get("team"), Some(&"backend"));
}

#[tokio::test]
async fn test_delete_object_tagging() {
    let s = TestServer::start().await;
    s.create_bucket("test-bucket").await;
    s.put_object("test-bucket", "tagged-del.txt", b"content")
        .await;

    // Put some tags
    let tagging = aws_sdk_s3::types::Tagging::builder()
        .tag_set(
            aws_sdk_s3::types::Tag::builder()
                .key("color")
                .value("blue")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    s.client
        .put_object_tagging()
        .bucket("test-bucket")
        .key("tagged-del.txt")
        .tagging(tagging)
        .send()
        .await
        .unwrap();

    // Delete the tags
    s.client
        .delete_object_tagging()
        .bucket("test-bucket")
        .key("tagged-del.txt")
        .send()
        .await
        .unwrap();

    // Verify tags are gone
    let get_result = s
        .client
        .get_object_tagging()
        .bucket("test-bucket")
        .key("tagged-del.txt")
        .send()
        .await
        .unwrap();

    assert!(
        get_result.tag_set().is_empty(),
        "Tag set should be empty after delete"
    );
}

// --- CORS Configuration tests ---

#[tokio::test]
async fn test_put_and_get_bucket_cors() {
    let s = TestServer::start().await;
    s.create_bucket("cors-bucket").await;

    let cors_rule = aws_sdk_s3::types::CorsRule::builder()
        .allowed_origins("*")
        .allowed_methods("GET")
        .allowed_methods("PUT")
        .allowed_headers("*")
        .build()
        .unwrap();

    let cors_config = aws_sdk_s3::types::CorsConfiguration::builder()
        .cors_rules(cors_rule)
        .build()
        .unwrap();

    s.client
        .put_bucket_cors()
        .bucket("cors-bucket")
        .cors_configuration(cors_config)
        .send()
        .await
        .unwrap();

    let get_result = s
        .client
        .get_bucket_cors()
        .bucket("cors-bucket")
        .send()
        .await
        .unwrap();

    let rules = get_result.cors_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].allowed_origins(), &["*".to_string()]);
    assert_eq!(
        rules[0].allowed_methods(),
        &["GET".to_string(), "PUT".to_string()]
    );
    assert_eq!(rules[0].allowed_headers(), &["*".to_string()]);
}

#[tokio::test]
async fn test_delete_bucket_cors() {
    let s = TestServer::start().await;
    s.create_bucket("cors-del-bucket").await;

    // Put CORS config
    let cors_rule = aws_sdk_s3::types::CorsRule::builder()
        .allowed_origins("http://example.com")
        .allowed_methods("GET")
        .build()
        .unwrap();

    let cors_config = aws_sdk_s3::types::CorsConfiguration::builder()
        .cors_rules(cors_rule)
        .build()
        .unwrap();

    s.client
        .put_bucket_cors()
        .bucket("cors-del-bucket")
        .cors_configuration(cors_config)
        .send()
        .await
        .unwrap();

    // Delete CORS config
    s.client
        .delete_bucket_cors()
        .bucket("cors-del-bucket")
        .send()
        .await
        .unwrap();

    // GET CORS should fail (no config)
    let result = s
        .client
        .get_bucket_cors()
        .bucket("cors-del-bucket")
        .send()
        .await;

    assert!(
        result.is_err(),
        "Getting CORS after delete should fail with no configuration"
    );
}
