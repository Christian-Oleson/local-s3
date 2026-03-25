use serde::{Deserialize, Serialize};

pub const S3_NAMESPACE: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "ListAllMyBucketsResult")]
pub struct ListAllMyBucketsResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Owner")]
    pub owner: Owner,
    #[serde(rename = "Buckets")]
    pub buckets: Buckets,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Buckets {
    #[serde(rename = "Bucket", default)]
    pub entries: Vec<BucketEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BucketEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CreationDate")]
    pub creation_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Owner {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "DisplayName")]
    pub display_name: String,
}

impl Owner {
    pub fn default_owner() -> Self {
        Self {
            id: "local-s3-owner-id".to_string(),
            display_name: "local-s3".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "CreateBucketConfiguration")]
pub struct CreateBucketConfiguration {
    #[serde(rename = "LocationConstraint", default)]
    pub location_constraint: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "LocationConstraint")]
pub struct LocationConstraintResponse {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "$text")]
    pub value: Option<String>,
}

// --- ListObjects V2 ---

#[derive(Debug, Serialize)]
#[serde(rename = "ListBucketResult")]
pub struct ListObjectsV2Result {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "MaxKeys")]
    pub max_keys: i32,
    #[serde(rename = "KeyCount")]
    pub key_count: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Delimiter", skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    #[serde(rename = "StartAfter", skip_serializing_if = "Option::is_none")]
    pub start_after: Option<String>,
    #[serde(rename = "ContinuationToken", skip_serializing_if = "Option::is_none")]
    pub continuation_token: Option<String>,
    #[serde(
        rename = "NextContinuationToken",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_continuation_token: Option<String>,
    #[serde(rename = "Contents", default)]
    pub contents: Vec<ObjectEntry>,
    #[serde(rename = "CommonPrefixes", default)]
    pub common_prefixes: Vec<CommonPrefix>,
}

#[derive(Debug, Serialize)]
pub struct ObjectEntry {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "Size")]
    pub size: u64,
    #[serde(rename = "StorageClass")]
    pub storage_class: String,
}

#[derive(Debug, Serialize)]
pub struct CommonPrefix {
    #[serde(rename = "Prefix")]
    pub prefix: String,
}

// --- ListObjects V1 ---

#[derive(Debug, Serialize)]
#[serde(rename = "ListBucketResult")]
pub struct ListObjectsV1Result {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "Marker")]
    pub marker: String,
    #[serde(rename = "MaxKeys")]
    pub max_keys: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Delimiter", skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    #[serde(rename = "NextMarker", skip_serializing_if = "Option::is_none")]
    pub next_marker: Option<String>,
    #[serde(rename = "Contents", default)]
    pub contents: Vec<ObjectEntry>,
    #[serde(rename = "CommonPrefixes", default)]
    pub common_prefixes: Vec<CommonPrefix>,
}

// --- Batch Delete ---

#[derive(Debug, Deserialize)]
#[serde(rename = "Delete")]
pub struct DeleteRequest {
    #[serde(rename = "Object")]
    pub objects: Vec<DeleteObjectEntry>,
    #[serde(rename = "Quiet", default)]
    pub quiet: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteObjectEntry {
    #[serde(rename = "Key")]
    pub key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename = "DeleteResult")]
pub struct DeleteResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Deleted", default)]
    pub deleted: Vec<DeletedEntry>,
}

#[derive(Debug, Serialize)]
pub struct DeletedEntry {
    #[serde(rename = "Key")]
    pub key: String,
}

// --- CopyObject ---

#[derive(Debug, Serialize)]
#[serde(rename = "CopyObjectResult")]
pub struct CopyObjectResult {
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
}

// --- Multipart Upload ---

// CreateMultipartUpload response
#[derive(Debug, Serialize)]
#[serde(rename = "InitiateMultipartUploadResult")]
pub struct InitiateMultipartUploadResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "UploadId")]
    pub upload_id: String,
}

// CompleteMultipartUpload request body
#[derive(Debug, Deserialize)]
#[serde(rename = "CompleteMultipartUpload")]
pub struct CompleteMultipartUploadRequest {
    #[serde(rename = "Part")]
    pub parts: Vec<CompletePart>,
}

#[derive(Debug, Deserialize)]
pub struct CompletePart {
    #[serde(rename = "PartNumber")]
    pub part_number: i32,
    #[serde(rename = "ETag")]
    pub etag: String,
}

// CompleteMultipartUpload response
#[derive(Debug, Serialize)]
#[serde(rename = "CompleteMultipartUploadResult")]
pub struct CompleteMultipartUploadResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Location")]
    pub location: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "ETag")]
    pub etag: String,
}

// ListParts response
#[derive(Debug, Serialize)]
#[serde(rename = "ListPartsResult")]
pub struct ListPartsResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "UploadId")]
    pub upload_id: String,
    #[serde(rename = "MaxParts")]
    pub max_parts: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Part", default)]
    pub parts: Vec<PartEntry>,
}

#[derive(Debug, Serialize)]
pub struct PartEntry {
    #[serde(rename = "PartNumber")]
    pub part_number: i32,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "Size")]
    pub size: u64,
}

// ListMultipartUploads response
#[derive(Debug, Serialize)]
#[serde(rename = "ListMultipartUploadsResult")]
pub struct ListMultipartUploadsResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "MaxUploads")]
    pub max_uploads: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Upload", default)]
    pub uploads: Vec<UploadEntry>,
}

#[derive(Debug, Serialize)]
pub struct UploadEntry {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "UploadId")]
    pub upload_id: String,
    #[serde(rename = "Initiated")]
    pub initiated: String,
}

// --- Object Tagging ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "Tagging")]
pub struct Tagging {
    #[serde(rename = "TagSet")]
    pub tag_set: TagSet,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TagSet {
    #[serde(rename = "Tag", default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

// --- CORS Configuration ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "CORSConfiguration")]
pub struct CORSConfiguration {
    #[serde(rename = "CORSRule")]
    pub rules: Vec<CORSRuleXml>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CORSRuleXml {
    #[serde(rename = "AllowedOrigin")]
    pub allowed_origin: Vec<String>,
    #[serde(rename = "AllowedMethod")]
    pub allowed_method: Vec<String>,
    #[serde(rename = "AllowedHeader", default)]
    pub allowed_header: Vec<String>,
    #[serde(rename = "MaxAgeSeconds", skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<i32>,
    #[serde(rename = "ExposeHeader", default)]
    pub expose_header: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::se::to_string as to_xml_string;

    #[test]
    fn test_serialize_list_buckets_result() {
        let result = ListAllMyBucketsResult {
            xmlns: S3_NAMESPACE.to_string(),
            owner: Owner::default_owner(),
            buckets: Buckets {
                entries: vec![BucketEntry {
                    name: "test-bucket".to_string(),
                    creation_date: "2024-01-01T00:00:00.000Z".to_string(),
                }],
            },
        };
        let xml = to_xml_string(&result).unwrap();
        assert!(xml.contains("ListAllMyBucketsResult"));
        assert!(xml.contains("test-bucket"));
        assert!(xml.contains("2024-01-01T00:00:00.000Z"));
        assert!(xml.contains(S3_NAMESPACE));
    }

    #[test]
    fn test_serialize_error_response() {
        #[derive(Serialize)]
        #[serde(rename = "Error")]
        struct ErrorResponse {
            #[serde(rename = "Code")]
            code: String,
            #[serde(rename = "Message")]
            message: String,
        }
        let err = ErrorResponse {
            code: "NoSuchBucket".to_string(),
            message: "The specified bucket does not exist".to_string(),
        };
        let xml = to_xml_string(&err).unwrap();
        assert!(xml.contains("<Error>"));
        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
        assert!(xml.contains("<Message>"));
    }

    #[test]
    fn test_deserialize_create_bucket_config() {
        let xml = r#"<CreateBucketConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><LocationConstraint>us-west-2</LocationConstraint></CreateBucketConfiguration>"#;
        let config: CreateBucketConfiguration = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(config.location_constraint, Some("us-west-2".to_string()));
    }

    #[test]
    fn test_serialize_location_constraint() {
        let loc = LocationConstraintResponse {
            xmlns: S3_NAMESPACE.to_string(),
            value: Some("us-west-2".to_string()),
        };
        let xml = to_xml_string(&loc).unwrap();
        assert!(xml.contains("LocationConstraint"));
        assert!(xml.contains("us-west-2"));
    }

    #[test]
    fn test_serialize_empty_list_objects_v2_result() {
        let result = ListObjectsV2Result {
            xmlns: S3_NAMESPACE.to_string(),
            name: "my-bucket".to_string(),
            prefix: String::new(),
            max_keys: 1000,
            key_count: 0,
            is_truncated: false,
            delimiter: None,
            start_after: None,
            continuation_token: None,
            next_continuation_token: None,
            contents: vec![],
            common_prefixes: vec![],
        };
        let xml = to_xml_string(&result).unwrap();
        assert!(xml.contains("ListBucketResult"));
        assert!(xml.contains("my-bucket"));
        assert!(xml.contains("<IsTruncated>false</IsTruncated>"));
        assert!(xml.contains("<KeyCount>0</KeyCount>"));
    }

    #[test]
    fn test_serialize_list_objects_v1_result() {
        let result = ListObjectsV1Result {
            xmlns: S3_NAMESPACE.to_string(),
            name: "my-bucket".to_string(),
            prefix: String::new(),
            marker: String::new(),
            max_keys: 1000,
            is_truncated: false,
            delimiter: None,
            next_marker: None,
            contents: vec![ObjectEntry {
                key: "test.txt".to_string(),
                last_modified: "2024-01-01T00:00:00.000Z".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 100,
                storage_class: "STANDARD".to_string(),
            }],
            common_prefixes: vec![],
        };
        let xml = to_xml_string(&result).unwrap();
        assert!(xml.contains("ListBucketResult"));
        assert!(xml.contains("test.txt"));
        assert!(xml.contains("<Size>100</Size>"));
    }
}
