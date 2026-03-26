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

// ---------------------------------------------------------------------------
// Phase 2 tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_describe_secret() {
    let s = TestServer::start().await;

    let create_result = s
        .sm_client
        .create_secret()
        .name("describe-me")
        .description("A described secret")
        .secret_string("my-value")
        .tags(
            aws_sdk_secretsmanager::types::Tag::builder()
                .key("env")
                .value("test")
                .build(),
        )
        .send()
        .await
        .unwrap();

    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("describe-me")
        .send()
        .await
        .unwrap();

    assert_eq!(desc.name(), Some("describe-me"));
    assert_eq!(desc.arn(), create_result.arn());
    assert_eq!(desc.description(), Some("A described secret"));

    let tags = desc.tags();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].key(), Some("env"));
    assert_eq!(tags[0].value(), Some("test"));

    // VersionIdsToStages should have AWSCURRENT
    let stages = desc.version_ids_to_stages().unwrap();
    assert_eq!(stages.len(), 1);
    let (_, stage_list) = stages.iter().next().unwrap();
    assert!(stage_list.contains(&"AWSCURRENT".to_string()));
}

#[tokio::test]
async fn test_describe_secret_version_stages() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("describe-versions")
        .secret_string("v1")
        .send()
        .await
        .unwrap();

    // Put a new value so v1 becomes AWSPREVIOUS
    s.sm_client
        .put_secret_value()
        .secret_id("describe-versions")
        .secret_string("v2")
        .send()
        .await
        .unwrap();

    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("describe-versions")
        .send()
        .await
        .unwrap();

    let stages = desc.version_ids_to_stages().unwrap();
    assert_eq!(stages.len(), 2, "Should have 2 version entries");

    // Collect all stage labels
    let mut all_stages: Vec<String> = stages.values().flat_map(|v| v.iter().cloned()).collect();
    all_stages.sort();
    assert!(all_stages.contains(&"AWSCURRENT".to_string()));
    assert!(all_stages.contains(&"AWSPREVIOUS".to_string()));
}

#[tokio::test]
async fn test_list_secrets() {
    let s = TestServer::start().await;

    for name in &["list-alpha", "list-beta", "list-gamma"] {
        s.sm_client
            .create_secret()
            .name(*name)
            .secret_string("value")
            .send()
            .await
            .unwrap();
    }

    let result = s.sm_client.list_secrets().send().await.unwrap();

    let names: Vec<&str> = result
        .secret_list()
        .iter()
        .filter_map(|e| e.name())
        .collect();
    assert!(names.contains(&"list-alpha"));
    assert!(names.contains(&"list-beta"));
    assert!(names.contains(&"list-gamma"));
    assert_eq!(names.len(), 3);
}

#[tokio::test]
async fn test_list_secrets_pagination() {
    let s = TestServer::start().await;

    for i in 1..=5 {
        s.sm_client
            .create_secret()
            .name(format!("page-secret-{i:02}"))
            .secret_string(format!("value-{i}"))
            .send()
            .await
            .unwrap();
    }

    // First page: max_results=2
    let page1 = s
        .sm_client
        .list_secrets()
        .max_results(2)
        .send()
        .await
        .unwrap();

    assert_eq!(page1.secret_list().len(), 2);
    let token1 = page1.next_token().expect("Should have next token");

    // Second page
    let page2 = s
        .sm_client
        .list_secrets()
        .max_results(2)
        .next_token(token1)
        .send()
        .await
        .unwrap();

    assert_eq!(page2.secret_list().len(), 2);
    let token2 = page2.next_token().expect("Should have next token");

    // Third page (last one, 1 remaining)
    let page3 = s
        .sm_client
        .list_secrets()
        .max_results(2)
        .next_token(token2)
        .send()
        .await
        .unwrap();

    assert_eq!(page3.secret_list().len(), 1);
    assert!(page3.next_token().is_none(), "No more pages");

    // Verify all 5 unique names were returned
    let mut all_names: Vec<String> = Vec::new();
    for page in [&page1, &page2, &page3] {
        for entry in page.secret_list() {
            all_names.push(entry.name().unwrap().to_string());
        }
    }
    all_names.sort();
    assert_eq!(all_names.len(), 5);
    for i in 1..=5 {
        assert!(all_names.contains(&format!("page-secret-{i:02}")));
    }
}

#[tokio::test]
async fn test_list_secrets_filter_by_name() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("alpha-secret")
        .secret_string("a")
        .send()
        .await
        .unwrap();
    s.sm_client
        .create_secret()
        .name("beta-secret")
        .secret_string("b")
        .send()
        .await
        .unwrap();
    s.sm_client
        .create_secret()
        .name("gamma-test")
        .secret_string("c")
        .send()
        .await
        .unwrap();

    let result = s
        .sm_client
        .list_secrets()
        .filters(
            aws_sdk_secretsmanager::types::Filter::builder()
                .key(aws_sdk_secretsmanager::types::FilterNameStringType::Name)
                .values("secret")
                .build(),
        )
        .send()
        .await
        .unwrap();

    let names: Vec<&str> = result
        .secret_list()
        .iter()
        .filter_map(|e| e.name())
        .collect();
    assert_eq!(names.len(), 2, "Only secrets with 'secret' in name");
    assert!(names.contains(&"alpha-secret"));
    assert!(names.contains(&"beta-secret"));
}

#[tokio::test]
async fn test_update_secret_description() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("update-desc")
        .description("old description")
        .secret_string("original")
        .send()
        .await
        .unwrap();

    let update_result = s
        .sm_client
        .update_secret()
        .secret_id("update-desc")
        .description("new description")
        .send()
        .await
        .unwrap();

    // VersionId should be None since no value was changed
    assert!(
        update_result.version_id().is_none(),
        "No new version should be created for metadata-only update"
    );

    // Describe to verify new description
    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("update-desc")
        .send()
        .await
        .unwrap();

    assert_eq!(desc.description(), Some("new description"));

    // Original value should still be accessible
    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("update-desc")
        .send()
        .await
        .unwrap();
    assert_eq!(get_result.secret_string(), Some("original"));
}

#[tokio::test]
async fn test_update_secret_value() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("update-val")
        .secret_string("old-value")
        .send()
        .await
        .unwrap();

    let update_result = s
        .sm_client
        .update_secret()
        .secret_id("update-val")
        .secret_string("new-value")
        .send()
        .await
        .unwrap();

    assert!(
        update_result.version_id().is_some(),
        "Should have a new version id"
    );

    let get_result = s
        .sm_client
        .get_secret_value()
        .secret_id("update-val")
        .send()
        .await
        .unwrap();
    assert_eq!(get_result.secret_string(), Some("new-value"));
}

#[tokio::test]
async fn test_list_secret_version_ids() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("versions-test")
        .secret_string("v1")
        .send()
        .await
        .unwrap();

    s.sm_client
        .put_secret_value()
        .secret_id("versions-test")
        .secret_string("v2")
        .send()
        .await
        .unwrap();

    s.sm_client
        .put_secret_value()
        .secret_id("versions-test")
        .secret_string("v3")
        .send()
        .await
        .unwrap();

    let result = s
        .sm_client
        .list_secret_version_ids()
        .secret_id("versions-test")
        .include_deprecated(true)
        .send()
        .await
        .unwrap();

    assert_eq!(result.name(), Some("versions-test"));
    assert!(result.arn().is_some());

    let versions = result.versions();
    assert_eq!(
        versions.len(),
        3,
        "Should have 3 versions (include deprecated)"
    );

    // Verify at least one has AWSCURRENT and one has AWSPREVIOUS
    let all_stages: Vec<String> = versions
        .iter()
        .flat_map(|v| v.version_stages().iter().map(|s| s.to_string()))
        .collect();
    assert!(
        all_stages.contains(&"AWSCURRENT".to_string()),
        "Should have AWSCURRENT"
    );
    assert!(
        all_stages.contains(&"AWSPREVIOUS".to_string()),
        "Should have AWSPREVIOUS"
    );
}

// ---------------------------------------------------------------------------
// Phase 3 tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tag_and_untag_resource() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("tag-test")
        .secret_string("value")
        .send()
        .await
        .unwrap();

    // Tag with 2 tags
    s.sm_client
        .tag_resource()
        .secret_id("tag-test")
        .tags(
            aws_sdk_secretsmanager::types::Tag::builder()
                .key("env")
                .value("prod")
                .build(),
        )
        .tags(
            aws_sdk_secretsmanager::types::Tag::builder()
                .key("team")
                .value("backend")
                .build(),
        )
        .send()
        .await
        .unwrap();

    // Describe -> should have 2 tags
    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("tag-test")
        .send()
        .await
        .unwrap();
    let tags = desc.tags();
    assert_eq!(tags.len(), 2, "Should have 2 tags after tagging");

    // Untag 1 key
    s.sm_client
        .untag_resource()
        .secret_id("tag-test")
        .tag_keys("env")
        .send()
        .await
        .unwrap();

    // Describe -> should have 1 tag
    let desc2 = s
        .sm_client
        .describe_secret()
        .secret_id("tag-test")
        .send()
        .await
        .unwrap();
    let tags2 = desc2.tags();
    assert_eq!(tags2.len(), 1, "Should have 1 tag after untagging");
    assert_eq!(tags2[0].key(), Some("team"));
    assert_eq!(tags2[0].value(), Some("backend"));
}

#[tokio::test]
async fn test_put_and_get_resource_policy() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("policy-test")
        .secret_string("value")
        .send()
        .await
        .unwrap();

    let policy_json = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;

    // Put resource policy
    s.sm_client
        .put_resource_policy()
        .secret_id("policy-test")
        .resource_policy(policy_json)
        .send()
        .await
        .unwrap();

    // Get resource policy
    let get_resp = s
        .sm_client
        .get_resource_policy()
        .secret_id("policy-test")
        .send()
        .await
        .unwrap();

    assert_eq!(
        get_resp.resource_policy(),
        Some(policy_json),
        "Resource policy should match what was put"
    );
    assert!(get_resp.arn().is_some());
    assert_eq!(get_resp.name(), Some("policy-test"));
}

#[tokio::test]
async fn test_delete_resource_policy() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("policy-del-test")
        .secret_string("value")
        .send()
        .await
        .unwrap();

    let policy_json = r#"{"Version":"2012-10-17"}"#;

    // Put a policy
    s.sm_client
        .put_resource_policy()
        .secret_id("policy-del-test")
        .resource_policy(policy_json)
        .send()
        .await
        .unwrap();

    // Delete the policy
    s.sm_client
        .delete_resource_policy()
        .secret_id("policy-del-test")
        .send()
        .await
        .unwrap();

    // Get should return no policy
    let get_resp = s
        .sm_client
        .get_resource_policy()
        .secret_id("policy-del-test")
        .send()
        .await
        .unwrap();

    assert!(
        get_resp.resource_policy().is_none(),
        "Resource policy should be None after deletion"
    );
}

#[tokio::test]
async fn test_rotate_secret_config() {
    let s = TestServer::start().await;

    s.sm_client
        .create_secret()
        .name("rotate-config-test")
        .secret_string("initial-value")
        .send()
        .await
        .unwrap();

    // Rotate with lambda ARN and rules
    let rotate_resp = s
        .sm_client
        .rotate_secret()
        .secret_id("rotate-config-test")
        .rotation_lambda_arn("arn:aws:lambda:us-east-1:000000000000:function:my-rotation")
        .rotation_rules(
            aws_sdk_secretsmanager::types::RotationRulesType::builder()
                .automatically_after_days(30)
                .build(),
        )
        .send()
        .await
        .unwrap();

    assert!(rotate_resp.arn().is_some());
    assert_eq!(rotate_resp.name(), Some("rotate-config-test"));
    assert!(
        rotate_resp.version_id().is_some(),
        "Should return a version id for the pending version"
    );

    // Describe -> rotation_enabled should be true
    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("rotate-config-test")
        .send()
        .await
        .unwrap();

    assert_eq!(
        desc.rotation_enabled(),
        Some(true),
        "Rotation should be enabled after rotate_secret"
    );
}

#[tokio::test]
async fn test_update_version_stage() {
    let s = TestServer::start().await;

    // Create secret with initial value
    let create_resp = s
        .sm_client
        .create_secret()
        .name("stage-test")
        .secret_string("v1")
        .send()
        .await
        .unwrap();
    let v1_id = create_resp.version_id().unwrap().to_string();

    // Put a new value -> v2 becomes AWSCURRENT, v1 becomes AWSPREVIOUS
    let put_resp = s
        .sm_client
        .put_secret_value()
        .secret_id("stage-test")
        .secret_string("v2")
        .send()
        .await
        .unwrap();
    let v2_id = put_resp.version_id().unwrap().to_string();

    // Move a custom label "MYLABEL" to v1
    s.sm_client
        .update_secret_version_stage()
        .secret_id("stage-test")
        .version_stage("MYLABEL")
        .move_to_version_id(&v1_id)
        .send()
        .await
        .unwrap();

    // Describe -> verify v1 has MYLABEL
    let desc = s
        .sm_client
        .describe_secret()
        .secret_id("stage-test")
        .send()
        .await
        .unwrap();

    let stages = desc.version_ids_to_stages().unwrap();
    let v1_stages = stages.get(&v1_id).expect("v1 should have stages");
    assert!(
        v1_stages.contains(&"MYLABEL".to_string()),
        "v1 should have MYLABEL stage, got: {v1_stages:?}"
    );

    // v2 should still have AWSCURRENT
    let v2_stages = stages.get(&v2_id).expect("v2 should have stages");
    assert!(
        v2_stages.contains(&"AWSCURRENT".to_string()),
        "v2 should still have AWSCURRENT"
    );
}
