use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    pub key: String,
    pub content_type: String,
    pub content_length: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
    pub custom_metadata: HashMap<String, String>,
    #[serde(default)]
    pub content_disposition: Option<String>,
    #[serde(default)]
    pub cache_control: Option<String>,
    #[serde(default)]
    pub content_encoding: Option<String>,
    #[serde(default)]
    pub expires: Option<String>,
}
