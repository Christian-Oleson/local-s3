use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
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
}

#[tokio::test]
async fn test_create_and_list_bucket() {
    let s = TestServer::start().await;

    s.client
        .create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    let result = s.client.list_buckets().send().await.unwrap();

    let buckets = result.buckets();
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].name(), Some("test-bucket"));
    assert!(buckets[0].creation_date().is_some());
}

#[tokio::test]
async fn test_create_bucket_with_region() {
    let s = TestServer::start().await;

    s.client
        .create_bucket()
        .bucket("region-bucket")
        .create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::UsWest2)
                .build(),
        )
        .send()
        .await
        .unwrap();

    let result = s
        .client
        .get_bucket_location()
        .bucket("region-bucket")
        .send()
        .await
        .unwrap();

    assert_eq!(
        result.location_constraint(),
        Some(&BucketLocationConstraint::UsWest2)
    );
}

#[tokio::test]
async fn test_create_duplicate_bucket() {
    let s = TestServer::start().await;

    s.client
        .create_bucket()
        .bucket("dup-bucket")
        .send()
        .await
        .unwrap();

    let result = s.client.create_bucket().bucket("dup-bucket").send().await;

    assert!(result.is_err(), "Duplicate bucket should fail");
}

#[tokio::test]
async fn test_delete_bucket() {
    let s = TestServer::start().await;

    s.client
        .create_bucket()
        .bucket("del-bucket")
        .send()
        .await
        .unwrap();

    s.client
        .head_bucket()
        .bucket("del-bucket")
        .send()
        .await
        .unwrap();

    s.client
        .delete_bucket()
        .bucket("del-bucket")
        .send()
        .await
        .unwrap();

    let list = s.client.list_buckets().send().await.unwrap();
    assert!(list.buckets().is_empty());

    let head_result = s.client.head_bucket().bucket("del-bucket").send().await;
    assert!(head_result.is_err(), "HeadBucket should fail after delete");
}

#[tokio::test]
async fn test_delete_nonexistent_bucket() {
    let s = TestServer::start().await;

    let result = s.client.delete_bucket().bucket("ghost-bucket").send().await;

    assert!(result.is_err(), "Deleting nonexistent bucket should fail");
}

#[tokio::test]
async fn test_head_bucket() {
    let s = TestServer::start().await;

    let result = s.client.head_bucket().bucket("no-bucket").send().await;
    assert!(result.is_err());

    s.client
        .create_bucket()
        .bucket("head-bucket")
        .send()
        .await
        .unwrap();

    s.client
        .head_bucket()
        .bucket("head-bucket")
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_list_buckets_empty() {
    let s = TestServer::start().await;

    let result = s.client.list_buckets().send().await.unwrap();

    assert!(result.buckets().is_empty());
}

#[tokio::test]
async fn test_create_multiple_buckets() {
    let s = TestServer::start().await;

    let names = ["alpha", "beta", "gamma", "delta", "epsilon"];
    for name in &names {
        s.client
            .create_bucket()
            .bucket(*name)
            .send()
            .await
            .unwrap_or_else(|e| panic!("Failed to create bucket {name}: {e}"));
    }

    let result = s.client.list_buckets().send().await.unwrap();
    assert_eq!(result.buckets().len(), 5);

    s.client
        .delete_bucket()
        .bucket("beta")
        .send()
        .await
        .unwrap();
    s.client
        .delete_bucket()
        .bucket("delta")
        .send()
        .await
        .unwrap();

    let result = s.client.list_buckets().send().await.unwrap();
    let remaining: Vec<&str> = result.buckets().iter().filter_map(|b| b.name()).collect();
    assert_eq!(remaining.len(), 3);
    assert!(remaining.contains(&"alpha"));
    assert!(remaining.contains(&"gamma"));
    assert!(remaining.contains(&"epsilon"));
}
