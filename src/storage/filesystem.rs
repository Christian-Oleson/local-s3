use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::error::S3Error;
use crate::types::bucket::Bucket;
use crate::types::object::ObjectMetadata;

/// Output of a list_objects call.
#[derive(Debug)]
pub struct ListObjectsOutput {
    pub objects: Vec<ObjectInfo>,
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

/// Summary info about a single object, used for listing.
#[derive(Debug)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartUploadState {
    pub key: String,
    pub upload_id: String,
    pub initiated: DateTime<Utc>,
    pub parts: HashMap<i32, PartInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub part_number: i32,
    pub etag: String,
    pub size: u64,
    pub last_modified: DateTime<Utc>,
}

/// Result of a delete_object call, used to convey versioning information.
#[derive(Debug)]
pub struct DeleteObjectResult {
    pub version_id: Option<String>,
    pub is_delete_marker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsRule {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub max_age_seconds: Option<i32>,
    pub expose_headers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FileSystemStorage {
    root_dir: PathBuf,
}

impl FileSystemStorage {
    pub async fn new(root_dir: PathBuf) -> Result<Self, S3Error> {
        fs::create_dir_all(&root_dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to create data directory: {e}"),
            })?;
        Ok(Self { root_dir })
    }

    fn bucket_path(&self, name: &str) -> PathBuf {
        self.root_dir.join(name)
    }

    fn bucket_metadata_path(&self, name: &str) -> PathBuf {
        self.bucket_path(name).join(".bucket-metadata.json")
    }

    pub fn bucket_exists(&self, name: &str) -> bool {
        self.bucket_path(name).is_dir()
    }

    pub async fn create_bucket(&self, name: &str, region: &str) -> Result<Bucket, S3Error> {
        let bucket_path = self.bucket_path(name);

        if bucket_path.is_dir() {
            return Err(S3Error::BucketAlreadyOwnedByYou {
                bucket_name: name.to_string(),
            });
        }

        fs::create_dir_all(&bucket_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to create bucket directory: {e}"),
            })?;

        let bucket = Bucket::new(name.to_string(), region.to_string());
        let metadata_json =
            serde_json::to_string_pretty(&bucket).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize bucket metadata: {e}"),
            })?;

        fs::write(self.bucket_metadata_path(name), metadata_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write bucket metadata: {e}"),
            })?;

        Ok(bucket)
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<(), S3Error> {
        let bucket_path = self.bucket_path(name);

        if !bucket_path.is_dir() {
            return Err(S3Error::NoSuchBucket {
                bucket_name: name.to_string(),
            });
        }

        // Check if bucket is empty (only .bucket-metadata.json allowed)
        let mut entries = fs::read_dir(&bucket_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read bucket directory: {e}"),
            })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read directory entry: {e}"),
            })?
        {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if !Self::is_internal_entry(&name_str) {
                return Err(S3Error::BucketNotEmpty {
                    bucket_name: name.to_string(),
                });
            }
        }

        fs::remove_dir_all(&bucket_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to delete bucket directory: {e}"),
            })?;

        Ok(())
    }

    pub async fn list_buckets(&self) -> Result<Vec<Bucket>, S3Error> {
        let mut buckets = Vec::new();
        let mut entries =
            fs::read_dir(&self.root_dir)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read data directory: {e}"),
                })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read directory entry: {e}"),
            })?
        {
            let path = entry.path();
            if path.is_dir() {
                let metadata_path = path.join(".bucket-metadata.json");
                if metadata_path.exists() {
                    let content = fs::read_to_string(&metadata_path).await.map_err(|e| {
                        S3Error::InternalError {
                            message: format!("Failed to read bucket metadata: {e}"),
                        }
                    })?;
                    let bucket: Bucket =
                        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
                            message: format!("Failed to parse bucket metadata: {e}"),
                        })?;
                    buckets.push(bucket);
                }
            }
        }

        buckets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(buckets)
    }

    pub async fn head_bucket(&self, name: &str) -> Result<Bucket, S3Error> {
        if !self.bucket_exists(name) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: name.to_string(),
            });
        }

        let content = fs::read_to_string(self.bucket_metadata_path(name))
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read bucket metadata: {e}"),
            })?;

        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse bucket metadata: {e}"),
        })
    }

    pub async fn get_bucket_location(&self, name: &str) -> Result<String, S3Error> {
        let bucket = self.head_bucket(name).await?;
        Ok(bucket.region)
    }

    // --- Object helpers ---

    fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.bucket_path(bucket).join(key)
    }

    fn object_metadata_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.bucket_path(bucket)
            .join(".meta")
            .join(format!("{key}.json"))
    }

    fn is_internal_entry(name: &str) -> bool {
        name == ".bucket-metadata.json"
            || name == ".meta"
            || name == ".uploads"
            || name == ".tags"
            || name == ".cors.json"
            || name == ".versioning.json"
            || name == ".versions"
            || name == ".policy.json"
            || name == ".acl.xml"
            || name == ".acls"
            || name == ".lifecycle.xml"
    }

    // --- Versioning Configuration ---

    fn versioning_path(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".versioning.json")
    }

    fn versions_dir(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".versions")
    }

    fn version_data_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket)
            .join(key)
            .join(format!("{version_id}.data"))
    }

    fn version_meta_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket)
            .join(key)
            .join(format!("{version_id}.meta.json"))
    }

    pub async fn put_bucket_versioning(&self, bucket: &str, status: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let config = serde_json::json!({ "status": status });
        let json = serde_json::to_string_pretty(&config).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize versioning config: {e}"),
        })?;

        fs::write(self.versioning_path(bucket), json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write versioning config: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_bucket_versioning(&self, bucket: &str) -> Result<Option<String>, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.versioning_path(bucket);
        if !path.is_file() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read versioning config: {e}"),
            })?;

        let config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
                message: format!("Failed to parse versioning config: {e}"),
            })?;

        Ok(config
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    pub async fn is_versioning_enabled(&self, bucket: &str) -> bool {
        matches!(
            self.get_bucket_versioning(bucket).await,
            Ok(Some(ref s)) if s == "Enabled"
        )
    }

    /// Save a version (data + metadata) to the .versions/ directory.
    async fn save_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        data: &[u8],
        metadata: &ObjectMetadata,
    ) -> Result<(), S3Error> {
        let data_path = self.version_data_path(bucket, key, version_id);
        let meta_path = self.version_meta_path(bucket, key, version_id);

        if let Some(parent) = data_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create versions directory: {e}"),
                })?;
        }

        fs::write(&data_path, data)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write version data: {e}"),
            })?;

        let metadata_json =
            serde_json::to_string_pretty(metadata).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize version metadata: {e}"),
            })?;

        fs::write(&meta_path, metadata_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write version metadata: {e}"),
            })?;

        Ok(())
    }

    /// Read a specific version from .versions/.
    async fn read_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(ObjectMetadata, Vec<u8>), S3Error> {
        let data_path = self.version_data_path(bucket, key, version_id);
        let meta_path = self.version_meta_path(bucket, key, version_id);

        if !meta_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let content = fs::read_to_string(&meta_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read version metadata: {e}"),
            })?;

        let metadata: ObjectMetadata =
            serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
                message: format!("Failed to parse version metadata: {e}"),
            })?;

        // Delete markers have no data file
        if metadata.is_delete_marker {
            return Ok((metadata, Vec::new()));
        }

        let data = fs::read(&data_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read version data: {e}"),
            })?;

        Ok((metadata, data))
    }

    /// Read version metadata only (no body data).
    async fn read_version_metadata(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMetadata, S3Error> {
        let meta_path = self.version_meta_path(bucket, key, version_id);

        if !meta_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let content = fs::read_to_string(&meta_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read version metadata: {e}"),
            })?;

        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse version metadata: {e}"),
        })
    }

    /// Delete a specific version from .versions/.
    async fn delete_version(&self, bucket: &str, key: &str, version_id: &str) {
        let data_path = self.version_data_path(bucket, key, version_id);
        let meta_path = self.version_meta_path(bucket, key, version_id);

        if data_path.is_file() {
            let _ = fs::remove_file(&data_path).await;
        }
        if meta_path.is_file() {
            let _ = fs::remove_file(&meta_path).await;
        }

        // Clean up empty parent directories within .versions
        let versions_dir = self.versions_dir(bucket);
        self.cleanup_empty_parents(&meta_path, &versions_dir).await;
    }

    /// List all version IDs for a given key, sorted by last_modified descending.
    async fn list_versions_for_key(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        let key_versions_dir = self.versions_dir(bucket).join(key);
        if !key_versions_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut versions = Vec::new();
        let mut entries =
            fs::read_dir(&key_versions_dir)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read versions directory: {e}"),
                })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read version entry: {e}"),
            })?
        {
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name.ends_with(".meta.json")
                && let Ok(content) = fs::read_to_string(&path).await
                && let Ok(meta) = serde_json::from_str::<ObjectMetadata>(&content)
            {
                versions.push(meta);
            }
        }

        // Sort by last_modified descending (newest first)
        versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        Ok(versions)
    }

    /// Restore the most recent non-delete-marker version as the current object,
    /// or remove the current object if no such version exists.
    async fn restore_latest_version(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        let versions = self.list_versions_for_key(bucket, key).await?;

        // Find the most recent non-delete-marker
        let latest = versions.iter().find(|v| !v.is_delete_marker);

        match latest {
            Some(meta) => {
                let vid = meta.version_id.as_deref().unwrap_or("null");
                let data_path = self.version_data_path(bucket, key, vid);
                let data = if data_path.is_file() {
                    fs::read(&data_path).await.unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Write to current location
                let obj_path = self.object_path(bucket, key);
                if let Some(parent) = obj_path.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }
                let _ = fs::write(&obj_path, &data).await;

                // Write metadata to current location
                let meta_path = self.object_metadata_path(bucket, key);
                if let Some(parent) = meta_path.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }
                let metadata_json = serde_json::to_string_pretty(meta).unwrap_or_default();
                let _ = fs::write(&meta_path, metadata_json).await;

                Ok(())
            }
            None => {
                // No non-delete-marker versions remain; remove current object
                let obj_path = self.object_path(bucket, key);
                if obj_path.is_file() {
                    let _ = fs::remove_file(&obj_path).await;
                }
                let meta_path = self.object_metadata_path(bucket, key);
                if meta_path.is_file() {
                    let _ = fs::remove_file(&meta_path).await;
                }
                let bucket_path = self.bucket_path(bucket);
                self.cleanup_empty_parents(&obj_path, &bucket_path).await;
                self.cleanup_empty_parents(&meta_path, &bucket_path).await;
                Ok(())
            }
        }
    }

    /// List all object versions across all keys in a bucket.
    pub async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
        max_keys: i32,
    ) -> Result<crate::types::xml::ListVersionsResult, S3Error> {
        use crate::types::xml::{
            DeleteMarkerEntry, ListVersionsResult, S3_NAMESPACE, VersionEntry,
        };

        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // Collect all versions from .versions/ directory
        let versions_dir = self.versions_dir(bucket);
        let mut all_versions: Vec<ObjectMetadata> = Vec::new();

        if versions_dir.is_dir() {
            self.walk_versions_dir(&versions_dir, &versions_dir, &mut all_versions)
                .await?;
        }

        // Also include the current version of each object (from .meta/)
        let meta_dir = self.bucket_path(bucket).join(".meta");
        if meta_dir.is_dir() {
            let mut current_objects: Vec<ObjectInfo> = Vec::new();
            self.walk_meta_dir(&meta_dir, &meta_dir, &mut current_objects)
                .await?;

            for obj in current_objects {
                // Read the full metadata for this current object
                if let Ok(meta) = self.read_object_metadata(bucket, &obj.key).await {
                    // Only include if it has a version_id (versioned)
                    if meta.version_id.is_some() {
                        // Check if this version is already in all_versions
                        let already_present = all_versions
                            .iter()
                            .any(|v| v.key == meta.key && v.version_id == meta.version_id);
                        if !already_present {
                            all_versions.push(meta);
                        }
                    }
                }
            }
        }

        // Filter by prefix
        all_versions.retain(|v| v.key.starts_with(prefix));

        // Sort by key ASC, then last_modified DESC
        all_versions.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| b.last_modified.cmp(&a.last_modified))
        });

        // Determine is_latest per key
        let mut latest_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut versions = Vec::new();
        let mut delete_markers = Vec::new();
        let max = max_keys as usize;
        let mut is_truncated = false;

        for (count, meta) in all_versions.iter().enumerate() {
            if count >= max {
                is_truncated = true;
                break;
            }

            let is_latest = latest_keys.insert(meta.key.clone());
            let vid = meta
                .version_id
                .clone()
                .unwrap_or_else(|| "null".to_string());
            let last_modified = meta
                .last_modified
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();

            if meta.is_delete_marker {
                delete_markers.push(DeleteMarkerEntry {
                    key: meta.key.clone(),
                    version_id: vid,
                    is_latest,
                    last_modified,
                });
            } else {
                versions.push(VersionEntry {
                    key: meta.key.clone(),
                    version_id: vid,
                    is_latest,
                    last_modified,
                    etag: meta.etag.clone(),
                    size: meta.content_length,
                });
            }
        }

        Ok(ListVersionsResult {
            xmlns: S3_NAMESPACE.to_string(),
            name: bucket.to_string(),
            prefix: prefix.to_string(),
            max_keys,
            is_truncated,
            versions,
            delete_markers,
        })
    }

    /// Walk the .versions/ directory tree to collect all version metadata.
    async fn walk_versions_dir(
        &self,
        dir: &std::path::Path,
        versions_root: &std::path::Path,
        versions: &mut Vec<ObjectMetadata>,
    ) -> Result<(), S3Error> {
        let mut entries = fs::read_dir(dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read versions directory: {e}"),
            })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read versions directory entry: {e}"),
            })?
        {
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.walk_versions_dir(&path, versions_root, versions)).await?;
            } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && file_name.ends_with(".meta.json")
                && let Ok(content) = fs::read_to_string(&path).await
                && let Ok(meta) = serde_json::from_str::<ObjectMetadata>(&content)
            {
                versions.push(meta);
            }
        }
        Ok(())
    }

    // --- Object CRUD ---

    #[allow(clippy::too_many_arguments)]
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: &[u8],
        content_type: &str,
        custom_metadata: HashMap<String, String>,
        content_disposition: Option<String>,
        cache_control: Option<String>,
        content_encoding: Option<String>,
        expires: Option<String>,
    ) -> Result<ObjectMetadata, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let versioning_enabled = self.is_versioning_enabled(bucket).await;
        let versioning_status = self.get_bucket_versioning(bucket).await.unwrap_or(None);
        let is_suspended = versioning_status.as_deref() == Some("Suspended");

        // If versioning is enabled, archive the current version before overwriting
        if versioning_enabled
            && let Ok(current_meta) = self.read_object_metadata(bucket, key).await
            && let Some(ref vid) = current_meta.version_id
        {
            let current_obj_path = self.object_path(bucket, key);
            if current_obj_path.is_file() {
                let current_body = fs::read(&current_obj_path).await.unwrap_or_default();
                self.save_version(bucket, key, vid, &current_body, &current_meta)
                    .await?;
            }
        }

        // Compute ETag (quoted hex MD5)
        let mut hasher = Md5::new();
        hasher.update(body);
        let digest = hasher.finalize();
        let etag = format!("\"{}\"", hex::encode(digest));

        // Determine version_id for the new object
        let version_id = if versioning_enabled {
            Some(Uuid::new_v4().to_string())
        } else if is_suspended {
            Some("null".to_string())
        } else {
            None
        };

        let obj_path = self.object_path(bucket, key);

        // Create parent dirs for nested keys
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create parent directories for object: {e}"),
                })?;
        }

        // Write object body
        fs::write(&obj_path, body)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write object: {e}"),
            })?;

        let metadata = ObjectMetadata {
            key: key.to_string(),
            content_type: content_type.to_string(),
            content_length: body.len() as u64,
            etag,
            last_modified: Utc::now(),
            custom_metadata,
            content_disposition,
            cache_control,
            content_encoding,
            expires,
            version_id: version_id.clone(),
            is_delete_marker: false,
        };

        // Write metadata sidecar
        let meta_path = self.object_metadata_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create metadata directory: {e}"),
                })?;
        }

        let metadata_json =
            serde_json::to_string_pretty(&metadata).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize object metadata: {e}"),
            })?;

        fs::write(&meta_path, metadata_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write object metadata: {e}"),
            })?;

        // Also save new version to .versions/ when versioning is enabled
        if versioning_enabled && let Some(ref vid) = version_id {
            self.save_version(bucket, key, vid, body, &metadata).await?;
        }

        Ok(metadata)
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<(ObjectMetadata, Vec<u8>), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // If a specific version is requested, read from .versions/
        if let Some(vid) = version_id {
            let (metadata, body) = self.read_version(bucket, key, vid).await?;
            if metadata.is_delete_marker {
                // S3 returns 405 MethodNotAllowed when you GET a delete marker by version
                return Err(S3Error::MethodNotAllowed {
                    message: "The specified method is not allowed against this resource"
                        .to_string(),
                });
            }
            return Ok((metadata, body));
        }

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let metadata = self.read_object_metadata(bucket, key).await?;

        // If the current version is a delete marker, return NoSuchKey
        if metadata.is_delete_marker {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let body = fs::read(&obj_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read object: {e}"),
            })?;

        Ok((metadata, body))
    }

    pub async fn head_object(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<ObjectMetadata, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // If a specific version is requested, read from .versions/
        if let Some(vid) = version_id {
            let metadata = self.read_version_metadata(bucket, key, vid).await?;
            if metadata.is_delete_marker {
                return Err(S3Error::MethodNotAllowed {
                    message: "The specified method is not allowed against this resource"
                        .to_string(),
                });
            }
            return Ok(metadata);
        }

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let metadata = self.read_object_metadata(bucket, key).await?;

        if metadata.is_delete_marker {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        Ok(metadata)
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<DeleteObjectResult, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // If a specific version ID is provided, permanently delete that version
        if let Some(vid) = version_id {
            // Check if this version exists
            let meta_path = self.version_meta_path(bucket, key, vid);
            let was_delete_marker = if meta_path.is_file() {
                let content = fs::read_to_string(&meta_path).await.unwrap_or_default();
                serde_json::from_str::<ObjectMetadata>(&content)
                    .map(|m| m.is_delete_marker)
                    .unwrap_or(false)
            } else {
                false
            };

            self.delete_version(bucket, key, vid).await;

            // Check if the deleted version was the current one
            let current_meta = self.read_object_metadata(bucket, key).await.ok();
            let was_current =
                current_meta.as_ref().and_then(|m| m.version_id.as_deref()) == Some(vid);

            if was_current {
                // Restore the latest remaining version as current
                self.restore_latest_version(bucket, key).await?;
            }

            return Ok(DeleteObjectResult {
                version_id: Some(vid.to_string()),
                is_delete_marker: was_delete_marker,
            });
        }

        // Check if versioning is enabled
        let versioning_enabled = self.is_versioning_enabled(bucket).await;

        if versioning_enabled {
            // Archive the current version if it exists
            if let Ok(current_meta) = self.read_object_metadata(bucket, key).await
                && let Some(ref vid) = current_meta.version_id
            {
                let current_obj_path = self.object_path(bucket, key);
                if current_obj_path.is_file() {
                    let current_body = fs::read(&current_obj_path).await.unwrap_or_default();
                    self.save_version(bucket, key, vid, &current_body, &current_meta)
                        .await?;
                }
            }

            // Create a delete marker
            let delete_marker_version_id = Uuid::new_v4().to_string();
            let delete_marker_meta = ObjectMetadata {
                key: key.to_string(),
                content_type: String::new(),
                content_length: 0,
                etag: String::new(),
                last_modified: Utc::now(),
                custom_metadata: HashMap::new(),
                content_disposition: None,
                cache_control: None,
                content_encoding: None,
                expires: None,
                version_id: Some(delete_marker_version_id.clone()),
                is_delete_marker: true,
            };

            // Save the delete marker as a version
            self.save_version(
                bucket,
                key,
                &delete_marker_version_id,
                &[],
                &delete_marker_meta,
            )
            .await?;

            // Update the current metadata to be the delete marker
            let meta_path = self.object_metadata_path(bucket, key);
            if let Some(parent) = meta_path.parent() {
                let _ = fs::create_dir_all(parent).await;
            }
            let metadata_json =
                serde_json::to_string_pretty(&delete_marker_meta).unwrap_or_default();
            let _ = fs::write(&meta_path, metadata_json).await;

            // Remove the current object data file
            let obj_path = self.object_path(bucket, key);
            if obj_path.is_file() {
                let _ = fs::remove_file(&obj_path).await;
            }

            return Ok(DeleteObjectResult {
                version_id: Some(delete_marker_version_id),
                is_delete_marker: true,
            });
        }

        // Non-versioned: simple delete (existing behavior)
        // S3 delete is idempotent: no error for missing keys
        let obj_path = self.object_path(bucket, key);
        if obj_path.is_file() {
            let _ = fs::remove_file(&obj_path).await;
        }

        // Remove metadata sidecar
        let meta_path = self.object_metadata_path(bucket, key);
        if meta_path.is_file() {
            let _ = fs::remove_file(&meta_path).await;
        }

        // Remove tags sidecar
        let tags_path = self.tags_path(bucket, key);
        if tags_path.is_file() {
            let _ = fs::remove_file(&tags_path).await;
        }

        // Clean up empty parent dirs (but not bucket dir itself)
        let bucket_path = self.bucket_path(bucket);
        self.cleanup_empty_parents(&obj_path, &bucket_path).await;
        self.cleanup_empty_parents(&meta_path, &bucket_path).await;
        self.cleanup_empty_parents(&tags_path, &bucket_path).await;

        Ok(DeleteObjectResult {
            version_id: None,
            is_delete_marker: false,
        })
    }

    // --- List / Copy / Batch-delete ---

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: Option<&str>,
        max_keys: i32,
        start_after: Option<&str>,
    ) -> Result<ListObjectsOutput, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // Collect all object metadata from the .meta directory
        let meta_dir = self.bucket_path(bucket).join(".meta");
        let mut all_objects: Vec<ObjectInfo> = Vec::new();

        if meta_dir.is_dir() {
            self.walk_meta_dir(&meta_dir, &meta_dir, &mut all_objects)
                .await?;
        }

        // Sort alphabetically by key
        all_objects.sort_by(|a, b| a.key.cmp(&b.key));

        // Filter by prefix
        let filtered: Vec<ObjectInfo> = all_objects
            .into_iter()
            .filter(|o| o.key.starts_with(prefix))
            .collect();

        // Apply start_after: skip keys <= start_after
        let after_start: Vec<ObjectInfo> = if let Some(start) = start_after {
            filtered
                .into_iter()
                .filter(|o| o.key.as_str() > start)
                .collect()
        } else {
            filtered
        };

        // Apply delimiter logic
        let mut contents: Vec<ObjectInfo> = Vec::new();
        let mut common_prefixes: BTreeSet<String> = BTreeSet::new();

        for obj in &after_start {
            if let Some(delim) = delimiter {
                // Look for delimiter after the prefix
                let rest = &obj.key[prefix.len()..];
                if let Some(pos) = rest.find(delim) {
                    // Everything up to and including the delimiter is a common prefix
                    let cp = format!("{}{}", prefix, &rest[..pos + delim.len()]);
                    common_prefixes.insert(cp);
                } else {
                    contents.push(ObjectInfo {
                        key: obj.key.clone(),
                        size: obj.size,
                        etag: obj.etag.clone(),
                        last_modified: obj.last_modified,
                    });
                }
            } else {
                contents.push(ObjectInfo {
                    key: obj.key.clone(),
                    size: obj.size,
                    etag: obj.etag.clone(),
                    last_modified: obj.last_modified,
                });
            }
        }

        // Merge and truncate to max_keys
        // S3 counts both Contents and CommonPrefixes against MaxKeys
        let cp_vec: Vec<String> = common_prefixes.into_iter().collect();
        let total = contents.len() + cp_vec.len();
        let max = max_keys as usize;

        let is_truncated;
        let result_contents;
        let result_prefixes;

        if total <= max {
            is_truncated = false;
            result_contents = contents;
            result_prefixes = cp_vec;
        } else {
            is_truncated = true;
            // We need to interleave contents and prefixes alphabetically and take max_keys
            // Build a merged list of (key, is_prefix) to determine cutoff
            let mut merged: Vec<(String, bool)> = Vec::new();
            for c in &contents {
                merged.push((c.key.clone(), false));
            }
            for p in &cp_vec {
                merged.push((p.clone(), true));
            }
            merged.sort_by(|a, b| a.0.cmp(&b.0));
            merged.truncate(max);

            let kept_keys: BTreeSet<String> = merged
                .iter()
                .filter(|(_, is_prefix)| !is_prefix)
                .map(|(k, _)| k.clone())
                .collect();
            let kept_prefixes: BTreeSet<String> = merged
                .iter()
                .filter(|(_, is_prefix)| *is_prefix)
                .map(|(k, _)| k.clone())
                .collect();

            result_contents = contents
                .into_iter()
                .filter(|c| kept_keys.contains(&c.key))
                .collect();
            result_prefixes = cp_vec
                .into_iter()
                .filter(|p| kept_prefixes.contains(p))
                .collect();
        }

        let next_continuation_token = if is_truncated {
            // Use the last key (from either contents or prefixes) as the token
            let last_content_key = result_contents.last().map(|c| c.key.as_str());
            let last_prefix = result_prefixes.last().map(|s| s.as_str());
            let last_key = match (last_content_key, last_prefix) {
                (Some(ck), Some(pk)) => {
                    if ck > pk {
                        ck
                    } else {
                        pk
                    }
                }
                (Some(ck), None) => ck,
                (None, Some(pk)) => pk,
                (None, None) => "",
            };
            if last_key.is_empty() {
                None
            } else {
                use base64::Engine;
                Some(base64::engine::general_purpose::STANDARD.encode(last_key))
            }
        } else {
            None
        };

        Ok(ListObjectsOutput {
            objects: result_contents,
            common_prefixes: result_prefixes,
            is_truncated,
            next_continuation_token,
        })
    }

    pub async fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<ObjectMetadata, S3Error> {
        // Read source
        let (src_meta, body) = self.get_object(src_bucket, src_key, None).await?;

        // Write to destination using put_object logic
        let metadata = self
            .put_object(
                dst_bucket,
                dst_key,
                &body,
                &src_meta.content_type,
                src_meta.custom_metadata,
                src_meta.content_disposition,
                src_meta.cache_control,
                src_meta.content_encoding,
                src_meta.expires,
            )
            .await?;

        Ok(metadata)
    }

    pub async fn delete_objects(
        &self,
        bucket: &str,
        keys: &[String],
    ) -> Result<Vec<String>, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let mut deleted = Vec::new();
        for key in keys {
            // delete_object is idempotent, so we always report it as deleted
            self.delete_object(bucket, key, None).await?;
            deleted.push(key.clone());
        }

        Ok(deleted)
    }

    // --- Object Tagging ---

    fn tags_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.bucket_path(bucket)
            .join(".tags")
            .join(format!("{key}.json"))
    }

    pub async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // Verify the object exists
        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let tags_path = self.tags_path(bucket, key);
        if let Some(parent) = tags_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create tags directory: {e}"),
                })?;
        }

        let json = serde_json::to_string_pretty(&tags).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize tags: {e}"),
        })?;

        fs::write(&tags_path, json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write tags: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // Verify the object exists
        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let tags_path = self.tags_path(bucket, key);
        if !tags_path.is_file() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&tags_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read tags: {e}"),
            })?;

        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse tags: {e}"),
        })
    }

    pub async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        // Verify the object exists
        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let tags_path = self.tags_path(bucket, key);
        if tags_path.is_file() {
            let _ = fs::remove_file(&tags_path).await;
        }

        // Cleanup empty parent dirs within .tags
        let tags_dir = self.bucket_path(bucket).join(".tags");
        self.cleanup_empty_parents(&tags_path, &tags_dir).await;

        Ok(())
    }

    // --- CORS Configuration ---

    fn cors_path(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".cors.json")
    }

    pub async fn put_bucket_cors(&self, bucket: &str, rules: Vec<CorsRule>) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let json = serde_json::to_string_pretty(&rules).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize CORS rules: {e}"),
        })?;

        fs::write(self.cors_path(bucket), json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write CORS config: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let cors_path = self.cors_path(bucket);
        if !cors_path.is_file() {
            return Err(S3Error::NoSuchCORSConfiguration {
                bucket_name: bucket.to_string(),
            });
        }

        let content = fs::read_to_string(&cors_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read CORS config: {e}"),
            })?;

        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse CORS config: {e}"),
        })
    }

    pub async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let cors_path = self.cors_path(bucket);
        if cors_path.is_file() {
            let _ = fs::remove_file(&cors_path).await;
        }

        Ok(())
    }

    // --- Bucket Policy ---

    fn policy_path(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".policy.json")
    }

    pub async fn put_bucket_policy(&self, bucket: &str, policy_json: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        fs::write(self.policy_path(bucket), policy_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write bucket policy: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_bucket_policy(&self, bucket: &str) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.policy_path(bucket);
        if !path.is_file() {
            return Err(S3Error::NoSuchBucketPolicy {
                bucket_name: bucket.to_string(),
            });
        }

        fs::read_to_string(&path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read bucket policy: {e}"),
            })
    }

    pub async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.policy_path(bucket);
        if path.is_file() {
            let _ = fs::remove_file(&path).await;
        }

        Ok(())
    }

    // --- Bucket ACL ---

    fn bucket_acl_path(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".acl.xml")
    }

    fn default_acl_xml() -> String {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<AccessControlPolicy xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>local-s3-owner-id</ID><DisplayName>local-s3</DisplayName></Owner>
  <AccessControlList>
    <Grant>
      <Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="CanonicalUser">
        <ID>local-s3-owner-id</ID><DisplayName>local-s3</DisplayName>
      </Grantee>
      <Permission>FULL_CONTROL</Permission>
    </Grant>
  </AccessControlList>
</AccessControlPolicy>"#
            .to_string()
    }

    pub async fn put_bucket_acl(&self, bucket: &str, acl_xml: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        fs::write(self.bucket_acl_path(bucket), acl_xml)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write bucket ACL: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_bucket_acl(&self, bucket: &str) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.bucket_acl_path(bucket);
        if !path.is_file() {
            return Ok(Self::default_acl_xml());
        }

        fs::read_to_string(&path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read bucket ACL: {e}"),
            })
    }

    // --- Object ACL ---

    fn object_acl_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.bucket_path(bucket)
            .join(".acls")
            .join(format!("{key}.xml"))
    }

    pub async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl_xml: &str,
    ) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let acl_path = self.object_acl_path(bucket, key);
        if let Some(parent) = acl_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create ACLs directory: {e}"),
                })?;
        }

        fs::write(&acl_path, acl_xml)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write object ACL: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_object_acl(&self, bucket: &str, key: &str) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey {
                key: key.to_string(),
            });
        }

        let acl_path = self.object_acl_path(bucket, key);
        if !acl_path.is_file() {
            return Ok(Self::default_acl_xml());
        }

        fs::read_to_string(&acl_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read object ACL: {e}"),
            })
    }

    // --- Lifecycle Configuration ---

    fn lifecycle_path(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".lifecycle.xml")
    }

    pub async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        lifecycle_xml: &str,
    ) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        fs::write(self.lifecycle_path(bucket), lifecycle_xml)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write lifecycle configuration: {e}"),
            })?;

        Ok(())
    }

    pub async fn get_bucket_lifecycle(&self, bucket: &str) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.lifecycle_path(bucket);
        if !path.is_file() {
            return Err(S3Error::NoSuchLifecycleConfiguration {
                bucket_name: bucket.to_string(),
            });
        }

        fs::read_to_string(&path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read lifecycle configuration: {e}"),
            })
    }

    pub async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let path = self.lifecycle_path(bucket);
        if path.is_file() {
            let _ = fs::remove_file(&path).await;
        }

        Ok(())
    }

    // --- Multipart upload ---

    fn uploads_dir(&self, bucket: &str) -> PathBuf {
        self.bucket_path(bucket).join(".uploads")
    }

    fn upload_dir(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.uploads_dir(bucket).join(upload_id)
    }

    fn upload_state_path(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.upload_dir(bucket, upload_id).join("state.json")
    }

    fn upload_part_path(&self, bucket: &str, upload_id: &str, part_number: i32) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(format!("{part_number}.part"))
    }

    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let upload_id = Uuid::new_v4().to_string();
        let upload_dir = self.upload_dir(bucket, &upload_id);

        fs::create_dir_all(&upload_dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to create upload directory: {e}"),
            })?;

        let state = MultipartUploadState {
            key: key.to_string(),
            upload_id: upload_id.clone(),
            initiated: Utc::now(),
            parts: HashMap::new(),
        };

        let state_json =
            serde_json::to_string_pretty(&state).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize upload state: {e}"),
            })?;

        fs::write(self.upload_state_path(bucket, &upload_id), state_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write upload state: {e}"),
            })?;

        Ok(upload_id)
    }

    pub async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: i32,
        body: &[u8],
    ) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        if !(1..=10000).contains(&part_number) {
            return Err(S3Error::InvalidPart {
                message: format!("Part number must be between 1 and 10000, got {part_number}"),
            });
        }

        let state_path = self.upload_state_path(bucket, upload_id);
        if !state_path.is_file() {
            return Err(S3Error::NoSuchUpload {
                upload_id: upload_id.to_string(),
            });
        }

        // Compute ETag (quoted hex MD5 of part data)
        let mut hasher = Md5::new();
        hasher.update(body);
        let digest = hasher.finalize();
        let etag = format!("\"{}\"", hex::encode(digest));

        // Write part data
        let part_path = self.upload_part_path(bucket, upload_id, part_number);
        fs::write(&part_path, body)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write part data: {e}"),
            })?;

        // Update state
        let content =
            fs::read_to_string(&state_path)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read upload state: {e}"),
                })?;

        let mut state: MultipartUploadState =
            serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
                message: format!("Failed to parse upload state: {e}"),
            })?;

        state.parts.insert(
            part_number,
            PartInfo {
                part_number,
                etag: etag.clone(),
                size: body.len() as u64,
                last_modified: Utc::now(),
            },
        );

        let state_json =
            serde_json::to_string_pretty(&state).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize upload state: {e}"),
            })?;

        fs::write(&state_path, state_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write upload state: {e}"),
            })?;

        Ok(etag)
    }

    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>,
    ) -> Result<ObjectMetadata, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let state_path = self.upload_state_path(bucket, upload_id);
        if !state_path.is_file() {
            return Err(S3Error::NoSuchUpload {
                upload_id: upload_id.to_string(),
            });
        }

        let content =
            fs::read_to_string(&state_path)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read upload state: {e}"),
                })?;

        let state: MultipartUploadState =
            serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
                message: format!("Failed to parse upload state: {e}"),
            })?;

        // Validate all parts exist and ETags match
        for (part_number, etag) in &parts {
            let part_info = state.parts.get(part_number).ok_or(S3Error::InvalidPart {
                message: format!("Part {part_number} not found in upload {upload_id}"),
            })?;

            if part_info.etag != *etag {
                return Err(S3Error::InvalidPart {
                    message: format!(
                        "ETag mismatch for part {part_number}: expected {}, got {}",
                        part_info.etag, etag
                    ),
                });
            }
        }

        // Read and concatenate part data in order
        let mut assembled = Vec::new();
        let mut raw_md5_bytes = Vec::new();

        for (part_number, _etag) in &parts {
            let part_path = self.upload_part_path(bucket, upload_id, *part_number);
            let part_data = fs::read(&part_path)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read part {part_number}: {e}"),
                })?;

            // Compute MD5 of this part's data for composite ETag
            let mut hasher = Md5::new();
            hasher.update(&part_data);
            let part_md5 = hasher.finalize();
            raw_md5_bytes.extend_from_slice(&part_md5);

            assembled.extend_from_slice(&part_data);
        }

        // Compute composite ETag: MD5(concatenated raw MD5 bytes)-N
        let mut composite_hasher = Md5::new();
        composite_hasher.update(&raw_md5_bytes);
        let composite_digest = composite_hasher.finalize();
        let composite_etag = format!("\"{}-{}\"", hex::encode(composite_digest), parts.len());

        // Write the assembled body using put_object logic
        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create parent directories for object: {e}"),
                })?;
        }

        fs::write(&obj_path, &assembled)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write assembled object: {e}"),
            })?;

        let metadata = ObjectMetadata {
            key: key.to_string(),
            content_type: "application/octet-stream".to_string(),
            content_length: assembled.len() as u64,
            etag: composite_etag,
            last_modified: Utc::now(),
            custom_metadata: HashMap::new(),
            content_disposition: None,
            cache_control: None,
            content_encoding: None,
            expires: None,
            version_id: None,
            is_delete_marker: false,
        };

        // Write metadata sidecar
        let meta_path = self.object_metadata_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to create metadata directory: {e}"),
                })?;
        }

        let metadata_json =
            serde_json::to_string_pretty(&metadata).map_err(|e| S3Error::InternalError {
                message: format!("Failed to serialize object metadata: {e}"),
            })?;

        fs::write(&meta_path, metadata_json)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to write object metadata: {e}"),
            })?;

        // Clean up upload directory
        let upload_dir = self.upload_dir(bucket, upload_id);
        let _ = fs::remove_dir_all(&upload_dir).await;

        Ok(metadata)
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let upload_dir = self.upload_dir(bucket, upload_id);
        if !upload_dir.is_dir() {
            return Err(S3Error::NoSuchUpload {
                upload_id: upload_id.to_string(),
            });
        }

        fs::remove_dir_all(&upload_dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to remove upload directory: {e}"),
            })?;

        Ok(())
    }

    pub async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<MultipartUploadState, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let state_path = self.upload_state_path(bucket, upload_id);
        if !state_path.is_file() {
            return Err(S3Error::NoSuchUpload {
                upload_id: upload_id.to_string(),
            });
        }

        let content =
            fs::read_to_string(&state_path)
                .await
                .map_err(|e| S3Error::InternalError {
                    message: format!("Failed to read upload state: {e}"),
                })?;

        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse upload state: {e}"),
        })
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadState>, S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

        let uploads_dir = self.uploads_dir(bucket);
        if !uploads_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut uploads = Vec::new();
        let mut entries = fs::read_dir(&uploads_dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read uploads directory: {e}"),
            })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read upload entry: {e}"),
            })?
        {
            let path = entry.path();
            if path.is_dir() {
                let state_path = path.join("state.json");
                if state_path.is_file()
                    && let Ok(content) = fs::read_to_string(&state_path).await
                    && let Ok(state) = serde_json::from_str::<MultipartUploadState>(&content)
                {
                    uploads.push(state);
                }
            }
        }

        uploads.sort_by(|a, b| a.initiated.cmp(&b.initiated));
        Ok(uploads)
    }

    // --- Private helpers ---

    async fn read_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMetadata, S3Error> {
        let meta_path = self.object_metadata_path(bucket, key);
        let content = fs::read_to_string(&meta_path)
            .await
            .map_err(|_| S3Error::NoSuchKey {
                key: key.to_string(),
            })?;
        serde_json::from_str(&content).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse object metadata: {e}"),
        })
    }

    /// Recursively walk the .meta directory to find all *.json metadata files.
    /// Reconstructs the object key from the relative path within .meta.
    async fn walk_meta_dir(
        &self,
        dir: &std::path::Path,
        meta_root: &std::path::Path,
        objects: &mut Vec<ObjectInfo>,
    ) -> Result<(), S3Error> {
        let mut entries = fs::read_dir(dir)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read meta directory: {e}"),
            })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read meta directory entry: {e}"),
            })?
        {
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.walk_meta_dir(&path, meta_root, objects)).await?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
                // Reconstruct the key from relative path minus .json extension
                if let Ok(rel) = path.strip_prefix(meta_root) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    if let Some(key) = rel_str.strip_suffix(".json") {
                        // Read the metadata
                        if let Ok(content) = fs::read_to_string(&path).await
                            && let Ok(meta) = serde_json::from_str::<ObjectMetadata>(&content)
                        {
                            objects.push(ObjectInfo {
                                key: key.to_string(),
                                size: meta.content_length,
                                etag: meta.etag,
                                last_modified: meta.last_modified,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn cleanup_empty_parents(&self, path: &std::path::Path, stop_at: &std::path::Path) {
        let mut current = path.to_path_buf();
        loop {
            let Some(parent) = current.parent() else {
                break;
            };
            if parent == stop_at || !parent.starts_with(stop_at) {
                break;
            }
            // Try to remove the directory; if it's not empty, this will fail and we stop
            if fs::remove_dir(parent).await.is_err() {
                break;
            }
            current = parent.to_path_buf();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_storage() -> (FileSystemStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = FileSystemStorage::new(tmp.path().to_path_buf())
            .await
            .unwrap();
        (storage, tmp)
    }

    #[tokio::test]
    async fn test_create_bucket() {
        let (storage, _tmp) = test_storage().await;
        let bucket = storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();
        assert_eq!(bucket.name, "test-bucket");
        assert_eq!(bucket.region, "us-east-1");
        assert!(storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_create_bucket_duplicate() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("dup-bucket", "us-east-1")
            .await
            .unwrap();
        let err = storage
            .create_bucket("dup-bucket", "us-east-1")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::BucketAlreadyOwnedByYou { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("del-bucket", "us-east-1")
            .await
            .unwrap();
        storage.delete_bucket("del-bucket").await.unwrap();
        assert!(!storage.bucket_exists("del-bucket"));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_bucket() {
        let (storage, _tmp) = test_storage().await;
        let err = storage.delete_bucket("ghost").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_list_buckets_empty() {
        let (storage, _tmp) = test_storage().await;
        let buckets = storage.list_buckets().await.unwrap();
        assert!(buckets.is_empty());
    }

    #[tokio::test]
    async fn test_list_buckets_multiple() {
        let (storage, _tmp) = test_storage().await;
        storage.create_bucket("alpha", "us-east-1").await.unwrap();
        storage.create_bucket("beta", "us-east-1").await.unwrap();
        storage.create_bucket("gamma", "us-east-1").await.unwrap();
        let buckets = storage.list_buckets().await.unwrap();
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].name, "alpha");
        assert_eq!(buckets[1].name, "beta");
        assert_eq!(buckets[2].name, "gamma");
    }

    #[tokio::test]
    async fn test_head_bucket_exists() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("head-test", "us-west-2")
            .await
            .unwrap();
        let bucket = storage.head_bucket("head-test").await.unwrap();
        assert_eq!(bucket.name, "head-test");
        assert_eq!(bucket.region, "us-west-2");
    }

    #[tokio::test]
    async fn test_head_bucket_missing() {
        let (storage, _tmp) = test_storage().await;
        let err = storage.head_bucket("nope").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_get_bucket_location() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("loc-test", "eu-west-1")
            .await
            .unwrap();
        let region = storage.get_bucket_location("loc-test").await.unwrap();
        assert_eq!(region, "eu-west-1");
    }

    // --- Object CRUD tests ---

    async fn create_test_bucket(storage: &FileSystemStorage) {
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_put_and_get_object() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let body = b"hello world";
        let meta = storage
            .put_object(
                "test-bucket",
                "greeting.txt",
                body,
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.key, "greeting.txt");
        assert_eq!(meta.content_type, "text/plain");
        assert_eq!(meta.content_length, 11);
        // ETag should be quoted hex MD5 of "hello world"
        assert_eq!(meta.etag, "\"5eb63bbbe01eeed093cb22bb8f5acdc3\"");

        let (got_meta, got_body) = storage
            .get_object("test-bucket", "greeting.txt", None)
            .await
            .unwrap();
        assert_eq!(got_body, b"hello world");
        assert_eq!(got_meta.etag, meta.etag);
        assert_eq!(got_meta.content_type, "text/plain");
    }

    #[tokio::test]
    async fn test_put_object_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_object(
                "no-bucket",
                "key.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_get_object_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .get_object("no-bucket", "key.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_get_object_no_key() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .get_object("test-bucket", "missing.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_head_object() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_object(
                "test-bucket",
                "info.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta = storage
            .head_object("test-bucket", "info.txt", None)
            .await
            .unwrap();
        assert_eq!(meta.key, "info.txt");
        assert_eq!(meta.content_length, 4);
    }

    #[tokio::test]
    async fn test_head_object_no_key() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .head_object("test-bucket", "missing.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_delete_object() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_object(
                "test-bucket",
                "doomed.txt",
                b"bye",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Delete should succeed
        storage
            .delete_object("test-bucket", "doomed.txt", None)
            .await
            .unwrap();

        // Get should now fail with NoSuchKey
        let err = storage
            .get_object("test-bucket", "doomed.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_delete_object_idempotent() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Deleting a non-existent key should succeed (S3 is idempotent)
        storage
            .delete_object("test-bucket", "nonexistent.txt", None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_delete_object_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .delete_object("no-bucket", "key.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_put_object_nested_key() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let meta = storage
            .put_object(
                "test-bucket",
                "path/to/deep/file.txt",
                b"nested content",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.key, "path/to/deep/file.txt");

        let (_, body) = storage
            .get_object("test-bucket", "path/to/deep/file.txt", None)
            .await
            .unwrap();
        assert_eq!(body, b"nested content");
    }

    #[tokio::test]
    async fn test_delete_object_cleans_empty_parents() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_object(
                "test-bucket",
                "a/b/c/file.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        storage
            .delete_object("test-bucket", "a/b/c/file.txt", None)
            .await
            .unwrap();

        // Parent dirs should be cleaned up
        let nested = storage.object_path("test-bucket", "a");
        assert!(!nested.exists(), "empty parent dir 'a' should be removed");

        // But bucket dir should still exist
        assert!(storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_put_object_custom_metadata() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let mut custom = HashMap::new();
        custom.insert("author".to_string(), "test-user".to_string());
        custom.insert("version".to_string(), "1".to_string());

        let meta = storage
            .put_object(
                "test-bucket",
                "meta.txt",
                b"data",
                "text/plain",
                custom,
                Some("attachment; filename=\"meta.txt\"".to_string()),
                Some("max-age=3600".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.custom_metadata.get("author").unwrap(), "test-user");
        assert_eq!(meta.custom_metadata.get("version").unwrap(), "1");
        assert_eq!(
            meta.content_disposition.as_deref(),
            Some("attachment; filename=\"meta.txt\"")
        );
        assert_eq!(meta.cache_control.as_deref(), Some("max-age=3600"));

        // Verify round-trip through head_object
        let head = storage
            .head_object("test-bucket", "meta.txt", None)
            .await
            .unwrap();
        assert_eq!(head.custom_metadata.get("author").unwrap(), "test-user");
        assert_eq!(
            head.content_disposition.as_deref(),
            Some("attachment; filename=\"meta.txt\"")
        );
    }

    #[tokio::test]
    async fn test_put_object_overwrite() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"version 1",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"version 2",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let (meta, body) = storage
            .get_object("test-bucket", "file.txt", None)
            .await
            .unwrap();
        assert_eq!(body, b"version 2");
        assert_eq!(meta.content_length, 9);
    }

    #[tokio::test]
    async fn test_put_object_empty_body() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let meta = storage
            .put_object(
                "test-bucket",
                "empty.txt",
                b"",
                "application/octet-stream",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.content_length, 0);
        // MD5 of empty string
        assert_eq!(meta.etag, "\"d41d8cd98f00b204e9800998ecf8427e\"");

        let (_, body) = storage
            .get_object("test-bucket", "empty.txt", None)
            .await
            .unwrap();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_delete_bucket_with_meta_dir_only() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Put and delete an object, leaving the .meta directory
        storage
            .put_object(
                "test-bucket",
                "temp.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        storage
            .delete_object("test-bucket", "temp.txt", None)
            .await
            .unwrap();

        // .meta dir might still exist (if sidecar parent cleanup left it)
        // Regardless, delete_bucket should succeed since only internal entries remain
        // First let's ensure the .meta dir exists for this test to be meaningful
        let meta_dir = storage.bucket_path("test-bucket").join(".meta");
        tokio::fs::create_dir_all(&meta_dir).await.unwrap();

        storage.delete_bucket("test-bucket").await.unwrap();
        assert!(!storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_is_internal_entry() {
        assert!(FileSystemStorage::is_internal_entry(
            ".bucket-metadata.json"
        ));
        assert!(FileSystemStorage::is_internal_entry(".meta"));
        assert!(!FileSystemStorage::is_internal_entry("file.txt"));
        assert!(!FileSystemStorage::is_internal_entry(""));
    }

    // --- List objects tests ---

    async fn put_test_object(storage: &FileSystemStorage, key: &str) {
        storage
            .put_object(
                "test-bucket",
                key,
                format!("content of {key}").as_bytes(),
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_objects_empty_bucket() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let result = storage
            .list_objects("test-bucket", "", None, 1000, None)
            .await
            .unwrap();
        assert!(result.objects.is_empty());
        assert!(result.common_prefixes.is_empty());
        assert!(!result.is_truncated);
    }

    #[tokio::test]
    async fn test_list_objects_flat() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "a.txt").await;
        put_test_object(&storage, "b.txt").await;
        put_test_object(&storage, "c.txt").await;

        let result = storage
            .list_objects("test-bucket", "", None, 1000, None)
            .await
            .unwrap();
        assert_eq!(result.objects.len(), 3);
        assert_eq!(result.objects[0].key, "a.txt");
        assert_eq!(result.objects[1].key, "b.txt");
        assert_eq!(result.objects[2].key, "c.txt");
        assert!(!result.is_truncated);
    }

    #[tokio::test]
    async fn test_list_objects_with_prefix() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "photos/cat.jpg").await;
        put_test_object(&storage, "photos/dog.jpg").await;
        put_test_object(&storage, "docs/readme.txt").await;

        let result = storage
            .list_objects("test-bucket", "photos/", None, 1000, None)
            .await
            .unwrap();
        assert_eq!(result.objects.len(), 2);
        assert_eq!(result.objects[0].key, "photos/cat.jpg");
        assert_eq!(result.objects[1].key, "photos/dog.jpg");
    }

    #[tokio::test]
    async fn test_list_objects_with_delimiter() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "photos/cat.jpg").await;
        put_test_object(&storage, "photos/dog.jpg").await;
        put_test_object(&storage, "docs/readme.txt").await;
        put_test_object(&storage, "root.txt").await;

        let result = storage
            .list_objects("test-bucket", "", Some("/"), 1000, None)
            .await
            .unwrap();

        // root.txt should be in contents
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.objects[0].key, "root.txt");

        // "docs/" and "photos/" should be common prefixes
        assert_eq!(result.common_prefixes.len(), 2);
        assert_eq!(result.common_prefixes[0], "docs/");
        assert_eq!(result.common_prefixes[1], "photos/");
    }

    #[tokio::test]
    async fn test_list_objects_with_prefix_and_delimiter() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "photos/2024/jan/a.jpg").await;
        put_test_object(&storage, "photos/2024/feb/b.jpg").await;
        put_test_object(&storage, "photos/2023/dec/c.jpg").await;
        put_test_object(&storage, "photos/banner.jpg").await;

        let result = storage
            .list_objects("test-bucket", "photos/", Some("/"), 1000, None)
            .await
            .unwrap();

        // banner.jpg is directly under photos/
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.objects[0].key, "photos/banner.jpg");

        // "photos/2023/" and "photos/2024/" are common prefixes
        assert_eq!(result.common_prefixes.len(), 2);
        assert_eq!(result.common_prefixes[0], "photos/2023/");
        assert_eq!(result.common_prefixes[1], "photos/2024/");
    }

    #[tokio::test]
    async fn test_list_objects_max_keys_truncation() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "a.txt").await;
        put_test_object(&storage, "b.txt").await;
        put_test_object(&storage, "c.txt").await;

        let result = storage
            .list_objects("test-bucket", "", None, 2, None)
            .await
            .unwrap();
        assert_eq!(result.objects.len(), 2);
        assert_eq!(result.objects[0].key, "a.txt");
        assert_eq!(result.objects[1].key, "b.txt");
        assert!(result.is_truncated);
        assert!(result.next_continuation_token.is_some());
    }

    #[tokio::test]
    async fn test_list_objects_start_after() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "a.txt").await;
        put_test_object(&storage, "b.txt").await;
        put_test_object(&storage, "c.txt").await;

        let result = storage
            .list_objects("test-bucket", "", None, 1000, Some("a.txt"))
            .await
            .unwrap();
        assert_eq!(result.objects.len(), 2);
        assert_eq!(result.objects[0].key, "b.txt");
        assert_eq!(result.objects[1].key, "c.txt");
    }

    #[tokio::test]
    async fn test_list_objects_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .list_objects("nonexistent", "", None, 1000, None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    // --- Copy object tests ---

    #[tokio::test]
    async fn test_copy_object_same_bucket() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "original.txt").await;

        let meta = storage
            .copy_object("test-bucket", "original.txt", "test-bucket", "copy.txt")
            .await
            .unwrap();

        assert_eq!(meta.key, "copy.txt");
        assert_eq!(meta.content_type, "text/plain");

        // Both should exist
        let (_, body1) = storage
            .get_object("test-bucket", "original.txt", None)
            .await
            .unwrap();
        let (_, body2) = storage
            .get_object("test-bucket", "copy.txt", None)
            .await
            .unwrap();
        assert_eq!(body1, body2);
    }

    #[tokio::test]
    async fn test_copy_object_cross_bucket() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .create_bucket("other-bucket", "us-east-1")
            .await
            .unwrap();

        put_test_object(&storage, "source.txt").await;

        let meta = storage
            .copy_object("test-bucket", "source.txt", "other-bucket", "dest.txt")
            .await
            .unwrap();

        assert_eq!(meta.key, "dest.txt");

        let (_, body) = storage
            .get_object("other-bucket", "dest.txt", None)
            .await
            .unwrap();
        assert_eq!(body, b"content of source.txt");
    }

    #[tokio::test]
    async fn test_copy_object_source_not_found() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .copy_object("test-bucket", "nonexistent.txt", "test-bucket", "copy.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    // --- Batch delete tests ---

    #[tokio::test]
    async fn test_delete_objects_batch() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "a.txt").await;
        put_test_object(&storage, "b.txt").await;
        put_test_object(&storage, "c.txt").await;

        let keys = vec!["a.txt".to_string(), "c.txt".to_string()];
        let deleted = storage.delete_objects("test-bucket", &keys).await.unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(deleted.contains(&"a.txt".to_string()));
        assert!(deleted.contains(&"c.txt".to_string()));

        // Only b.txt should remain
        let result = storage
            .list_objects("test-bucket", "", None, 1000, None)
            .await
            .unwrap();
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.objects[0].key, "b.txt");
    }

    #[tokio::test]
    async fn test_delete_objects_idempotent() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Deleting nonexistent keys should succeed
        let keys = vec!["ghost1.txt".to_string(), "ghost2.txt".to_string()];
        let deleted = storage.delete_objects("test-bucket", &keys).await.unwrap();
        assert_eq!(deleted.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_objects_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let keys = vec!["a.txt".to_string()];
        let err = storage
            .delete_objects("nonexistent", &keys)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    // --- Multipart upload tests ---

    #[tokio::test]
    async fn test_create_multipart_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "large-file.bin")
            .await
            .unwrap();

        assert!(!upload_id.is_empty());

        // Verify state file was created
        let state = storage.list_parts("test-bucket", &upload_id).await.unwrap();
        assert_eq!(state.key, "large-file.bin");
        assert_eq!(state.upload_id, upload_id);
        assert!(state.parts.is_empty());
    }

    #[tokio::test]
    async fn test_create_multipart_upload_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .create_multipart_upload("nonexistent", "key.bin")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_upload_part() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        let etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, b"part one data")
            .await
            .unwrap();

        let etag2 = storage
            .upload_part("test-bucket", &upload_id, 2, b"part two data")
            .await
            .unwrap();

        assert!(etag1.starts_with('"') && etag1.ends_with('"'));
        assert!(etag2.starts_with('"') && etag2.ends_with('"'));
        assert_ne!(etag1, etag2);

        // Verify parts recorded in state
        let state = storage.list_parts("test-bucket", &upload_id).await.unwrap();
        assert_eq!(state.parts.len(), 2);
        assert_eq!(state.parts[&1].etag, etag1);
        assert_eq!(state.parts[&2].etag, etag2);
    }

    #[tokio::test]
    async fn test_upload_part_invalid_number() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        let err = storage
            .upload_part("test-bucket", &upload_id, 0, b"data")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::InvalidPart { .. }));

        let err = storage
            .upload_part("test-bucket", &upload_id, 10001, b"data")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::InvalidPart { .. }));
    }

    #[tokio::test]
    async fn test_upload_part_no_such_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .upload_part("test-bucket", "nonexistent-upload-id", 1, b"data")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchUpload { .. }));
    }

    #[tokio::test]
    async fn test_complete_multipart_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "assembled.bin")
            .await
            .unwrap();

        let etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, b"hello ")
            .await
            .unwrap();

        let etag2 = storage
            .upload_part("test-bucket", &upload_id, 2, b"world")
            .await
            .unwrap();

        let metadata = storage
            .complete_multipart_upload(
                "test-bucket",
                "assembled.bin",
                &upload_id,
                vec![(1, etag1), (2, etag2)],
            )
            .await
            .unwrap();

        // Check composite ETag format: "hex-N"
        assert!(metadata.etag.starts_with('"'));
        assert!(metadata.etag.ends_with('"'));
        let inner = &metadata.etag[1..metadata.etag.len() - 1];
        assert!(inner.ends_with("-2"), "ETag should end with -2: {inner}");

        // Verify assembled content
        let (_, body) = storage
            .get_object("test-bucket", "assembled.bin", None)
            .await
            .unwrap();
        assert_eq!(body, b"hello world");

        // Upload directory should be cleaned up
        let upload_dir = storage.upload_dir("test-bucket", &upload_id);
        assert!(!upload_dir.exists());
    }

    #[tokio::test]
    async fn test_complete_multipart_upload_etag_format() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "etag-test.bin")
            .await
            .unwrap();

        let part1_data = b"aaaa";
        let part2_data = b"bbbb";
        let part3_data = b"cccc";

        let etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, part1_data)
            .await
            .unwrap();
        let etag2 = storage
            .upload_part("test-bucket", &upload_id, 2, part2_data)
            .await
            .unwrap();
        let etag3 = storage
            .upload_part("test-bucket", &upload_id, 3, part3_data)
            .await
            .unwrap();

        let metadata = storage
            .complete_multipart_upload(
                "test-bucket",
                "etag-test.bin",
                &upload_id,
                vec![(1, etag1), (2, etag2), (3, etag3)],
            )
            .await
            .unwrap();

        // Manually compute expected composite ETag
        use md5::{Digest, Md5};
        let md5_1 = {
            let mut h = Md5::new();
            h.update(part1_data);
            h.finalize()
        };
        let md5_2 = {
            let mut h = Md5::new();
            h.update(part2_data);
            h.finalize()
        };
        let md5_3 = {
            let mut h = Md5::new();
            h.update(part3_data);
            h.finalize()
        };

        let mut combined = Vec::new();
        combined.extend_from_slice(&md5_1);
        combined.extend_from_slice(&md5_2);
        combined.extend_from_slice(&md5_3);

        let mut composite_hasher = Md5::new();
        composite_hasher.update(&combined);
        let expected = format!("\"{}-3\"", hex::encode(composite_hasher.finalize()));

        assert_eq!(metadata.etag, expected);
    }

    #[tokio::test]
    async fn test_complete_multipart_upload_invalid_part() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        let etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, b"data")
            .await
            .unwrap();

        // Try to complete with a part that doesn't exist
        let err = storage
            .complete_multipart_upload(
                "test-bucket",
                "file.bin",
                &upload_id,
                vec![(1, etag1), (2, "\"fake-etag\"".to_string())],
            )
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::InvalidPart { .. }));
    }

    #[tokio::test]
    async fn test_complete_multipart_upload_etag_mismatch() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        let _etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, b"data")
            .await
            .unwrap();

        // Try to complete with wrong ETag
        let err = storage
            .complete_multipart_upload(
                "test-bucket",
                "file.bin",
                &upload_id,
                vec![(1, "\"wrong-etag\"".to_string())],
            )
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::InvalidPart { .. }));
    }

    #[tokio::test]
    async fn test_abort_multipart_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        storage
            .upload_part("test-bucket", &upload_id, 1, b"data")
            .await
            .unwrap();

        storage
            .abort_multipart_upload("test-bucket", &upload_id)
            .await
            .unwrap();

        // Upload should no longer exist
        let err = storage
            .list_parts("test-bucket", &upload_id)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchUpload { .. }));
    }

    #[tokio::test]
    async fn test_abort_multipart_upload_no_such_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .abort_multipart_upload("test-bucket", "nonexistent")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchUpload { .. }));
    }

    #[tokio::test]
    async fn test_list_parts() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        storage
            .upload_part("test-bucket", &upload_id, 1, b"part1")
            .await
            .unwrap();
        storage
            .upload_part("test-bucket", &upload_id, 3, b"part3")
            .await
            .unwrap();
        storage
            .upload_part("test-bucket", &upload_id, 2, b"part2")
            .await
            .unwrap();

        let state = storage.list_parts("test-bucket", &upload_id).await.unwrap();
        assert_eq!(state.parts.len(), 3);
        assert_eq!(state.parts[&1].size, 5);
        assert_eq!(state.parts[&2].size, 5);
        assert_eq!(state.parts[&3].size, 5);
    }

    #[tokio::test]
    async fn test_list_multipart_uploads() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let id1 = storage
            .create_multipart_upload("test-bucket", "file1.bin")
            .await
            .unwrap();
        let id2 = storage
            .create_multipart_upload("test-bucket", "file2.bin")
            .await
            .unwrap();

        let uploads = storage.list_multipart_uploads("test-bucket").await.unwrap();
        assert_eq!(uploads.len(), 2);

        let ids: Vec<&str> = uploads.iter().map(|u| u.upload_id.as_str()).collect();
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_empty() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let uploads = storage.list_multipart_uploads("test-bucket").await.unwrap();
        assert!(uploads.is_empty());
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .list_multipart_uploads("nonexistent")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_upload_part_overwrite() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();

        let etag1 = storage
            .upload_part("test-bucket", &upload_id, 1, b"original data")
            .await
            .unwrap();

        let etag2 = storage
            .upload_part("test-bucket", &upload_id, 1, b"replaced data")
            .await
            .unwrap();

        // ETags should differ since data is different
        assert_ne!(etag1, etag2);

        // State should reflect the latest upload
        let state = storage.list_parts("test-bucket", &upload_id).await.unwrap();
        assert_eq!(state.parts.len(), 1);
        assert_eq!(state.parts[&1].etag, etag2);
    }

    #[tokio::test]
    async fn test_delete_bucket_with_uploads_dir_only() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Create and abort a multipart upload to leave .uploads dir
        let upload_id = storage
            .create_multipart_upload("test-bucket", "file.bin")
            .await
            .unwrap();
        storage
            .abort_multipart_upload("test-bucket", &upload_id)
            .await
            .unwrap();

        // Ensure .uploads dir exists for this test
        let uploads_dir = storage.uploads_dir("test-bucket");
        tokio::fs::create_dir_all(&uploads_dir).await.unwrap();

        // Delete bucket should succeed since only internal entries remain
        storage.delete_bucket("test-bucket").await.unwrap();
        assert!(!storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_is_internal_entry_uploads() {
        assert!(FileSystemStorage::is_internal_entry(".uploads"));
    }

    #[tokio::test]
    async fn test_complete_multipart_upload_no_such_upload() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .complete_multipart_upload(
                "test-bucket",
                "file.bin",
                "nonexistent",
                vec![(1, "\"etag\"".to_string())],
            )
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchUpload { .. }));
    }

    // --- Object Tagging tests ---

    #[tokio::test]
    async fn test_put_and_get_object_tagging() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        put_test_object(&storage, "tagged.txt").await;

        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "production".to_string());
        tags.insert("project".to_string(), "test".to_string());

        storage
            .put_object_tagging("test-bucket", "tagged.txt", tags)
            .await
            .unwrap();

        let got_tags = storage
            .get_object_tagging("test-bucket", "tagged.txt")
            .await
            .unwrap();

        assert_eq!(got_tags.len(), 2);
        assert_eq!(got_tags.get("env").unwrap(), "production");
        assert_eq!(got_tags.get("project").unwrap(), "test");
    }

    #[tokio::test]
    async fn test_get_object_tagging_empty() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        put_test_object(&storage, "untagged.txt").await;

        let tags = storage
            .get_object_tagging("test-bucket", "untagged.txt")
            .await
            .unwrap();
        assert!(tags.is_empty());
    }

    #[tokio::test]
    async fn test_delete_object_tagging() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        put_test_object(&storage, "tagged.txt").await;

        let mut tags = HashMap::new();
        tags.insert("key".to_string(), "value".to_string());
        storage
            .put_object_tagging("test-bucket", "tagged.txt", tags)
            .await
            .unwrap();

        storage
            .delete_object_tagging("test-bucket", "tagged.txt")
            .await
            .unwrap();

        let got_tags = storage
            .get_object_tagging("test-bucket", "tagged.txt")
            .await
            .unwrap();
        assert!(got_tags.is_empty());
    }

    #[tokio::test]
    async fn test_tagging_no_such_key() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .put_object_tagging("test-bucket", "missing.txt", HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));

        let err = storage
            .get_object_tagging("test-bucket", "missing.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_tagging_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_object_tagging("no-bucket", "key.txt", HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_delete_object_removes_tags() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        put_test_object(&storage, "tagged.txt").await;

        let mut tags = HashMap::new();
        tags.insert("key".to_string(), "value".to_string());
        storage
            .put_object_tagging("test-bucket", "tagged.txt", tags)
            .await
            .unwrap();

        // Delete the object
        storage
            .delete_object("test-bucket", "tagged.txt", None)
            .await
            .unwrap();

        // Tags file should be gone (object no longer exists, so get_tagging should fail)
        let tags_path = storage.tags_path("test-bucket", "tagged.txt");
        assert!(!tags_path.exists());
    }

    #[tokio::test]
    async fn test_put_object_tagging_overwrite() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        put_test_object(&storage, "tagged.txt").await;

        let mut tags1 = HashMap::new();
        tags1.insert("version".to_string(), "1".to_string());
        storage
            .put_object_tagging("test-bucket", "tagged.txt", tags1)
            .await
            .unwrap();

        let mut tags2 = HashMap::new();
        tags2.insert("version".to_string(), "2".to_string());
        tags2.insert("extra".to_string(), "new".to_string());
        storage
            .put_object_tagging("test-bucket", "tagged.txt", tags2)
            .await
            .unwrap();

        let got = storage
            .get_object_tagging("test-bucket", "tagged.txt")
            .await
            .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got.get("version").unwrap(), "2");
        assert_eq!(got.get("extra").unwrap(), "new");
    }

    // --- CORS Configuration tests ---

    #[tokio::test]
    async fn test_put_and_get_bucket_cors() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let rules = vec![CorsRule {
            allowed_origins: vec!["http://example.com".to_string()],
            allowed_methods: vec!["GET".to_string(), "PUT".to_string()],
            allowed_headers: vec!["*".to_string()],
            max_age_seconds: Some(3600),
            expose_headers: vec!["x-amz-request-id".to_string()],
        }];

        storage.put_bucket_cors("test-bucket", rules).await.unwrap();

        let got = storage.get_bucket_cors("test-bucket").await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].allowed_origins, vec!["http://example.com"]);
        assert_eq!(got[0].allowed_methods, vec!["GET", "PUT"]);
        assert_eq!(got[0].allowed_headers, vec!["*"]);
        assert_eq!(got[0].max_age_seconds, Some(3600));
        assert_eq!(got[0].expose_headers, vec!["x-amz-request-id"]);
    }

    #[tokio::test]
    async fn test_get_bucket_cors_not_configured() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage.get_bucket_cors("test-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchCORSConfiguration { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_cors() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let rules = vec![CorsRule {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string()],
            allowed_headers: vec![],
            max_age_seconds: None,
            expose_headers: vec![],
        }];

        storage.put_bucket_cors("test-bucket", rules).await.unwrap();

        storage.delete_bucket_cors("test-bucket").await.unwrap();

        let err = storage.get_bucket_cors("test-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchCORSConfiguration { .. }));
    }

    #[tokio::test]
    async fn test_cors_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_bucket_cors("no-bucket", vec![])
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.get_bucket_cors("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.delete_bucket_cors("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_cors_idempotent() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Deleting CORS when none exists should succeed
        storage.delete_bucket_cors("test-bucket").await.unwrap();
    }

    #[tokio::test]
    async fn test_is_internal_entry_tags() {
        assert!(FileSystemStorage::is_internal_entry(".tags"));
    }

    #[tokio::test]
    async fn test_is_internal_entry_cors() {
        assert!(FileSystemStorage::is_internal_entry(".cors.json"));
    }

    #[tokio::test]
    async fn test_delete_bucket_with_cors_and_tags() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        // Set CORS config
        let rules = vec![CorsRule {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string()],
            allowed_headers: vec![],
            max_age_seconds: None,
            expose_headers: vec![],
        }];
        storage.put_bucket_cors("test-bucket", rules).await.unwrap();

        // Create .tags dir for good measure
        let tags_dir = storage.bucket_path("test-bucket").join(".tags");
        tokio::fs::create_dir_all(&tags_dir).await.unwrap();

        // Delete bucket should succeed since only internal entries remain
        storage.delete_bucket("test-bucket").await.unwrap();
        assert!(!storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_put_bucket_cors_multiple_rules() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let rules = vec![
            CorsRule {
                allowed_origins: vec!["http://app.example.com".to_string()],
                allowed_methods: vec!["GET".to_string()],
                allowed_headers: vec![],
                max_age_seconds: None,
                expose_headers: vec![],
            },
            CorsRule {
                allowed_origins: vec!["http://admin.example.com".to_string()],
                allowed_methods: vec!["GET".to_string(), "PUT".to_string(), "DELETE".to_string()],
                allowed_headers: vec!["Authorization".to_string()],
                max_age_seconds: Some(86400),
                expose_headers: vec![],
            },
        ];

        storage.put_bucket_cors("test-bucket", rules).await.unwrap();

        let got = storage.get_bucket_cors("test-bucket").await.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].allowed_origins, vec!["http://app.example.com"]);
        assert_eq!(got[1].allowed_methods.len(), 3);
    }

    // --- Versioning tests ---

    #[tokio::test]
    async fn test_is_internal_entry_versioning() {
        assert!(FileSystemStorage::is_internal_entry(".versioning.json"));
        assert!(FileSystemStorage::is_internal_entry(".versions"));
    }

    #[tokio::test]
    async fn test_versioning_config_default_none() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let status = storage.get_bucket_versioning("test-bucket").await.unwrap();
        assert!(status.is_none());
        assert!(!storage.is_versioning_enabled("test-bucket").await);
    }

    #[tokio::test]
    async fn test_versioning_config_enable() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let status = storage.get_bucket_versioning("test-bucket").await.unwrap();
        assert_eq!(status, Some("Enabled".to_string()));
        assert!(storage.is_versioning_enabled("test-bucket").await);
    }

    #[tokio::test]
    async fn test_versioning_config_suspend() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();
        storage
            .put_bucket_versioning("test-bucket", "Suspended")
            .await
            .unwrap();

        let status = storage.get_bucket_versioning("test-bucket").await.unwrap();
        assert_eq!(status, Some("Suspended".to_string()));
        assert!(!storage.is_versioning_enabled("test-bucket").await);
    }

    #[tokio::test]
    async fn test_versioning_config_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_bucket_versioning("no-bucket", "Enabled")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage
            .get_bucket_versioning("no-bucket")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_versioned_put_creates_version_id() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta = storage
            .put_object(
                "test-bucket",
                "versioned.txt",
                b"v1 content",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(meta.version_id.is_some());
        assert!(!meta.is_delete_marker);
    }

    #[tokio::test]
    async fn test_unversioned_put_no_version_id() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let meta = storage
            .put_object(
                "test-bucket",
                "unversioned.txt",
                b"content",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(meta.version_id.is_none());
    }

    #[tokio::test]
    async fn test_versioned_put_multiple_versions() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"version 1",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"version 2",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Version IDs should be different
        assert_ne!(meta1.version_id, meta2.version_id);

        // GET without version_id returns the latest version
        let (got_meta, got_body) = storage
            .get_object("test-bucket", "file.txt", None)
            .await
            .unwrap();
        assert_eq!(got_body, b"version 2");
        assert_eq!(got_meta.version_id, meta2.version_id);

        // GET with version_id returns specific version
        let vid1 = meta1.version_id.as_deref().unwrap();
        let (got_meta_v1, got_body_v1) = storage
            .get_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap();
        assert_eq!(got_body_v1, b"version 1");
        assert_eq!(got_meta_v1.version_id.as_deref(), Some(vid1));
    }

    #[tokio::test]
    async fn test_versioned_get_by_version_id() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"first",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let _meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"second",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let vid1 = meta1.version_id.as_deref().unwrap();
        let (_, body) = storage
            .get_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap();
        assert_eq!(body, b"first");
    }

    #[tokio::test]
    async fn test_versioned_head_by_version_id() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"first",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let _meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"second version",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let vid1 = meta1.version_id.as_deref().unwrap();
        let head = storage
            .head_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap();
        assert_eq!(head.content_length, 5); // "first" = 5 bytes
        assert_eq!(head.version_id.as_deref(), Some(vid1));
    }

    #[tokio::test]
    async fn test_versioned_delete_creates_marker() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta = storage
            .put_object(
                "test-bucket",
                "to-delete.txt",
                b"content",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let result = storage
            .delete_object("test-bucket", "to-delete.txt", None)
            .await
            .unwrap();

        assert!(result.is_delete_marker);
        assert!(result.version_id.is_some());

        // GET without version_id should return NoSuchKey (delete marker)
        let err = storage
            .get_object("test-bucket", "to-delete.txt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));

        // But the original version is still accessible by version_id
        let vid = meta.version_id.as_deref().unwrap();
        let (_, body) = storage
            .get_object("test-bucket", "to-delete.txt", Some(vid))
            .await
            .unwrap();
        assert_eq!(body, b"content");
    }

    #[tokio::test]
    async fn test_versioned_delete_specific_version() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"v1",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"v2",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Delete the old version specifically
        let vid1 = meta1.version_id.as_deref().unwrap();
        let result = storage
            .delete_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap();
        assert!(!result.is_delete_marker);
        assert_eq!(result.version_id.as_deref(), Some(vid1));

        // Old version should be gone
        let err = storage
            .get_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));

        // Latest version should still be accessible
        let vid2 = meta2.version_id.as_deref().unwrap();
        let (_, body) = storage
            .get_object("test-bucket", "file.txt", Some(vid2))
            .await
            .unwrap();
        assert_eq!(body, b"v2");
    }

    #[tokio::test]
    async fn test_versioned_delete_marker_then_delete_marker() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Create a delete marker
        let del_result = storage
            .delete_object("test-bucket", "file.txt", None)
            .await
            .unwrap();
        assert!(del_result.is_delete_marker);

        // Now delete the delete marker by version ID
        let dm_vid = del_result.version_id.as_deref().unwrap();
        let result2 = storage
            .delete_object("test-bucket", "file.txt", Some(dm_vid))
            .await
            .unwrap();
        assert!(result2.is_delete_marker); // was a delete marker we deleted

        // Now the object should be accessible again via the original version
        let vid = meta.version_id.as_deref().unwrap();
        let (_, body) = storage
            .get_object("test-bucket", "file.txt", Some(vid))
            .await
            .unwrap();
        assert_eq!(body, b"data");
    }

    #[tokio::test]
    async fn test_list_object_versions() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        // Put two versions of the same key
        let _meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"v1",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let _meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"v2",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let result = storage
            .list_object_versions("test-bucket", "", 1000)
            .await
            .unwrap();

        // Should have at least 2 versions (v1 in .versions/ and v2 as current + in .versions/)
        assert!(result.versions.len() >= 2);
        assert!(result.delete_markers.is_empty());

        // All should have key "file.txt"
        for v in &result.versions {
            assert_eq!(v.key, "file.txt");
        }

        // Exactly one should be marked as latest
        let latest_count = result.versions.iter().filter(|v| v.is_latest).count();
        assert_eq!(latest_count, 1);
    }

    #[tokio::test]
    async fn test_list_object_versions_with_delete_marker() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let _meta = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        storage
            .delete_object("test-bucket", "file.txt", None)
            .await
            .unwrap();

        let result = storage
            .list_object_versions("test-bucket", "", 1000)
            .await
            .unwrap();

        // Should have at least 1 version and 1 delete marker
        assert!(!result.versions.is_empty());
        assert!(!result.delete_markers.is_empty());

        // The delete marker should be the latest
        let dm_latest = result.delete_markers.iter().any(|dm| dm.is_latest);
        assert!(dm_latest);
    }

    #[tokio::test]
    async fn test_list_object_versions_prefix_filter() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        storage
            .put_object(
                "test-bucket",
                "docs/a.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        storage
            .put_object(
                "test-bucket",
                "photos/b.jpg",
                b"image data",
                "image/jpeg",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let result = storage
            .list_object_versions("test-bucket", "docs/", 1000)
            .await
            .unwrap();

        // Should only contain docs/a.txt versions
        for v in &result.versions {
            assert!(v.key.starts_with("docs/"));
        }
    }

    #[tokio::test]
    async fn test_delete_bucket_with_versioning_config() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        // Create .versions dir
        let versions_dir = storage.versions_dir("test-bucket");
        tokio::fs::create_dir_all(&versions_dir).await.unwrap();

        // Delete bucket should succeed since only internal entries remain
        storage.delete_bucket("test-bucket").await.unwrap();
        assert!(!storage.bucket_exists("test-bucket"));
    }

    #[tokio::test]
    async fn test_unversioned_delete_returns_no_version() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        put_test_object(&storage, "file.txt").await;

        let result = storage
            .delete_object("test-bucket", "file.txt", None)
            .await
            .unwrap();

        assert!(result.version_id.is_none());
        assert!(!result.is_delete_marker);
    }

    #[tokio::test]
    async fn test_suspended_versioning_uses_null_version() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        storage
            .put_bucket_versioning("test-bucket", "Suspended")
            .await
            .unwrap();

        let meta = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"suspended",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.version_id.as_deref(), Some("null"));
    }

    #[tokio::test]
    async fn test_versioned_get_nonexistent_version() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        put_test_object(&storage, "file.txt").await;

        let err = storage
            .get_object("test-bucket", "file.txt", Some("nonexistent-version"))
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_versioned_three_versions_get_each() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"aaa",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"bbb",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta3 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"ccc",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Get each by version
        let vid1 = meta1.version_id.as_deref().unwrap();
        let vid2 = meta2.version_id.as_deref().unwrap();
        let vid3 = meta3.version_id.as_deref().unwrap();

        let (_, b1) = storage
            .get_object("test-bucket", "file.txt", Some(vid1))
            .await
            .unwrap();
        let (_, b2) = storage
            .get_object("test-bucket", "file.txt", Some(vid2))
            .await
            .unwrap();
        let (_, b3) = storage
            .get_object("test-bucket", "file.txt", Some(vid3))
            .await
            .unwrap();

        assert_eq!(b1, b"aaa");
        assert_eq!(b2, b"bbb");
        assert_eq!(b3, b"ccc");

        // Latest should be v3
        let (_, latest) = storage
            .get_object("test-bucket", "file.txt", None)
            .await
            .unwrap();
        assert_eq!(latest, b"ccc");
    }

    #[tokio::test]
    async fn test_versioned_delete_current_restores_previous() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let _meta1 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"first",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let meta2 = storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"second",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Delete the current version (v2) by its version_id
        let vid2 = meta2.version_id.as_deref().unwrap();
        storage
            .delete_object("test-bucket", "file.txt", Some(vid2))
            .await
            .unwrap();

        // The current object should now be restored from v1
        let (_, body) = storage
            .get_object("test-bucket", "file.txt", None)
            .await
            .unwrap();
        assert_eq!(body, b"first");
    }

    #[tokio::test]
    async fn test_list_object_versions_empty_bucket() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        let result = storage
            .list_object_versions("test-bucket", "", 1000)
            .await
            .unwrap();

        assert!(result.versions.is_empty());
        assert!(result.delete_markers.is_empty());
        assert!(!result.is_truncated);
    }

    #[tokio::test]
    async fn test_get_delete_marker_by_version_returns_method_not_allowed() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;
        storage
            .put_bucket_versioning("test-bucket", "Enabled")
            .await
            .unwrap();

        storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"data",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let del_result = storage
            .delete_object("test-bucket", "file.txt", None)
            .await
            .unwrap();

        let dm_vid = del_result.version_id.as_deref().unwrap();

        // GET a delete marker by version ID should return MethodNotAllowed
        let err = storage
            .get_object("test-bucket", "file.txt", Some(dm_vid))
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::MethodNotAllowed { .. }));

        // HEAD a delete marker by version ID should also return MethodNotAllowed
        let err = storage
            .head_object("test-bucket", "file.txt", Some(dm_vid))
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::MethodNotAllowed { .. }));
    }

    // --- Bucket Policy tests ---

    #[tokio::test]
    async fn test_put_and_get_bucket_policy() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
        storage
            .put_bucket_policy("test-bucket", policy)
            .await
            .unwrap();

        let got = storage.get_bucket_policy("test-bucket").await.unwrap();
        assert_eq!(got, policy);
    }

    #[tokio::test]
    async fn test_get_bucket_policy_not_set() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let err = storage.get_bucket_policy("test-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucketPolicy { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_policy() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
        storage
            .put_bucket_policy("test-bucket", policy)
            .await
            .unwrap();

        storage.delete_bucket_policy("test-bucket").await.unwrap();

        let err = storage.get_bucket_policy("test-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucketPolicy { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_policy_idempotent() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        // Deleting when no policy exists should succeed silently
        storage.delete_bucket_policy("test-bucket").await.unwrap();
    }

    #[tokio::test]
    async fn test_bucket_policy_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_bucket_policy("no-bucket", "{}")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.get_bucket_policy("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.delete_bucket_policy("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_put_bucket_policy_overwrite() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        storage
            .put_bucket_policy("test-bucket", r#"{"v":1}"#)
            .await
            .unwrap();
        storage
            .put_bucket_policy("test-bucket", r#"{"v":2}"#)
            .await
            .unwrap();

        let got = storage.get_bucket_policy("test-bucket").await.unwrap();
        assert_eq!(got, r#"{"v":2}"#);
    }

    // --- Bucket ACL tests ---

    #[tokio::test]
    async fn test_get_bucket_acl_default() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let acl = storage.get_bucket_acl("test-bucket").await.unwrap();
        assert!(acl.contains("FULL_CONTROL"));
        assert!(acl.contains("local-s3-owner-id"));
    }

    #[tokio::test]
    async fn test_put_and_get_bucket_acl() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let custom_acl = "<AccessControlPolicy>custom</AccessControlPolicy>";
        storage
            .put_bucket_acl("test-bucket", custom_acl)
            .await
            .unwrap();

        let got = storage.get_bucket_acl("test-bucket").await.unwrap();
        assert_eq!(got, custom_acl);
    }

    #[tokio::test]
    async fn test_bucket_acl_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_bucket_acl("no-bucket", "<acl/>")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.get_bucket_acl("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    // --- Object ACL tests ---

    #[tokio::test]
    async fn test_get_object_acl_default() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();
        storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"hello",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let acl = storage
            .get_object_acl("test-bucket", "file.txt")
            .await
            .unwrap();
        assert!(acl.contains("FULL_CONTROL"));
        assert!(acl.contains("local-s3-owner-id"));
    }

    #[tokio::test]
    async fn test_put_and_get_object_acl() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();
        storage
            .put_object(
                "test-bucket",
                "file.txt",
                b"hello",
                "text/plain",
                HashMap::new(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let custom_acl = "<AccessControlPolicy>custom-object</AccessControlPolicy>";
        storage
            .put_object_acl("test-bucket", "file.txt", custom_acl)
            .await
            .unwrap();

        let got = storage
            .get_object_acl("test-bucket", "file.txt")
            .await
            .unwrap();
        assert_eq!(got, custom_acl);
    }

    #[tokio::test]
    async fn test_object_acl_no_such_key() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let err = storage
            .put_object_acl("test-bucket", "missing.txt", "<acl/>")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));

        let err = storage
            .get_object_acl("test-bucket", "missing.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchKey { .. }));
    }

    #[tokio::test]
    async fn test_object_acl_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_object_acl("no-bucket", "file.txt", "<acl/>")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage
            .get_object_acl("no-bucket", "file.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    // --- Lifecycle Configuration tests ---

    #[tokio::test]
    async fn test_put_and_get_bucket_lifecycle() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let lifecycle = "<LifecycleConfiguration><Rule><Status>Enabled</Status></Rule></LifecycleConfiguration>";
        storage
            .put_bucket_lifecycle("test-bucket", lifecycle)
            .await
            .unwrap();

        let got = storage.get_bucket_lifecycle("test-bucket").await.unwrap();
        assert_eq!(got, lifecycle);
    }

    #[tokio::test]
    async fn test_get_bucket_lifecycle_not_set() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let err = storage
            .get_bucket_lifecycle("test-bucket")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchLifecycleConfiguration { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_lifecycle() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        let lifecycle = "<LifecycleConfiguration/>";
        storage
            .put_bucket_lifecycle("test-bucket", lifecycle)
            .await
            .unwrap();

        storage
            .delete_bucket_lifecycle("test-bucket")
            .await
            .unwrap();

        let err = storage
            .get_bucket_lifecycle("test-bucket")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchLifecycleConfiguration { .. }));
    }

    #[tokio::test]
    async fn test_delete_bucket_lifecycle_idempotent() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        // Deleting when no lifecycle exists should succeed silently
        storage
            .delete_bucket_lifecycle("test-bucket")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_bucket_lifecycle_no_such_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .put_bucket_lifecycle("no-bucket", "<xml/>")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage.get_bucket_lifecycle("no-bucket").await.unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));

        let err = storage
            .delete_bucket_lifecycle("no-bucket")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_put_bucket_lifecycle_overwrite() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        storage
            .put_bucket_lifecycle("test-bucket", "<v1/>")
            .await
            .unwrap();
        storage
            .put_bucket_lifecycle("test-bucket", "<v2/>")
            .await
            .unwrap();

        let got = storage.get_bucket_lifecycle("test-bucket").await.unwrap();
        assert_eq!(got, "<v2/>");
    }

    // --- is_internal_entry tests for new entries ---

    #[tokio::test]
    async fn test_is_internal_entry_policy() {
        assert!(FileSystemStorage::is_internal_entry(".policy.json"));
    }

    #[tokio::test]
    async fn test_is_internal_entry_acl() {
        assert!(FileSystemStorage::is_internal_entry(".acl.xml"));
        assert!(FileSystemStorage::is_internal_entry(".acls"));
    }

    #[tokio::test]
    async fn test_is_internal_entry_lifecycle() {
        assert!(FileSystemStorage::is_internal_entry(".lifecycle.xml"));
    }

    // --- Bucket with policy/acl/lifecycle can still be deleted ---

    #[tokio::test]
    async fn test_delete_bucket_with_policy_and_acl_and_lifecycle() {
        let (storage, _tmp) = test_storage().await;
        storage
            .create_bucket("test-bucket", "us-east-1")
            .await
            .unwrap();

        storage
            .put_bucket_policy("test-bucket", r#"{"Statement":[]}"#)
            .await
            .unwrap();
        storage
            .put_bucket_acl("test-bucket", "<acl/>")
            .await
            .unwrap();
        storage
            .put_bucket_lifecycle("test-bucket", "<lifecycle/>")
            .await
            .unwrap();

        // Should succeed because policy/acl/lifecycle are internal entries
        storage.delete_bucket("test-bucket").await.unwrap();
        assert!(!storage.bucket_exists("test-bucket"));
    }
}
