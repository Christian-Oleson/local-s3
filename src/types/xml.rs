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

#[derive(Debug, Serialize)]
#[serde(rename = "ListBucketResult")]
pub struct ListBucketResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "MaxKeys")]
    pub max_keys: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
}

impl ListBucketResult {
    pub fn empty(bucket_name: &str) -> Self {
        Self {
            xmlns: S3_NAMESPACE.to_string(),
            name: bucket_name.to_string(),
            prefix: String::new(),
            max_keys: 1000,
            is_truncated: false,
        }
    }
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
    fn test_serialize_empty_list_bucket_result() {
        let result = ListBucketResult::empty("my-bucket");
        let xml = to_xml_string(&result).unwrap();
        assert!(xml.contains("ListBucketResult"));
        assert!(xml.contains("my-bucket"));
        assert!(xml.contains("<IsTruncated>false</IsTruncated>"));
    }
}
