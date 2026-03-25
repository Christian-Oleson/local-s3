use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use quick_xml::se::to_string as to_xml_string;
use serde::Deserialize;

use crate::error::S3Error;
use crate::server::AppState;
use crate::types::xml::{
    BucketEntry, Buckets, CreateBucketConfiguration, ListAllMyBucketsResult, ListBucketResult,
    LocationConstraintResponse, Owner, S3_NAMESPACE,
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
}

pub async fn get_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    Query(query): Query<BucketGetQuery>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    // Check if this is a ?location request
    // The AWS SDK sends ?location (with no value), so we check if the key exists
    // in the raw query string since serde will deserialize it as Some("")
    if query.location.is_some() {
        return get_bucket_location(state, &bucket_name).await;
    }

    // Check for ?delete query (batch delete — handled in Phase 2)
    // Check for other sub-resource queries here in the future

    // Default: ListObjects (return empty for now, Phase 2 will implement)
    // But first check bucket exists
    if !state.storage.bucket_exists(&bucket_name) {
        // Check if this might be a ListBuckets request with a weird path
        return Err(S3Error::NoSuchBucket {
            bucket_name: bucket_name.clone(),
        });
    }

    let _ = headers; // Will be used for pagination params in Phase 2

    let result = ListBucketResult::empty(&bucket_name);
    let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize response: {e}"),
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
