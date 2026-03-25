use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use md5::{Digest, Md5};
use tokio::fs;

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
        name == ".bucket-metadata.json" || name == ".meta"
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

        // Compute ETag (quoted hex MD5)
        let mut hasher = Md5::new();
        hasher.update(body);
        let digest = hasher.finalize();
        let etag = format!("\"{}\"", hex::encode(digest));

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

        Ok(metadata)
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(ObjectMetadata, Vec<u8>), S3Error> {
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

        let metadata = self.read_object_metadata(bucket, key).await?;
        let body = fs::read(&obj_path)
            .await
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to read object: {e}"),
            })?;

        Ok((metadata, body))
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMetadata, S3Error> {
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

        self.read_object_metadata(bucket, key).await
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket) {
            return Err(S3Error::NoSuchBucket {
                bucket_name: bucket.to_string(),
            });
        }

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

        // Clean up empty parent dirs (but not bucket dir itself)
        let bucket_path = self.bucket_path(bucket);
        self.cleanup_empty_parents(&obj_path, &bucket_path).await;
        self.cleanup_empty_parents(&meta_path, &bucket_path).await;

        Ok(())
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
        let (src_meta, body) = self.get_object(src_bucket, src_key).await?;

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
            self.delete_object(bucket, key).await?;
            deleted.push(key.clone());
        }

        Ok(deleted)
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
            .get_object("test-bucket", "greeting.txt")
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
            .get_object("no-bucket", "key.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, S3Error::NoSuchBucket { .. }));
    }

    #[tokio::test]
    async fn test_get_object_no_key() {
        let (storage, _tmp) = test_storage().await;
        create_test_bucket(&storage).await;

        let err = storage
            .get_object("test-bucket", "missing.txt")
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
            .head_object("test-bucket", "info.txt")
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
            .head_object("test-bucket", "missing.txt")
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
            .delete_object("test-bucket", "doomed.txt")
            .await
            .unwrap();

        // Get should now fail with NoSuchKey
        let err = storage
            .get_object("test-bucket", "doomed.txt")
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
            .delete_object("test-bucket", "nonexistent.txt")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_delete_object_no_bucket() {
        let (storage, _tmp) = test_storage().await;

        let err = storage
            .delete_object("no-bucket", "key.txt")
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
            .get_object("test-bucket", "path/to/deep/file.txt")
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
            .delete_object("test-bucket", "a/b/c/file.txt")
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
            .head_object("test-bucket", "meta.txt")
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

        let (meta, body) = storage.get_object("test-bucket", "file.txt").await.unwrap();
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
            .get_object("test-bucket", "empty.txt")
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
            .delete_object("test-bucket", "temp.txt")
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
            .get_object("test-bucket", "original.txt")
            .await
            .unwrap();
        let (_, body2) = storage.get_object("test-bucket", "copy.txt").await.unwrap();
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
            .get_object("other-bucket", "dest.txt")
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
}
