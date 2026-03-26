use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::primitives::ByteStream;
use tempfile::TempDir;

use local_s3::secretsmanager::storage::SecretsStorage;
use local_s3::server::{AppState, build_router};
use local_s3::storage::FileSystemStorage;

struct TestServer {
    s3_client: aws_sdk_s3::Client,
    sm_client: aws_sdk_secretsmanager::Client,
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

        let timeout_config = aws_sdk_s3::config::timeout::TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(5))
            .operation_attempt_timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(2))
            .build();

        let s3_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(creds.clone())
            .endpoint_url(format!("http://127.0.0.1:{port}"))
            .force_path_style(true)
            .retry_config(RetryConfig::disabled())
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .timeout_config(timeout_config.clone())
            .build();
        let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

        let sm_config = aws_sdk_secretsmanager::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(creds.clone())
            .endpoint_url(format!("http://127.0.0.1:{port}"))
            .retry_config(RetryConfig::disabled())
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .timeout_config(timeout_config.clone())
            .build();
        let sm_client = aws_sdk_secretsmanager::Client::from_conf(sm_config);

        Self {
            s3_client,
            sm_client,
            _tmp_dir: tmp_dir,
        }
    }
}

#[tokio::test]
async fn test_create_and_get_secret() {
    let s = TestServer::start().await;

    let create_result = s
        .sm_client
        .create_secret()
        .name("test/my-secret")
        .secret_string("super-secret-value")
        .send()
        .await
        .unwrap();

    assert!(create_result.arn().is_some());
    assert_eq!(create_result.name(), Some("test/my-secret"));
    assert!(create_result.version_id().is_some());

    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("test/my-secret")
        .send()
        .await
        .unwrap();

    assert_eq!(get_result.name(), Some("test/my-secret"));
    assert_eq!(get_result.secret_string(), Some("super-secret-value"));
    assert!(get_result.arn().is_some());
    assert_eq!(get_result.arn(), create_result.arn());
}

#[tokio::test]
async fn test_create_secret_with_description() {
    let s = TestServer::start().await;

    let create_result = s
        .sm_client
        .create_secret()
        .name("described-secret")
        .description("A secret with a description")
        .secret_string("desc-value")
        .send()
        .await
        .unwrap();

    assert_eq!(create_result.name(), Some("described-secret"));
    assert!(create_result.arn().is_some());

    // GetSecretValue does not return Description, but the secret should be accessible
    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("described-secret")
        .send()
        .await
        .unwrap();

    assert_eq!(get_result.secret_string(), Some("desc-value"));
}

#[tokio::test]
async fn test_put_secret_value_updates() {
    let s = TestServer::start().await;

    // Create the initial secret
    s.sm_client
        .create_secret()
        .name("update-secret")
        .secret_string("original-value")
        .send()
        .await
        .unwrap();

    // Put a new value
    s.sm_client
        .put_secret_value()
        .secret_id("update-secret")
        .secret_string("updated-value")
        .send()
        .await
        .unwrap();

    // Get AWSCURRENT -- should be the new value
    let get_current = s
        .sm_client
        .get_secret_value()
        .secret_id("update-secret")
        .send()
        .await
        .unwrap();

    assert_eq!(get_current.secret_string(), Some("updated-value"));

    // Get AWSPREVIOUS -- should be the old value
    let get_previous = s
        .sm_client
        .get_secret_value()
        .secret_id("update-secret")
        .version_stage("AWSPREVIOUS")
        .send()
        .await
        .unwrap();

    assert_eq!(get_previous.secret_string(), Some("original-value"));
}

#[tokio::test]
async fn test_delete_secret_force() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("force-delete-secret")
        .secret_string("gone-soon")
        .send()
        .await
        .unwrap();

    s.sm_client
        .delete_secret()
        .secret_id("force-delete-secret")
        .force_delete_without_recovery(true)
        .send()
        .await
        .unwrap();

    // Get should fail with ResourceNotFoundException
    let result = s
        .sm_client
        .get_secret_value()
        .secret_id("force-delete-secret")
        .send()
        .await;

    assert!(result.is_err(), "Get after force-delete should fail");
    let err = result.unwrap_err();
    let service_err = err.as_service_error().expect("Should be a service error");
    assert!(
        service_err.is_resource_not_found_exception(),
        "Expected ResourceNotFoundException, got: {err:?}"
    );
}

#[tokio::test]
async fn test_delete_and_restore_secret() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("restore-me")
        .secret_string("precious-value")
        .send()
        .await
        .unwrap();

    // Delete with recovery window
    s.sm_client
        .delete_secret()
        .secret_id("restore-me")
        .recovery_window_in_days(7)
        .send()
        .await
        .unwrap();

    // Get should fail while pending deletion
    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("restore-me")
        .send()
        .await;
    assert!(
        get_result.is_err(),
        "Get should fail for secret pending deletion"
    );

    // Restore the secret
    s.sm_client
        .restore_secret()
        .secret_id("restore-me")
        .send()
        .await
        .unwrap();

    // Get should work again
    let get_after_restore = s
        .sm_client
        .get_secret_value()
        .secret_id("restore-me")
        .send()
        .await
        .unwrap();

    assert_eq!(get_after_restore.secret_string(), Some("precious-value"));
}

#[tokio::test]
async fn test_create_duplicate_fails() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("dup-secret")
        .secret_string("first")
        .send()
        .await
        .unwrap();

    let result = s
        .sm_client
        .create_secret()
        .name("dup-secret")
        .secret_string("second")
        .send()
        .await;

    assert!(result.is_err(), "Creating duplicate secret should fail");
    let err = result.unwrap_err();
    let service_err = err.as_service_error().expect("Should be a service error");
    assert!(
        service_err.is_resource_exists_exception(),
        "Expected ResourceExistsException, got: {err:?}"
    );
}

#[tokio::test]
async fn test_get_nonexistent_fails() {
    let s = TestServer::start().await;

    let result = s
        .sm_client
        .get_secret_value()
        .secret_id("does-not-exist")
        .send()
        .await;

    assert!(result.is_err(), "Getting nonexistent secret should fail");
    let err = result.unwrap_err();
    let service_err = err.as_service_error().expect("Should be a service error");
    assert!(
        service_err.is_resource_not_found_exception(),
        "Expected ResourceNotFoundException, got: {err:?}"
    );
}

#[tokio::test]
async fn test_s3_and_sm_coexist() {
    let s = TestServer::start().await;

    // Create an S3 bucket and put an object
    s.s3_client
        .create_bucket()
        .bucket("coexist-bucket")
        .send()
        .await
        .unwrap();

    s.s3_client
        .put_object()
        .bucket("coexist-bucket")
        .key("hello.txt")
        .body(ByteStream::from(b"s3 content".to_vec()))
        .send()
        .await
        .unwrap();

    // Create a Secrets Manager secret
    s.sm_client
        .create_secret()
        .name("coexist-secret")
        .secret_string("sm content")
        .send()
        .await
        .unwrap();

    // Verify both work on the same port
    let s3_get = s
        .s3_client
        .get_object()
        .bucket("coexist-bucket")
        .key("hello.txt")
        .send()
        .await
        .unwrap();
    let s3_body = s3_get.body.collect().await.unwrap().into_bytes();
    assert_eq!(s3_body.as_ref(), b"s3 content");

    let sm_get = s
        .sm_client
        .get_secret_value()
        .secret_id("coexist-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(sm_get.secret_string(), Some("sm content"));
}

#[tokio::test]
async fn test_create_secret_with_binary() {
    let s = TestServer::start().await;

    let binary_data = aws_sdk_secretsmanager::primitives::Blob::new(b"hello binary" as &[u8]);

    let create_result = s
        .sm_client
        .create_secret()
        .name("binary-secret")
        .secret_binary(binary_data.clone())
        .send()
        .await
        .unwrap();

    assert_eq!(create_result.name(), Some("binary-secret"));

    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("binary-secret")
        .send()
        .await
        .unwrap();

    assert!(
        get_result.secret_string().is_none(),
        "SecretString should be None for binary secret"
    );
    let returned_binary = get_result
        .secret_binary()
        .expect("SecretBinary should be present");
    assert_eq!(returned_binary.as_ref(), b"hello binary");
}
