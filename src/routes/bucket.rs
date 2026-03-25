use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use quick_xml::se::to_string as to_xml_string;
use serde::Deserialize;

use crate::error::S3Error;
use crate::server::AppState;
use crate::types::xml::{
    BucketEntry, Buckets, CommonPrefix, CreateBucketConfiguration, DeleteResult, DeletedEntry,
    ListAllMyBucketsResult, ListObjectsV1Result, ListObjectsV2Result, LocationConstraintResponse,
    ObjectEntry, Owner, S3_NAMESPACE,
};

fn xml_response(xml: String) -> Response {
    let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}");
    (StatusCode::OK, [("content-type", "application/xml")], body).into_response()
}

pub async fn list_buckets(State(state): State<AppState>) -> Result<Response, S3Error> {
    let buckets = state.storage.list_buckets().await?;

    let result = ListAllMyBucketsResult {
        xmlns: S3_NAMESPACE.to_string(),
        owner: Owner::default_owner(),
        buckets: Buckets {
            entries: buckets
                .iter()
                .map(|b| BucketEntry {
                    name: b.name.clone(),
                    creation_date: b.creation_date.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                })
                .collect(),
        },
    };

    let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize response: {e}"),
    })?;

    Ok(xml_response(xml))
}

pub async fn create_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let region = if body.is_empty() {
        "us-east-1".to_string()
    } else {
        let config: CreateBucketConfiguration = quick_xml::de::from_reader(body.as_ref())
            .unwrap_or(CreateBucketConfiguration {
                location_constraint: None,
            });
        config
            .location_constraint
            .unwrap_or_else(|| "us-east-1".to_string())
    };

    state.storage.create_bucket(&bucket_name, &region).await?;

    Ok((
        StatusCode::OK,
        [("location", format!("/{bucket_name}").as_str())],
        "",
    )
        .into_response())
}

pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Response, S3Error> {
    state.storage.delete_bucket(&bucket_name).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Response, S3Error> {
    let bucket = state.storage.head_bucket(&bucket_name).await?;
    Ok((
        StatusCode::OK,
        [("x-amz-bucket-region", bucket.region.as_str())],
        "",
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct BucketGetQuery {
    pub location: Option<String>,
    #[serde(rename = "list-type")]
    pub list_type: Option<String>,
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<i32>,
    #[serde(rename = "start-after")]
    pub start_after: Option<String>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
    pub marker: Option<String>,
}

pub async fn get_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    Query(query): Query<BucketGetQuery>,
) -> Result<Response, S3Error> {
    // Check if this is a ?location request
    // The AWS SDK sends ?location (with no value), so we check if the key exists
    // in the raw query string since serde will deserialize it as Some("")
    if query.location.is_some() {
        return get_bucket_location(state, &bucket_name).await;
    }

    // ListObjects
    let prefix = query.prefix.as_deref().unwrap_or("");
    let delimiter = query.delimiter.as_deref();
    let max_keys = query.max_keys.unwrap_or(1000);

    // Determine start_after depending on V1 vs V2
    let is_v2 = query.list_type.as_deref() == Some("2");

    let start_after = if is_v2 {
        // If continuation-token is provided, base64-decode it as the start_after value
        if let Some(ref token) = query.continuation_token {
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(token)
                .map_err(|e| S3Error::InternalError {
                    message: format!("Invalid continuation-token: {e}"),
                })?;
            Some(
                String::from_utf8(decoded).map_err(|e| S3Error::InternalError {
                    message: format!("Invalid continuation-token encoding: {e}"),
                })?,
            )
        } else {
            query.start_after.clone()
        }
    } else {
        // V1 uses marker
        query.marker.clone()
    };

    let output = state
        .storage
        .list_objects(
            &bucket_name,
            prefix,
            delimiter,
            max_keys,
            start_after.as_deref(),
        )
        .await?;

    let contents: Vec<ObjectEntry> = output
        .objects
        .iter()
        .map(|o| ObjectEntry {
            key: o.key.clone(),
            last_modified: o.last_modified.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            etag: o.etag.clone(),
            size: o.size,
            storage_class: "STANDARD".to_string(),
        })
        .collect();

    let common_prefixes: Vec<CommonPrefix> = output
        .common_prefixes
        .iter()
        .map(|p| CommonPrefix { prefix: p.clone() })
        .collect();

    if is_v2 {
        let key_count = (contents.len() + common_prefixes.len()) as i32;
        let result = ListObjectsV2Result {
            xmlns: S3_NAMESPACE.to_string(),
            name: bucket_name,
            prefix: prefix.to_string(),
            max_keys,
            key_count,
            is_truncated: output.is_truncated,
            delimiter: delimiter.map(|s| s.to_string()),
            start_after: query.start_after,
            continuation_token: query.continuation_token,
            next_continuation_token: output.next_continuation_token,
            contents,
            common_prefixes,
        };
        let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize response: {e}"),
        })?;
        Ok(xml_response(xml))
    } else {
        let result = ListObjectsV1Result {
            xmlns: S3_NAMESPACE.to_string(),
            name: bucket_name,
            prefix: prefix.to_string(),
            marker: query.marker.unwrap_or_default(),
            max_keys,
            is_truncated: output.is_truncated,
            delimiter: delimiter.map(|s| s.to_string()),
            next_marker: if output.is_truncated {
                // For V1, NextMarker is the last key returned
                output
                    .objects
                    .last()
                    .map(|o| o.key.clone())
                    .or_else(|| output.common_prefixes.last().cloned())
            } else {
                None
            },
            contents,
            common_prefixes,
        };
        let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize response: {e}"),
        })?;
        Ok(xml_response(xml))
    }
}

pub async fn delete_objects_handler(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let request: crate::types::xml::DeleteRequest = quick_xml::de::from_reader(body.as_ref())
        .map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse DeleteObjects request: {e}"),
        })?;

    let keys: Vec<String> = request.objects.into_iter().map(|o| o.key).collect();
    let deleted_keys = state.storage.delete_objects(&bucket_name, &keys).await?;

    let result = DeleteResult {
        xmlns: S3_NAMESPACE.to_string(),
        deleted: deleted_keys
            .into_iter()
            .map(|key| DeletedEntry { key })
            .collect(),
    };

    let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize DeleteResult: {e}"),
    })?;

    Ok(xml_response(xml))
}

async fn get_bucket_location(state: AppState, bucket_name: &str) -> Result<Response, S3Error> {
    let region = state.storage.get_bucket_location(bucket_name).await?;

    // S3 behavior: us-east-1 returns empty/null LocationConstraint
    let value = if region == "us-east-1" {
        None
    } else {
        Some(region)
    };

    let loc = LocationConstraintResponse {
        xmlns: S3_NAMESPACE.to_string(),
        value,
    };

    let xml = to_xml_string(&loc).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize response: {e}"),
    })?;

    Ok(xml_response(xml))
}
