use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::primitives::ByteStream;
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

    async fn enable_versioning(&self, bucket: &str) {
        self.client
            .put_bucket_versioning()
            .bucket(bucket)
            .versioning_configuration(
                aws_sdk_s3::types::VersioningConfiguration::builder()
                    .status(aws_sdk_s3::types::BucketVersioningStatus::Enabled)
                    .build(),
            )
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

// --- Versioning lifecycle tests ---

#[tokio::test]
async fn test_enable_versioning() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;

    s.enable_versioning("ver-bucket").await;

    let result = s
        .client
        .get_bucket_versioning()
        .bucket("ver-bucket")
        .send()
        .await
        .unwrap();

    assert_eq!(
        result.status(),
        Some(&aws_sdk_s3::types::BucketVersioningStatus::Enabled)
    );
}

#[tokio::test]
async fn test_versioned_put_returns_version_id() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;
    s.enable_versioning("ver-bucket").await;

    let put_result = s
        .client
        .put_object()
        .bucket("ver-bucket")
        .key("doc.txt")
        .body(ByteStream::from(b"content v1".to_vec()))
        .send()
        .await
        .unwrap();

    assert!(
        put_result.version_id().is_some(),
        "Put on versioned bucket should return a version_id"
    );
    assert!(
        !put_result.version_id().unwrap().is_empty(),
        "version_id should not be empty"
    );
}

#[tokio::test]
async fn test_versioned_put_multiple_versions() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;
    s.enable_versioning("ver-bucket").await;

    // Put same key 3 times with different content
    for i in 1..=3 {
        s.client
            .put_object()
            .bucket("ver-bucket")
            .key("multi.txt")
            .body(ByteStream::from(format!("version {i}").into_bytes()))
            .send()
            .await
            .unwrap();
    }

    let versions_result = s
        .client
        .list_object_versions()
        .bucket("ver-bucket")
        .send()
        .await
        .unwrap();

    let versions = versions_result.versions();
    assert_eq!(
        versions.len(),
        3,
        "Expected 3 versions, got {}",
        versions.len()
    );

    // All versions should have the same key
    for v in versions {
        assert_eq!(v.key(), Some("multi.txt"));
    }
}

#[tokio::test]
async fn test_get_specific_version() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;
    s.enable_versioning("ver-bucket").await;

    // Put version 1
    let put1 = s
        .client
        .put_object()
        .bucket("ver-bucket")
        .key("data.txt")
        .body(ByteStream::from(b"first version".to_vec()))
        .send()
        .await
        .unwrap();
    let vid1 = put1.version_id().unwrap().to_string();

    // Put version 2
    let put2 = s
        .client
        .put_object()
        .bucket("ver-bucket")
        .key("data.txt")
        .body(ByteStream::from(b"second version".to_vec()))
        .send()
        .await
        .unwrap();
    let vid2 = put2.version_id().unwrap().to_string();

    assert_ne!(vid1, vid2, "Version IDs should be different");

    // Get version 1 by ID
    let get1 = s
        .client
        .get_object()
        .bucket("ver-bucket")
        .key("data.txt")
        .version_id(&vid1)
        .send()
        .await
        .unwrap();
    let body1 = get1.body.collect().await.unwrap().into_bytes();
    assert_eq!(body1.as_ref(), b"first version");

    // Get version 2 by ID
    let get2 = s
        .client
        .get_object()
        .bucket("ver-bucket")
        .key("data.txt")
        .version_id(&vid2)
        .send()
        .await
        .unwrap();
    let body2 = get2.body.collect().await.unwrap().into_bytes();
    assert_eq!(body2.as_ref(), b"second version");

    // Get without version ID should return the latest (version 2)
    let get_latest = s
        .client
        .get_object()
        .bucket("ver-bucket")
        .key("data.txt")
        .send()
        .await
        .unwrap();
    let body_latest = get_latest.body.collect().await.unwrap().into_bytes();
    assert_eq!(body_latest.as_ref(), b"second version");
}

#[tokio::test]
async fn test_delete_creates_delete_marker() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;
    s.enable_versioning("ver-bucket").await;

    s.put_object("ver-bucket", "ephemeral.txt", b"delete me")
        .await;

    // Delete without specifying a version ID (should create a delete marker)
    s.client
        .delete_object()
        .bucket("ver-bucket")
        .key("ephemeral.txt")
        .send()
        .await
        .unwrap();

    // GET should now fail because the current version is a delete marker
    let get_result = s
        .client
        .get_object()
        .bucket("ver-bucket")
        .key("ephemeral.txt")
        .send()
        .await;

    assert!(
        get_result.is_err(),
        "GET after delete-marker creation should fail (404/NoSuchKey)"
    );
}

#[tokio::test]
async fn test_list_object_versions() {
    let s = TestServer::start().await;
    s.create_bucket("ver-bucket").await;
    s.enable_versioning("ver-bucket").await;

    // Put 2 objects, each with 2 versions
    for key in &["alpha.txt", "beta.txt"] {
        for i in 1..=2 {
            s.client
                .put_object()
                .bucket("ver-bucket")
                .key(*key)
                .body(ByteStream::from(format!("{key} version {i}").into_bytes()))
                .send()
                .await
                .unwrap();
        }
    }

    let result = s
        .client
        .list_object_versions()
        .bucket("ver-bucket")
        .send()
        .await
        .unwrap();

    let versions = result.versions();
    assert_eq!(
        versions.len(),
        4,
        "Expected 4 versions (2 objects x 2 versions), got {}",
        versions.len()
    );

    // Each version should have a non-empty version_id
    for v in versions {
        assert!(
            v.version_id().is_some(),
            "Each version should have a version_id"
        );
        assert!(
            !v.version_id().unwrap().is_empty(),
            "version_id should not be empty"
        );
    }

    // Count versions per key
    let alpha_count = versions
        .iter()
        .filter(|v| v.key() == Some("alpha.txt"))
        .count();
    let beta_count = versions
        .iter()
        .filter(|v| v.key() == Some("beta.txt"))
        .count();
    assert_eq!(alpha_count, 2, "alpha.txt should have 2 versions");
    assert_eq!(beta_count, 2, "beta.txt should have 2 versions");
}

// --- Config storage tests ---

#[tokio::test]
async fn test_put_get_bucket_policy() {
    let s = TestServer::start().await;
    s.create_bucket("policy-bucket").await;

    let policy_json = r#"{"Version":"2012-10-17","Statement":[{"Sid":"PublicRead","Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::policy-bucket/*"}]}"#;

    s.client
        .put_bucket_policy()
        .bucket("policy-bucket")
        .policy(policy_json)
        .send()
        .await
        .unwrap();

    let get_result = s
        .client
        .get_bucket_policy()
        .bucket("policy-bucket")
        .send()
        .await
        .unwrap();

    let returned_policy = get_result.policy().unwrap_or("");
    assert_eq!(
        returned_policy, policy_json,
        "Returned policy should match what was put"
    );
}

#[tokio::test]
async fn test_put_get_bucket_acl() {
    let s = TestServer::start().await;
    s.create_bucket("acl-bucket").await;

    // The AWS SDK's get_bucket_acl returns structured data, not raw XML.
    // We test by getting the ACL after creation (default ACL should exist).
    let acl_result = s
        .client
        .get_bucket_acl()
        .bucket("acl-bucket")
        .send()
        .await
        .unwrap();

    // The default ACL should have an owner
    let owner = acl_result.owner();
    assert!(owner.is_some(), "ACL should have an owner");

    // Should have at least one grant
    let grants = acl_result.grants();
    assert!(
        !grants.is_empty(),
        "Default ACL should have at least one grant"
    );

    // The first grant should be FULL_CONTROL
    let first_grant = &grants[0];
    assert_eq!(
        first_grant.permission(),
        Some(&aws_sdk_s3::types::Permission::FullControl)
    );
}

#[tokio::test]
async fn test_default_bucket_acl() {
    let s = TestServer::start().await;
    s.create_bucket("default-acl-bucket").await;

    // Without putting any ACL, the default should be returned
    let acl_result = s
        .client
        .get_bucket_acl()
        .bucket("default-acl-bucket")
        .send()
        .await
        .unwrap();

    // Should have an owner with a display name
    let owner = acl_result.owner().expect("Default ACL should have owner");
    assert!(
        owner.display_name().is_some(),
        "Owner should have a display name"
    );

    // Should have grants
    let grants = acl_result.grants();
    assert!(
        !grants.is_empty(),
        "Default ACL should have at least one grant"
    );

    // Verify it's FULL_CONTROL for the owner
    let perm = grants[0].permission();
    assert_eq!(perm, Some(&aws_sdk_s3::types::Permission::FullControl));
}
