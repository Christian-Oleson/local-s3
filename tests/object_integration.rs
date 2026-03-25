use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::primitives::ByteStream;
use tempfile::TempDir;

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

        let state = AppState {
            storage: Arc::new(storage),
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
