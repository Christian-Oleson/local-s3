use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use tokio::fs;
use uuid::Uuid;

use super::error::SmError;
use super::types::{
    CancelRotateSecretRequest, CancelRotateSecretResponse, CreateSecretRequest,
    CreateSecretResponse, DeleteResourcePolicyRequest, DeleteResourcePolicyResponse,
    DeleteSecretRequest, DeleteSecretResponse, DescribeSecretRequest, DescribeSecretResponse,
    GetResourcePolicyRequest, GetResourcePolicyResponse, GetSecretValueRequest,
    GetSecretValueResponse, ListSecretVersionIdsRequest, ListSecretVersionIdsResponse,
    ListSecretsRequest, ListSecretsResponse, PutResourcePolicyRequest, PutResourcePolicyResponse,
    PutSecretValueRequest, PutSecretValueResponse, RestoreSecretRequest, RestoreSecretResponse,
    RotateSecretRequest, RotateSecretResponse, SecretFilter, SecretListEntry, SecretMetadata,
    SecretVersion, SecretVersionEntry, TagResourceRequest, UntagResourceRequest,
    UpdateSecretRequest, UpdateSecretResponse, UpdateSecretVersionStageRequest,
    UpdateSecretVersionStageResponse,
};

pub struct SecretsStorage {
    root_dir: PathBuf,
    region: String,
    account_id: String,
}

impl SecretsStorage {
    pub async fn new(data_dir: PathBuf) -> Result<Self, std::io::Error> {
        let root_dir = data_dir.join(".secrets-manager");
        fs::create_dir_all(root_dir.join("secrets")).await?;
        Ok(Self {
            root_dir,
            region: "us-east-1".to_string(),
            account_id: "000000000000".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    pub async fn create_secret(
        &self,
        req: CreateSecretRequest,
    ) -> Result<CreateSecretResponse, SmError> {
        let dir = self.secret_dir(&req.name);

        // Check for duplicate
        if dir.join("metadata.json").exists() {
            return Err(SmError::ResourceExistsException {
                message: format!(
                    "The operation failed because the secret {} already exists.",
                    req.name
                ),
            });
        }

        let arn = self.generate_arn(&req.name);
        let now = epoch_now();
        let version_id = req
            .client_request_token
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut version_ids_to_stages: HashMap<String, Vec<String>> = HashMap::new();

        // Only create a version if there is actual secret content
        let has_content = req.secret_string.is_some() || req.secret_binary.is_some();
        if has_content {
            version_ids_to_stages.insert(version_id.clone(), vec!["AWSCURRENT".to_string()]);
        }

        let metadata = SecretMetadata {
            name: req.name.clone(),
            arn: arn.clone(),
            description: req.description,
            kms_key_id: req.kms_key_id,
            tags: req.tags.unwrap_or_default(),
            created_date: now,
            last_changed_date: now,
            last_accessed_date: None,
            deleted_date: None,
            version_ids_to_stages,
            rotation_enabled: false,
            rotation_lambda_arn: None,
            rotation_rules: None,
            last_rotated_date: None,
        };

        // Write metadata
        fs::create_dir_all(dir.join("versions"))
            .await
            .map_err(io_err)?;
        self.write_metadata(&metadata).await?;

        // Write initial version if content was provided
        if has_content {
            let version = SecretVersion {
                version_id: version_id.clone(),
                secret_string: req.secret_string,
                secret_binary: req.secret_binary,
                version_stages: vec!["AWSCURRENT".to_string()],
                created_date: now,
            };
            self.write_version(&req.name, &version).await?;
        }

        Ok(CreateSecretResponse {
            arn,
            name: req.name,
            version_id,
        })
    }

    pub async fn get_secret_value(
        &self,
        req: GetSecretValueRequest,
    ) -> Result<GetSecretValueResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        // Cannot get value of a deleted (pending deletion) secret
        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        // Determine which version to return
        let version_id = if let Some(ref vid) = req.version_id {
            vid.clone()
        } else {
            let stage = req.version_stage.as_deref().unwrap_or("AWSCURRENT");
            self.find_version_by_stage(&metadata, stage)?
        };

        let version = self.read_version(&metadata.name, &version_id).await?;

        // Update last_accessed_date
        let mut meta = metadata;
        meta.last_accessed_date = Some(epoch_now());
        let _ = self.write_metadata(&meta).await;

        Ok(GetSecretValueResponse {
            arn: meta.arn,
            name: meta.name,
            version_id: version.version_id,
            secret_string: version.secret_string,
            secret_binary: version.secret_binary,
            version_stages: version.version_stages,
            created_date: version.created_date,
        })
    }

    pub async fn put_secret_value(
        &self,
        req: PutSecretValueRequest,
    ) -> Result<PutSecretValueResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        let now = epoch_now();
        let version_id = req
            .client_request_token
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let version_stages = req
            .version_stages
            .unwrap_or_else(|| vec!["AWSCURRENT".to_string()]);

        let new_has_current = version_stages.contains(&"AWSCURRENT".to_string());

        // Version rotation: if new version gets AWSCURRENT, rotate labels
        if new_has_current {
            // Find the old AWSCURRENT version and move it to AWSPREVIOUS
            let old_current_vid = self.find_version_id_by_stage(&metadata, "AWSCURRENT");
            let old_previous_vid = self.find_version_id_by_stage(&metadata, "AWSPREVIOUS");

            // Remove AWSPREVIOUS from old previous version
            if let Some(ref prev_vid) = old_previous_vid {
                if let Some(stages) = metadata.version_ids_to_stages.get_mut(prev_vid) {
                    stages.retain(|s| s != "AWSPREVIOUS");
                    if stages.is_empty() {
                        metadata.version_ids_to_stages.remove(prev_vid);
                    }
                }
                // Update the version file on disk
                if let Ok(mut ver) = self.read_version(&metadata.name, prev_vid).await {
                    ver.version_stages.retain(|s| s != "AWSPREVIOUS");
                    let _ = self.write_version(&metadata.name, &ver).await;
                }
            }

            // Move old AWSCURRENT to AWSPREVIOUS
            if let Some(ref cur_vid) = old_current_vid {
                if let Some(stages) = metadata.version_ids_to_stages.get_mut(cur_vid) {
                    stages.retain(|s| s != "AWSCURRENT");
                    if !stages.contains(&"AWSPREVIOUS".to_string()) {
                        stages.push("AWSPREVIOUS".to_string());
                    }
                }
                // Update version file on disk
                if let Ok(mut ver) = self.read_version(&metadata.name, cur_vid).await {
                    ver.version_stages.retain(|s| s != "AWSCURRENT");
                    if !ver.version_stages.contains(&"AWSPREVIOUS".to_string()) {
                        ver.version_stages.push("AWSPREVIOUS".to_string());
                    }
                    let _ = self.write_version(&metadata.name, &ver).await;
                }
            }
        }

        // Create the new version
        let version = SecretVersion {
            version_id: version_id.clone(),
            secret_string: req.secret_string,
            secret_binary: req.secret_binary,
            version_stages: version_stages.clone(),
            created_date: now,
        };

        self.write_version(&metadata.name, &version).await?;

        // Update metadata
        metadata
            .version_ids_to_stages
            .insert(version_id.clone(), version_stages.clone());
        metadata.last_changed_date = now;
        self.write_metadata(&metadata).await?;

        Ok(PutSecretValueResponse {
            arn: metadata.arn,
            name: metadata.name,
            version_id,
            version_stages,
        })
    }

    pub async fn delete_secret(
        &self,
        req: DeleteSecretRequest,
    ) -> Result<DeleteSecretResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was already marked for deletion.".to_string(),
            });
        }

        let force = req.force_delete_without_recovery.unwrap_or(false);

        if force {
            // Immediate deletion: remove the directory
            let dir = self.secret_dir(&metadata.name);
            if dir.exists() {
                fs::remove_dir_all(&dir).await.map_err(io_err)?;
            }
            let now = epoch_now();
            return Ok(DeleteSecretResponse {
                arn: metadata.arn,
                name: metadata.name,
                deletion_date: now,
            });
        }

        // Scheduled deletion
        let window_days = req.recovery_window_in_days.unwrap_or(30);
        let now = epoch_now();
        let deletion_date = now + (window_days as f64) * 86400.0;

        metadata.deleted_date = Some(deletion_date);
        metadata.last_changed_date = now;
        self.write_metadata(&metadata).await?;

        Ok(DeleteSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
            deletion_date,
        })
    }

    pub async fn restore_secret(
        &self,
        req: RestoreSecretRequest,
    ) -> Result<RestoreSecretResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_none() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was not marked for deletion.".to_string(),
            });
        }

        metadata.deleted_date = None;
        metadata.last_changed_date = epoch_now();
        self.write_metadata(&metadata).await?;

        Ok(RestoreSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
        })
    }

    pub async fn describe_secret(
        &self,
        req: DescribeSecretRequest,
    ) -> Result<DescribeSecretResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        // Build VersionIdsToStages from version files on disk
        let version_ids_to_stages = self.build_version_ids_to_stages(&metadata.name).await?;

        Ok(DescribeSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
            description: metadata.description,
            kms_key_id: metadata.kms_key_id,
            rotation_enabled: metadata.rotation_enabled,
            tags: metadata.tags,
            version_ids_to_stages,
            created_date: metadata.created_date,
            last_changed_date: metadata.last_changed_date,
            last_accessed_date: metadata.last_accessed_date,
            deleted_date: metadata.deleted_date,
        })
    }

    pub async fn list_secrets(
        &self,
        req: ListSecretsRequest,
    ) -> Result<ListSecretsResponse, SmError> {
        let max_results = req.max_results.unwrap_or(100).min(100);
        let include_planned_deletion = req.include_planned_deletion.unwrap_or(false);
        let filters = req.filters.unwrap_or_default();

        // Decode the pagination token (base64-encoded secret name to start after)
        let start_after = req
            .next_token
            .as_deref()
            .and_then(super::types::decode_next_token);

        // Scan all secret directories
        let secrets_dir = self.root_dir.join("secrets");
        let mut entries = Vec::new();

        if let Ok(mut dir) = fs::read_dir(&secrets_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let meta_path = entry.path().join("metadata.json");
                if !meta_path.exists() {
                    continue;
                }
                if let Ok(metadata) = self.read_metadata_from_path(&meta_path).await {
                    // Exclude deleted secrets unless include_planned_deletion
                    if metadata.deleted_date.is_some() && !include_planned_deletion {
                        continue;
                    }
                    // Apply filters
                    if !Self::matches_filters(&metadata, &filters) {
                        continue;
                    }
                    entries.push(metadata);
                }
            }
        }

        // Sort by name ascending
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        // Apply pagination: skip entries until we pass start_after
        if let Some(ref after) = start_after {
            entries.retain(|m| m.name.as_str() > after.as_str());
        }

        // Take max_results + 1 to detect if there's a next page
        let has_more = entries.len() > max_results;
        entries.truncate(max_results);

        let next_token = if has_more {
            entries
                .last()
                .map(|m| super::types::encode_next_token(&m.name))
        } else {
            None
        };

        // Build the response list
        let mut secret_list = Vec::with_capacity(entries.len());
        for metadata in entries {
            let version_ids_to_stages = self.build_version_ids_to_stages(&metadata.name).await?;
            secret_list.push(SecretListEntry {
                arn: metadata.arn,
                name: metadata.name,
                description: metadata.description,
                kms_key_id: metadata.kms_key_id,
                rotation_enabled: metadata.rotation_enabled,
                tags: metadata.tags,
                secret_versions_to_stages: version_ids_to_stages,
                created_date: metadata.created_date,
                last_changed_date: metadata.last_changed_date,
                last_accessed_date: metadata.last_accessed_date,
                deleted_date: metadata.deleted_date,
            });
        }

        Ok(ListSecretsResponse {
            secret_list,
            next_token,
        })
    }

    pub async fn update_secret(
        &self,
        req: UpdateSecretRequest,
    ) -> Result<UpdateSecretResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        let now = epoch_now();

        // Update metadata fields if provided
        if let Some(ref desc) = req.description {
            metadata.description = Some(desc.clone());
            metadata.last_changed_date = now;
        }
        if let Some(ref kms) = req.kms_key_id {
            metadata.kms_key_id = Some(kms.clone());
            metadata.last_changed_date = now;
        }

        // If secret value is provided, create a new version via put_secret_value logic
        let version_id = if req.secret_string.is_some() || req.secret_binary.is_some() {
            // Write updated metadata first (description/kms changes)
            self.write_metadata(&metadata).await?;

            let put_req = PutSecretValueRequest {
                secret_id: metadata.name.clone(),
                secret_string: req.secret_string,
                secret_binary: req.secret_binary,
                client_request_token: req.client_request_token,
                version_stages: None,
            };
            let put_resp = self.put_secret_value(put_req).await?;
            Some(put_resp.version_id)
        } else {
            // No new value -- just persist metadata changes
            self.write_metadata(&metadata).await?;
            None
        };

        Ok(UpdateSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
            version_id,
        })
    }

    pub async fn list_secret_version_ids(
        &self,
        req: ListSecretVersionIdsRequest,
    ) -> Result<ListSecretVersionIdsResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        let max_results = req.max_results.unwrap_or(100).min(100);
        let include_deprecated = req.include_deprecated.unwrap_or(false);

        // Read all version files
        let versions_dir = self.secret_dir(&metadata.name).join("versions");
        let mut versions = Vec::new();

        if let Ok(mut dir) = fs::read_dir(&versions_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let data = fs::read_to_string(&path).await.map_err(io_err)?;
                let ver: SecretVersion =
                    serde_json::from_str(&data).map_err(|e| SmError::InternalServiceError {
                        message: format!("Failed to deserialize version: {e}"),
                    })?;

                // Filter deprecated (no staging labels) unless include_deprecated
                if !include_deprecated && ver.version_stages.is_empty() {
                    continue;
                }

                versions.push(SecretVersionEntry {
                    version_id: ver.version_id,
                    version_stages: ver.version_stages,
                    created_date: ver.created_date,
                });
            }
        }

        // Sort by created_date descending
        versions.sort_by(|a, b| {
            b.created_date
                .partial_cmp(&a.created_date)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Pagination
        let start_after = req
            .next_token
            .as_deref()
            .and_then(super::types::decode_next_token);

        if let Some(ref after) = start_after
            && let Some(pos) = versions.iter().position(|v| v.version_id == *after)
        {
            versions = versions.split_off(pos + 1);
        }

        let has_more = versions.len() > max_results;
        versions.truncate(max_results);

        let next_token = if has_more {
            versions
                .last()
                .map(|v| super::types::encode_next_token(&v.version_id))
        } else {
            None
        };

        Ok(ListSecretVersionIdsResponse {
            arn: metadata.arn,
            name: metadata.name,
            versions,
            next_token,
        })
    }

    // -----------------------------------------------------------------------
    // TagResource / UntagResource
    // -----------------------------------------------------------------------

    pub async fn tag_resource(&self, req: TagResourceRequest) -> Result<(), SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        // Merge tags: overwrite on key match, add new ones
        for new_tag in req.tags {
            if let Some(existing) = metadata.tags.iter_mut().find(|t| t.key == new_tag.key) {
                existing.value = new_tag.value;
            } else {
                metadata.tags.push(new_tag);
            }
        }

        metadata.last_changed_date = epoch_now();
        self.write_metadata(&metadata).await?;
        Ok(())
    }

    pub async fn untag_resource(&self, req: UntagResourceRequest) -> Result<(), SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        metadata.tags.retain(|t| !req.tag_keys.contains(&t.key));

        metadata.last_changed_date = epoch_now();
        self.write_metadata(&metadata).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // PutResourcePolicy / GetResourcePolicy / DeleteResourcePolicy
    // -----------------------------------------------------------------------

    pub async fn put_resource_policy(
        &self,
        req: PutResourcePolicyRequest,
    ) -> Result<PutResourcePolicyResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        let policy_path = self.secret_dir(&metadata.name).join("policy.json");
        fs::write(&policy_path, &req.resource_policy)
            .await
            .map_err(io_err)?;

        Ok(PutResourcePolicyResponse {
            arn: metadata.arn,
            name: metadata.name,
        })
    }

    pub async fn get_resource_policy(
        &self,
        req: GetResourcePolicyRequest,
    ) -> Result<GetResourcePolicyResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        let policy_path = self.secret_dir(&metadata.name).join("policy.json");
        let resource_policy = if policy_path.exists() {
            Some(fs::read_to_string(&policy_path).await.map_err(io_err)?)
        } else {
            None
        };

        Ok(GetResourcePolicyResponse {
            arn: metadata.arn,
            name: metadata.name,
            resource_policy,
        })
    }

    pub async fn delete_resource_policy(
        &self,
        req: DeleteResourcePolicyRequest,
    ) -> Result<DeleteResourcePolicyResponse, SmError> {
        let metadata = self.resolve_secret(&req.secret_id).await?;

        let policy_path = self.secret_dir(&metadata.name).join("policy.json");
        if policy_path.exists() {
            fs::remove_file(&policy_path).await.map_err(io_err)?;
        }

        Ok(DeleteResourcePolicyResponse {
            arn: metadata.arn,
            name: metadata.name,
        })
    }

    // -----------------------------------------------------------------------
    // RotateSecret / CancelRotateSecret
    // -----------------------------------------------------------------------

    pub async fn rotate_secret(
        &self,
        req: RotateSecretRequest,
    ) -> Result<RotateSecretResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        let now = epoch_now();
        let version_id = req
            .client_request_token
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // Update rotation configuration
        metadata.rotation_enabled = true;
        if let Some(lambda_arn) = req.rotation_lambda_arn {
            metadata.rotation_lambda_arn = Some(lambda_arn);
        }
        if let Some(rules) = req.rotation_rules {
            metadata.rotation_rules = Some(rules);
        }
        metadata.last_rotated_date = Some(now);
        metadata.last_changed_date = now;

        // Create a new version with AWSPENDING label
        let version = SecretVersion {
            version_id: version_id.clone(),
            secret_string: None,
            secret_binary: None,
            version_stages: vec!["AWSPENDING".to_string()],
            created_date: now,
        };
        self.write_version(&metadata.name, &version).await?;

        // Update metadata version stages
        metadata
            .version_ids_to_stages
            .insert(version_id.clone(), vec!["AWSPENDING".to_string()]);
        self.write_metadata(&metadata).await?;

        Ok(RotateSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
            version_id,
        })
    }

    pub async fn cancel_rotate_secret(
        &self,
        req: CancelRotateSecretRequest,
    ) -> Result<CancelRotateSecretResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        metadata.rotation_enabled = false;
        metadata.last_changed_date = epoch_now();

        // Remove AWSPENDING label from any version that has it
        let pending_vids: Vec<String> = metadata
            .version_ids_to_stages
            .iter()
            .filter(|(_, stages)| stages.contains(&"AWSPENDING".to_string()))
            .map(|(vid, _)| vid.clone())
            .collect();

        for vid in &pending_vids {
            if let Some(stages) = metadata.version_ids_to_stages.get_mut(vid) {
                stages.retain(|s| s != "AWSPENDING");
                if stages.is_empty() {
                    metadata.version_ids_to_stages.remove(vid);
                }
            }
            // Update the version file on disk
            if let Ok(mut ver) = self.read_version(&metadata.name, vid).await {
                ver.version_stages.retain(|s| s != "AWSPENDING");
                let _ = self.write_version(&metadata.name, &ver).await;
            }
        }

        self.write_metadata(&metadata).await?;

        Ok(CancelRotateSecretResponse {
            arn: metadata.arn,
            name: metadata.name,
        })
    }

    // -----------------------------------------------------------------------
    // UpdateSecretVersionStage
    // -----------------------------------------------------------------------

    pub async fn update_secret_version_stage(
        &self,
        req: UpdateSecretVersionStageRequest,
    ) -> Result<UpdateSecretVersionStageResponse, SmError> {
        let mut metadata = self.resolve_secret(&req.secret_id).await?;

        if metadata.deleted_date.is_some() {
            return Err(SmError::InvalidRequestException {
                message: "You can't perform this operation on the secret because it was marked for deletion.".to_string(),
            });
        }

        let now = epoch_now();

        // Remove the stage from the source version
        if let Some(ref remove_vid) = req.remove_from_version_id {
            if let Some(stages) = metadata.version_ids_to_stages.get_mut(remove_vid) {
                stages.retain(|s| s != &req.version_stage);
                if stages.is_empty() {
                    metadata.version_ids_to_stages.remove(remove_vid);
                }
            }
            // Update the version file on disk
            if let Ok(mut ver) = self.read_version(&metadata.name, remove_vid).await {
                ver.version_stages.retain(|s| s != &req.version_stage);
                let _ = self.write_version(&metadata.name, &ver).await;
            }
        }

        // Add the stage to the target version
        if let Some(ref move_vid) = req.move_to_version_id {
            // Verify the version exists on disk
            let _ver = self.read_version(&metadata.name, move_vid).await?;

            let stages = metadata
                .version_ids_to_stages
                .entry(move_vid.clone())
                .or_default();
            if !stages.contains(&req.version_stage) {
                stages.push(req.version_stage.clone());
            }

            // Update the version file on disk
            if let Ok(mut ver) = self.read_version(&metadata.name, move_vid).await {
                if !ver.version_stages.contains(&req.version_stage) {
                    ver.version_stages.push(req.version_stage.clone());
                }
                let _ = self.write_version(&metadata.name, &ver).await;
            }
        }

        metadata.last_changed_date = now;
        self.write_metadata(&metadata).await?;

        Ok(UpdateSecretVersionStageResponse {
            arn: metadata.arn,
            name: metadata.name,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Resolve a secret identifier (name or ARN) to its metadata.
    async fn resolve_secret(&self, id: &str) -> Result<SecretMetadata, SmError> {
        // Try as a direct name first
        let dir = self.secret_dir(id);
        let meta_path = dir.join("metadata.json");
        if meta_path.exists() {
            return self.read_metadata_from_path(&meta_path).await;
        }

        // Try by scanning for ARN match
        if id.starts_with("arn:") {
            let secrets_dir = self.root_dir.join("secrets");
            if let Ok(mut entries) = fs::read_dir(&secrets_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let mp = entry.path().join("metadata.json");
                    if mp.exists()
                        && let Ok(meta) = self.read_metadata_from_path(&mp).await
                        && meta.arn == id
                    {
                        return Ok(meta);
                    }
                }
            }
        }

        Err(SmError::ResourceNotFoundException {
            message: "Secrets Manager can't find the specified secret.".to_string(),
        })
    }

    fn find_version_by_stage(
        &self,
        metadata: &SecretMetadata,
        stage: &str,
    ) -> Result<String, SmError> {
        for (vid, stages) in &metadata.version_ids_to_stages {
            if stages.iter().any(|s| s == stage) {
                return Ok(vid.clone());
            }
        }
        Err(SmError::ResourceNotFoundException {
            message: format!(
                "Secrets Manager can't find the specified secret value for staging label: {stage}"
            ),
        })
    }

    fn find_version_id_by_stage(&self, metadata: &SecretMetadata, stage: &str) -> Option<String> {
        for (vid, stages) in &metadata.version_ids_to_stages {
            if stages.iter().any(|s| s == stage) {
                return Some(vid.clone());
            }
        }
        None
    }

    /// Check whether a secret's metadata matches all of the given filters.
    fn matches_filters(metadata: &SecretMetadata, filters: &[SecretFilter]) -> bool {
        for filter in filters {
            let matched = match filter.key.as_str() {
                "name" => filter
                    .values
                    .iter()
                    .any(|v| metadata.name.contains(v.as_str())),
                "description" => filter.values.iter().any(|v| {
                    metadata
                        .description
                        .as_ref()
                        .is_some_and(|d| d.contains(v.as_str()))
                }),
                "tag-key" => filter
                    .values
                    .iter()
                    .any(|v| metadata.tags.iter().any(|t| t.key == *v)),
                "tag-value" => filter
                    .values
                    .iter()
                    .any(|v| metadata.tags.iter().any(|t| t.value == *v)),
                "all" => filter.values.iter().any(|v| {
                    metadata.name.contains(v.as_str())
                        || metadata
                            .description
                            .as_ref()
                            .is_some_and(|d| d.contains(v.as_str()))
                        || metadata
                            .tags
                            .iter()
                            .any(|t| t.key.contains(v.as_str()) || t.value.contains(v.as_str()))
                }),
                _ => true, // Unknown filter key — ignore
            };
            if !matched {
                return false;
            }
        }
        true
    }

    /// Read all version files for a secret and build VersionId -> [stages] map.
    async fn build_version_ids_to_stages(
        &self,
        name: &str,
    ) -> Result<HashMap<String, Vec<String>>, SmError> {
        let versions_dir = self.secret_dir(name).join("versions");
        let mut map: HashMap<String, Vec<String>> = HashMap::new();

        if let Ok(mut dir) = fs::read_dir(&versions_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let data = fs::read_to_string(&path).await.map_err(io_err)?;
                let ver: SecretVersion =
                    serde_json::from_str(&data).map_err(|e| SmError::InternalServiceError {
                        message: format!("Failed to deserialize version: {e}"),
                    })?;
                if !ver.version_stages.is_empty() {
                    map.insert(ver.version_id, ver.version_stages);
                }
            }
        }

        Ok(map)
    }

    fn generate_arn(&self, name: &str) -> String {
        let suffix = random_alphanum_6();
        format!(
            "arn:aws:secretsmanager:{}:{}:secret:{}-{}",
            self.region, self.account_id, name, suffix
        )
    }

    fn secret_dir(&self, name: &str) -> PathBuf {
        self.root_dir.join("secrets").join(encode_name(name))
    }

    async fn write_metadata(&self, metadata: &SecretMetadata) -> Result<(), SmError> {
        let dir = self.secret_dir(&metadata.name);
        let json =
            serde_json::to_string_pretty(metadata).map_err(|e| SmError::InternalServiceError {
                message: format!("Failed to serialize metadata: {e}"),
            })?;
        fs::write(dir.join("metadata.json"), json)
            .await
            .map_err(io_err)
    }

    async fn read_metadata_from_path(&self, path: &PathBuf) -> Result<SecretMetadata, SmError> {
        let data = fs::read_to_string(path).await.map_err(io_err)?;
        serde_json::from_str(&data).map_err(|e| SmError::InternalServiceError {
            message: format!("Failed to deserialize metadata: {e}"),
        })
    }

    async fn write_version(&self, name: &str, version: &SecretVersion) -> Result<(), SmError> {
        let dir = self.secret_dir(name);
        let json =
            serde_json::to_string_pretty(version).map_err(|e| SmError::InternalServiceError {
                message: format!("Failed to serialize version: {e}"),
            })?;
        fs::write(
            dir.join("versions")
                .join(format!("{}.json", version.version_id)),
            json,
        )
        .await
        .map_err(io_err)
    }

    async fn read_version(&self, name: &str, version_id: &str) -> Result<SecretVersion, SmError> {
        let path = self
            .secret_dir(name)
            .join("versions")
            .join(format!("{version_id}.json"));
        let data = fs::read_to_string(&path).await.map_err(|_| {
            SmError::ResourceNotFoundException {
                message: format!("Secrets Manager can't find the specified secret value for VersionId: {version_id}"),
            }
        })?;
        serde_json::from_str(&data).map_err(|e| SmError::InternalServiceError {
            message: format!("Failed to deserialize version: {e}"),
        })
    }
}

/// Replace "/" with "%2F" for filesystem path safety.
fn encode_name(name: &str) -> String {
    name.replace('/', "%2F")
}

/// Current time as f64 epoch seconds.
fn epoch_now() -> f64 {
    Utc::now().timestamp() as f64
}

/// Generate 6 random alphanumeric characters using uuid bytes.
fn random_alphanum_6() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let bytes = Uuid::new_v4();
    let raw = bytes.as_bytes();
    (0..6)
        .map(|i| {
            let idx = raw[i] as usize % CHARSET.len();
            CHARSET[idx] as char
        })
        .collect()
}

fn io_err(e: std::io::Error) -> SmError {
    SmError::InternalServiceError {
        message: format!("I/O error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_storage() -> (SecretsStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = SecretsStorage::new(tmp.path().to_path_buf()).await.unwrap();
        (storage, tmp)
    }

    fn make_create_req(name: &str, value: &str) -> CreateSecretRequest {
        CreateSecretRequest {
            name: name.to_string(),
            secret_string: Some(value.to_string()),
            secret_binary: None,
            description: Some("test secret".to_string()),
            kms_key_id: None,
            tags: None,
            client_request_token: None,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_round_trip() {
        let (storage, _tmp) = test_storage().await;

        let resp = storage
            .create_secret(make_create_req("my/secret", "hunter2"))
            .await
            .unwrap();

        assert_eq!(resp.name, "my/secret");
        assert!(
            resp.arn
                .starts_with("arn:aws:secretsmanager:us-east-1:000000000000:secret:my/secret-")
        );
        assert!(!resp.version_id.is_empty());

        // Get by name
        let get_resp = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "my/secret".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap();

        assert_eq!(get_resp.name, "my/secret");
        assert_eq!(get_resp.secret_string.as_deref(), Some("hunter2"));
        assert_eq!(get_resp.version_stages, vec!["AWSCURRENT"]);
        assert_eq!(get_resp.arn, resp.arn);
    }

    #[tokio::test]
    async fn test_get_by_arn() {
        let (storage, _tmp) = test_storage().await;

        let resp = storage
            .create_secret(make_create_req("arn-test", "value1"))
            .await
            .unwrap();

        let get_resp = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: resp.arn.clone(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap();

        assert_eq!(get_resp.name, "arn-test");
        assert_eq!(get_resp.secret_string.as_deref(), Some("value1"));
    }

    #[tokio::test]
    async fn test_put_secret_value_version_rotation() {
        let (storage, _tmp) = test_storage().await;

        // Create initial secret
        let create_resp = storage
            .create_secret(make_create_req("rotate-test", "v1"))
            .await
            .unwrap();
        let v1_id = create_resp.version_id;

        // Put new value (v2 -> AWSCURRENT, v1 -> AWSPREVIOUS)
        let put_resp = storage
            .put_secret_value(PutSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                secret_string: Some("v2".to_string()),
                secret_binary: None,
                client_request_token: None,
                version_stages: None,
            })
            .await
            .unwrap();
        let v2_id = put_resp.version_id;
        assert_eq!(put_resp.version_stages, vec!["AWSCURRENT"]);

        // Get AWSCURRENT -> v2
        let current = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                version_id: None,
                version_stage: Some("AWSCURRENT".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(current.secret_string.as_deref(), Some("v2"));
        assert_eq!(current.version_id, v2_id);

        // Get AWSPREVIOUS -> v1
        let previous = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                version_id: None,
                version_stage: Some("AWSPREVIOUS".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(previous.secret_string.as_deref(), Some("v1"));
        assert_eq!(previous.version_id, v1_id);

        // Put v3 -> AWSCURRENT, v2 -> AWSPREVIOUS, v1 -> no labels (deprecated)
        let put_resp3 = storage
            .put_secret_value(PutSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                secret_string: Some("v3".to_string()),
                secret_binary: None,
                client_request_token: None,
                version_stages: None,
            })
            .await
            .unwrap();
        let v3_id = put_resp3.version_id;

        let current3 = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                version_id: None,
                version_stage: Some("AWSCURRENT".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(current3.secret_string.as_deref(), Some("v3"));
        assert_eq!(current3.version_id, v3_id);

        let prev3 = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                version_id: None,
                version_stage: Some("AWSPREVIOUS".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(prev3.secret_string.as_deref(), Some("v2"));
        assert_eq!(prev3.version_id, v2_id);

        // v1 should still be retrievable by version_id
        let v1_direct = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "rotate-test".to_string(),
                version_id: Some(v1_id.clone()),
                version_stage: None,
            })
            .await
            .unwrap();
        assert_eq!(v1_direct.secret_string.as_deref(), Some("v1"));
    }

    #[tokio::test]
    async fn test_force_delete_then_get_fails() {
        let (storage, _tmp) = test_storage().await;

        storage
            .create_secret(make_create_req("del-test", "secret"))
            .await
            .unwrap();

        let del_resp = storage
            .delete_secret(DeleteSecretRequest {
                secret_id: "del-test".to_string(),
                recovery_window_in_days: None,
                force_delete_without_recovery: Some(true),
            })
            .await
            .unwrap();

        assert_eq!(del_resp.name, "del-test");

        // Get should now fail with ResourceNotFoundException
        let err = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "del-test".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SmError::ResourceNotFoundException { .. }));
    }

    #[tokio::test]
    async fn test_scheduled_delete_and_restore() {
        let (storage, _tmp) = test_storage().await;

        storage
            .create_secret(make_create_req("restore-test", "val"))
            .await
            .unwrap();

        // Schedule deletion
        let del_resp = storage
            .delete_secret(DeleteSecretRequest {
                secret_id: "restore-test".to_string(),
                recovery_window_in_days: Some(7),
                force_delete_without_recovery: None,
            })
            .await
            .unwrap();

        assert_eq!(del_resp.name, "restore-test");
        assert!(del_resp.deletion_date > epoch_now());

        // Get should fail with InvalidRequestException (marked for deletion)
        let err = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "restore-test".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, SmError::InvalidRequestException { .. }));

        // Restore
        let restore_resp = storage
            .restore_secret(RestoreSecretRequest {
                secret_id: "restore-test".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(restore_resp.name, "restore-test");

        // Now get should work again
        let get_resp = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "restore-test".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap();
        assert_eq!(get_resp.secret_string.as_deref(), Some("val"));
    }

    #[tokio::test]
    async fn test_create_duplicate_fails() {
        let (storage, _tmp) = test_storage().await;

        storage
            .create_secret(make_create_req("dup", "first"))
            .await
            .unwrap();

        let err = storage
            .create_secret(make_create_req("dup", "second"))
            .await
            .unwrap_err();

        assert!(matches!(err, SmError::ResourceExistsException { .. }));
    }

    #[tokio::test]
    async fn test_get_nonexistent_fails() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "nope".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SmError::ResourceNotFoundException { .. }));
    }

    #[tokio::test]
    async fn test_get_deleted_secret_returns_invalid_request() {
        let (storage, _tmp) = test_storage().await;

        storage
            .create_secret(make_create_req("del-ir", "val"))
            .await
            .unwrap();

        storage
            .delete_secret(DeleteSecretRequest {
                secret_id: "del-ir".to_string(),
                recovery_window_in_days: Some(30),
                force_delete_without_recovery: None,
            })
            .await
            .unwrap();

        let err = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "del-ir".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SmError::InvalidRequestException { .. }));
    }

    #[tokio::test]
    async fn test_create_with_binary() {
        let (storage, _tmp) = test_storage().await;

        let resp = storage
            .create_secret(CreateSecretRequest {
                name: "bin-secret".to_string(),
                secret_string: None,
                secret_binary: Some("aGVsbG8=".to_string()),
                description: None,
                kms_key_id: None,
                tags: None,
                client_request_token: None,
            })
            .await
            .unwrap();

        let get_resp = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "bin-secret".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap();

        assert!(get_resp.secret_string.is_none());
        assert_eq!(get_resp.secret_binary.as_deref(), Some("aGVsbG8="));
        assert_eq!(get_resp.version_id, resp.version_id);
    }

    #[tokio::test]
    async fn test_slash_in_name() {
        let (storage, _tmp) = test_storage().await;

        storage
            .create_secret(make_create_req("prod/db/password", "p@ss"))
            .await
            .unwrap();

        let get_resp = storage
            .get_secret_value(GetSecretValueRequest {
                secret_id: "prod/db/password".to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .unwrap();

        assert_eq!(get_resp.secret_string.as_deref(), Some("p@ss"));
    }
}
