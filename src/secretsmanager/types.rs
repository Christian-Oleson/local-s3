use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Stored on disk: metadata for a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub name: String,
    pub arn: String,
    pub description: Option<String>,
    pub kms_key_id: Option<String>,
    pub tags: Vec<Tag>,
    pub created_date: f64,
    pub last_changed_date: f64,
    pub last_accessed_date: Option<f64>,
    pub deleted_date: Option<f64>,
    pub version_ids_to_stages: HashMap<String, Vec<String>>,
}

/// Stored on disk: a single version of a secret's value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretVersion {
    pub version_id: String,
    pub secret_string: Option<String>,
    pub secret_binary: Option<String>,
    pub version_stages: Vec<String>,
    pub created_date: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

// ---------------------------------------------------------------------------
// API request / response types (PascalCase for AWS JSON protocol)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "SecretString")]
    pub secret_string: Option<String>,
    #[serde(rename = "SecretBinary")]
    pub secret_binary: Option<String>,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "KmsKeyId")]
    pub kms_key_id: Option<String>,
    #[serde(rename = "Tags")]
    pub tags: Option<Vec<Tag>>,
    #[serde(rename = "ClientRequestToken")]
    pub client_request_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "VersionId")]
    pub version_id: String,
}

#[derive(Debug, Deserialize)]
pub struct GetSecretValueRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "VersionId")]
    pub version_id: Option<String>,
    #[serde(rename = "VersionStage")]
    pub version_stage: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetSecretValueResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "VersionId")]
    pub version_id: String,
    #[serde(rename = "SecretString", skip_serializing_if = "Option::is_none")]
    pub secret_string: Option<String>,
    #[serde(rename = "SecretBinary", skip_serializing_if = "Option::is_none")]
    pub secret_binary: Option<String>,
    #[serde(rename = "VersionStages")]
    pub version_stages: Vec<String>,
    #[serde(rename = "CreatedDate")]
    pub created_date: f64,
}

#[derive(Debug, Deserialize)]
pub struct PutSecretValueRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "SecretString")]
    pub secret_string: Option<String>,
    #[serde(rename = "SecretBinary")]
    pub secret_binary: Option<String>,
    #[serde(rename = "ClientRequestToken")]
    pub client_request_token: Option<String>,
    #[serde(rename = "VersionStages")]
    pub version_stages: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct PutSecretValueResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "VersionId")]
    pub version_id: String,
    #[serde(rename = "VersionStages")]
    pub version_stages: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "RecoveryWindowInDays")]
    pub recovery_window_in_days: Option<i64>,
    #[serde(rename = "ForceDeleteWithoutRecovery")]
    pub force_delete_without_recovery: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DeleteSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "DeletionDate")]
    pub deletion_date: f64,
}

#[derive(Debug, Deserialize)]
pub struct RestoreSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
}

#[derive(Debug, Serialize)]
pub struct RestoreSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
}
