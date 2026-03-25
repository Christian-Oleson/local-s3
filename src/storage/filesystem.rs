use std::path::PathBuf;

use tokio::fs;

use crate::error::S3Error;
use crate::types::bucket::Bucket;

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
            if name_str != ".bucket-metadata.json" {
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
}
