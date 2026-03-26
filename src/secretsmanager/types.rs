use std::collections::HashMap;

use base64::Engine;
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
    #[serde(default)]
    pub rotation_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_lambda_arn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_rules: Option<RotationRulesInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rotated_date: Option<f64>,
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

// ---------------------------------------------------------------------------
// DescribeSecret
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DescribeSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
}

#[derive(Debug, Serialize)]
pub struct DescribeSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "KmsKeyId", skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(rename = "RotationEnabled")]
    pub rotation_enabled: bool,
    #[serde(rename = "Tags", skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    #[serde(rename = "VersionIdsToStages")]
    pub version_ids_to_stages: HashMap<String, Vec<String>>,
    #[serde(rename = "CreatedDate")]
    pub created_date: f64,
    #[serde(rename = "LastChangedDate")]
    pub last_changed_date: f64,
    #[serde(rename = "LastAccessedDate", skip_serializing_if = "Option::is_none")]
    pub last_accessed_date: Option<f64>,
    #[serde(rename = "DeletedDate", skip_serializing_if = "Option::is_none")]
    pub deleted_date: Option<f64>,
}

// ---------------------------------------------------------------------------
// ListSecrets
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListSecretsRequest {
    #[serde(rename = "MaxResults")]
    pub max_results: Option<usize>,
    #[serde(rename = "NextToken")]
    pub next_token: Option<String>,
    #[serde(rename = "Filters")]
    pub filters: Option<Vec<SecretFilter>>,
    #[serde(rename = "IncludePlannedDeletion")]
    pub include_planned_deletion: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SecretFilter {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Values")]
    pub values: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ListSecretsResponse {
    #[serde(rename = "SecretList")]
    pub secret_list: Vec<SecretListEntry>,
    #[serde(rename = "NextToken", skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SecretListEntry {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "KmsKeyId", skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(rename = "RotationEnabled")]
    pub rotation_enabled: bool,
    #[serde(rename = "Tags", skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    #[serde(rename = "SecretVersionsToStages")]
    pub secret_versions_to_stages: HashMap<String, Vec<String>>,
    #[serde(rename = "CreatedDate")]
    pub created_date: f64,
    #[serde(rename = "LastChangedDate")]
    pub last_changed_date: f64,
    #[serde(rename = "LastAccessedDate", skip_serializing_if = "Option::is_none")]
    pub last_accessed_date: Option<f64>,
    #[serde(rename = "DeletedDate", skip_serializing_if = "Option::is_none")]
    pub deleted_date: Option<f64>,
}

// ---------------------------------------------------------------------------
// UpdateSecret
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct UpdateSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "KmsKeyId")]
    pub kms_key_id: Option<String>,
    #[serde(rename = "SecretString")]
    pub secret_string: Option<String>,
    #[serde(rename = "SecretBinary")]
    pub secret_binary: Option<String>,
    #[serde(rename = "ClientRequestToken")]
    pub client_request_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "VersionId", skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

// ---------------------------------------------------------------------------
// ListSecretVersionIds
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListSecretVersionIdsRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "MaxResults")]
    pub max_results: Option<usize>,
    #[serde(rename = "NextToken")]
    pub next_token: Option<String>,
    #[serde(rename = "IncludeDeprecated")]
    pub include_deprecated: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListSecretVersionIdsResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Versions")]
    pub versions: Vec<SecretVersionEntry>,
    #[serde(rename = "NextToken", skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SecretVersionEntry {
    #[serde(rename = "VersionId")]
    pub version_id: String,
    #[serde(rename = "VersionStages")]
    pub version_stages: Vec<String>,
    #[serde(rename = "CreatedDate")]
    pub created_date: f64,
}

// ---------------------------------------------------------------------------
// TagResource / UntagResource
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TagResourceRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "Tags")]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct UntagResourceRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "TagKeys")]
    pub tag_keys: Vec<String>,
}

// ---------------------------------------------------------------------------
// PutResourcePolicy / GetResourcePolicy / DeleteResourcePolicy
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PutResourcePolicyRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "ResourcePolicy")]
    pub resource_policy: String,
    #[serde(rename = "BlockPublicPolicy", default)]
    pub block_public_policy: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PutResourcePolicyResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct GetResourcePolicyRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
}

#[derive(Debug, Serialize)]
pub struct GetResourcePolicyResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ResourcePolicy", skip_serializing_if = "Option::is_none")]
    pub resource_policy: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteResourcePolicyRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResourcePolicyResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
}

// ---------------------------------------------------------------------------
// RotateSecret / CancelRotateSecret
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationRulesInput {
    #[serde(
        rename = "AutomaticallyAfterDays",
        skip_serializing_if = "Option::is_none"
    )]
    pub automatically_after_days: Option<i64>,
    #[serde(rename = "Duration", skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(rename = "ScheduleExpression", skip_serializing_if = "Option::is_none")]
    pub schedule_expression: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RotateSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "ClientRequestToken")]
    pub client_request_token: Option<String>,
    #[serde(rename = "RotationLambdaARN")]
    pub rotation_lambda_arn: Option<String>,
    #[serde(rename = "RotationRules")]
    pub rotation_rules: Option<RotationRulesInput>,
}

#[derive(Debug, Serialize)]
pub struct RotateSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "VersionId")]
    pub version_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CancelRotateSecretRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
}

#[derive(Debug, Serialize)]
pub struct CancelRotateSecretResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
}

// ---------------------------------------------------------------------------
// UpdateSecretVersionStage
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct UpdateSecretVersionStageRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "VersionStage")]
    pub version_stage: String,
    #[serde(rename = "MoveToVersionId")]
    pub move_to_version_id: Option<String>,
    #[serde(rename = "RemoveFromVersionId")]
    pub remove_from_version_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSecretVersionStageResponse {
    #[serde(rename = "ARN")]
    pub arn: String,
    #[serde(rename = "Name")]
    pub name: String,
}

// ---------------------------------------------------------------------------
// BatchGetSecretValue
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct BatchGetSecretValueRequest {
    #[serde(rename = "SecretIdList")]
    pub secret_id_list: Vec<String>,
    #[serde(rename = "MaxResults")]
    pub max_results: Option<i32>,
    #[serde(rename = "NextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchGetSecretValueResponse {
    #[serde(rename = "SecretValues")]
    pub secret_values: Vec<BatchSecretValue>,
    #[serde(rename = "Errors")]
    pub errors: Vec<BatchSecretError>,
    #[serde(rename = "NextToken", skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchSecretValue {
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

#[derive(Debug, Serialize)]
pub struct BatchSecretError {
    #[serde(rename = "SecretId")]
    pub secret_id: String,
    #[serde(rename = "ErrorCode")]
    pub error_code: String,
    #[serde(rename = "Message")]
    pub message: String,
}

// ---------------------------------------------------------------------------
// ValidateResourcePolicy
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ValidateResourcePolicyRequest {
    #[serde(rename = "SecretId")]
    pub secret_id: Option<String>,
    #[serde(rename = "ResourcePolicy")]
    pub resource_policy: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateResourcePolicyResponse {
    #[serde(rename = "PolicyValidationPassed")]
    pub policy_validation_passed: bool,
    #[serde(rename = "ValidationErrors")]
    pub validation_errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize)]
pub struct ValidationError {
    #[serde(rename = "CheckName")]
    pub check_name: String,
    #[serde(rename = "ErrorMessage")]
    pub error_message: String,
}

// ---------------------------------------------------------------------------
// Pagination helpers
// ---------------------------------------------------------------------------

pub fn encode_next_token(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

pub fn decode_next_token(token: &str) -> Option<String> {
    base64::engine::general_purpose::STANDARD
        .decode(token)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}
