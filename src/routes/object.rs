use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::error::S3Error;
use crate::server::AppState;
use crate::types::xml::CopyObjectResult;

/// Strip leading "/" from wildcard key capture (axum includes it).
fn normalize_key(key: &str) -> &str {
    key.strip_prefix('/').unwrap_or(key)
}

/// Extract x-amz-meta-* custom metadata headers.
/// Strips the "x-amz-meta-" prefix when storing.
fn extract_custom_metadata(headers: &HeaderMap) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if let Some(meta_key) = name_str.strip_prefix("x-amz-meta-")
            && let Ok(v) = value.to_str()
        {
            metadata.insert(meta_key.to_string(), v.to_string());
        }
    }
    metadata
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn put_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    // If x-amz-copy-source is present, handle CopyObject
    if let Some(copy_source) = headers.get("x-amz-copy-source") {
        return handle_copy_object(&state, copy_source, &bucket_name, key).await;
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let custom_metadata = extract_custom_metadata(&headers);
    let content_disposition = header_string(&headers, "content-disposition");
    let cache_control = header_string(&headers, "cache-control");
    let content_encoding = header_string(&headers, "content-encoding");
    let expires = header_string(&headers, "expires");

    let metadata = state
        .storage
        .put_object(
            &bucket_name,
            key,
            &body,
            content_type,
            custom_metadata,
            content_disposition,
            cache_control,
            content_encoding,
            expires,
        )
        .await?;

    Ok((StatusCode::OK, [("etag", metadata.etag.as_str())], "").into_response())
}

/// Format a DateTime<Utc> as RFC 7231: "Thu, 01 Jan 2024 00:00:00 GMT"
fn format_last_modified(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Build response headers from ObjectMetadata.
fn metadata_headers(metadata: &crate::types::object::ObjectMetadata) -> Vec<(String, String)> {
    let mut hdrs = vec![
        ("content-type".to_string(), metadata.content_type.clone()),
        (
            "content-length".to_string(),
            metadata.content_length.to_string(),
        ),
        ("etag".to_string(), metadata.etag.clone()),
        (
            "last-modified".to_string(),
            format_last_modified(&metadata.last_modified),
        ),
    ];

    // Re-add x-amz-meta-* prefix for custom metadata
    for (k, v) in &metadata.custom_metadata {
        hdrs.push((format!("x-amz-meta-{k}"), v.clone()));
    }

    if let Some(ref cd) = metadata.content_disposition {
        hdrs.push(("content-disposition".to_string(), cd.clone()));
    }
    if let Some(ref cc) = metadata.cache_control {
        hdrs.push(("cache-control".to_string(), cc.clone()));
    }
    if let Some(ref ce) = metadata.content_encoding {
        hdrs.push(("content-encoding".to_string(), ce.clone()));
    }
    if let Some(ref exp) = metadata.expires {
        hdrs.push(("expires".to_string(), exp.clone()));
    }

    hdrs
}

fn build_header_map(pairs: &[(String, String)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in pairs {
        if let (Ok(name), Ok(val)) = (
            k.parse::<axum::http::HeaderName>(),
            HeaderValue::from_str(v),
        ) {
            map.insert(name, val);
        }
    }
    map
}

pub async fn get_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    let (metadata, body) = state.storage.get_object(&bucket_name, key).await?;

    let hdrs = metadata_headers(&metadata);
    let header_map = build_header_map(&hdrs);

    let mut response = (StatusCode::OK, body).into_response();
    let resp_headers = response.headers_mut();
    for (k, v) in header_map.iter() {
        resp_headers.insert(k, v.clone());
    }

    Ok(response)
}

pub async fn head_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    let metadata = state.storage.head_object(&bucket_name, key).await?;

    let hdrs = metadata_headers(&metadata);
    let header_map = build_header_map(&hdrs);

    let mut response = (StatusCode::OK, "").into_response();
    let resp_headers = response.headers_mut();
    for (k, v) in header_map.iter() {
        resp_headers.insert(k, v.clone());
    }

    Ok(response)
}

pub async fn delete_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    state.storage.delete_object(&bucket_name, key).await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Handle PUT with x-amz-copy-source header (CopyObject).
async fn handle_copy_object(
    state: &AppState,
    copy_source: &HeaderValue,
    dst_bucket: &str,
    dst_key: &str,
) -> Result<Response, S3Error> {
    let source_str = copy_source.to_str().map_err(|_| S3Error::InternalError {
        message: "Invalid x-amz-copy-source header value".to_string(),
    })?;

    // URL-decode the value
    let decoded = percent_decode(source_str);

    // Strip leading "/"
    let stripped = decoded.strip_prefix('/').unwrap_or(&decoded);

    // Split on first "/" to get bucket and key
    let (src_bucket, src_key) = stripped.split_once('/').ok_or(S3Error::InternalError {
        message: format!("Invalid x-amz-copy-source format: {source_str}"),
    })?;

    let metadata = state
        .storage
        .copy_object(src_bucket, src_key, dst_bucket, dst_key)
        .await?;

    let result = CopyObjectResult {
        etag: metadata.etag,
        last_modified: metadata
            .last_modified
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    };

    let xml = quick_xml::se::to_string(&result).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize CopyObjectResult: {e}"),
    })?;

    let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}");
    Ok((StatusCode::OK, [("content-type", "application/xml")], body).into_response())
}

/// Simple percent-decoding for copy source paths.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = [h, l];
                if let Ok(s) = std::str::from_utf8(&hex)
                    && let Ok(byte) = u8::from_str_radix(s, 16)
                {
                    result.push(byte as char);
                    continue;
                }
            }
            // If decoding fails, push the original characters
            result.push('%');
        } else {
            result.push(b as char);
        }
    }
    result
}
