use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub creation_date: DateTime<Utc>,
    pub region: String,
}

impl Bucket {
    pub fn new(name: String, region: String) -> Self {
        Self {
            name,
            creation_date: Utc::now(),
            region,
        }
    }
}
