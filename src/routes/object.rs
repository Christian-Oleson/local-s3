use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use quick_xml::se::to_string as to_xml_string;
use serde::Deserialize;

use crate::error::S3Error;
use crate::server::AppState;
use crate::types::xml::{
    CompleteMultipartUploadRequest, CompleteMultipartUploadResult, CopyObjectResult,
    InitiateMultipartUploadResult, ListPartsResult, PartEntry, S3_NAMESPACE, Tag, TagSet, Tagging,
};

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

fn xml_response(xml: String) -> Response {
    let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}");
    (StatusCode::OK, [("content-type", "application/xml")], body).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct ObjectQuery {
    #[serde(rename = "uploadId")]
    pub upload_id: Option<String>,
    #[serde(rename = "partNumber")]
    pub part_number: Option<i32>,
    pub uploads: Option<String>,
    pub tagging: Option<String>,
    #[serde(rename = "versionId")]
    pub version_id: Option<String>,
    pub acl: Option<String>,
}

pub async fn put_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    Query(query): Query<ObjectQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    // If tagging query param is present, this is PutObjectTagging
    if query.tagging.is_some() {
        return handle_put_object_tagging(&state, &bucket_name, key, &body).await;
    }

    // If acl query param is present, this is PutObjectAcl
    if query.acl.is_some() {
        return handle_put_object_acl(&state, &bucket_name, key, &body).await;
    }

    // If partNumber AND uploadId are present, this is an UploadPart request
    if let (Some(part_number), Some(upload_id)) = (query.part_number, &query.upload_id) {
        let etag = state
            .storage
            .upload_part(&bucket_name, upload_id, part_number, &body)
            .await?;

        return Ok((StatusCode::OK, [("etag", etag.as_str())], "").into_response());
    }

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

    let mut response = (StatusCode::OK, [("etag", metadata.etag.as_str())], "").into_response();
    if let Some(ref vid) = metadata.version_id {
        response.headers_mut().insert(
            "x-amz-version-id",
            HeaderValue::from_str(vid).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }
    Ok(response)
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
    Query(query): Query<ObjectQuery>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    // If tagging query param is present, this is GetObjectTagging
    if query.tagging.is_some() {
        return handle_get_object_tagging(&state, &bucket_name, key).await;
    }

    // If acl query param is present, this is GetObjectAcl
    if query.acl.is_some() {
        return handle_get_object_acl(&state, &bucket_name, key).await;
    }

    // If uploadId is present, this is a ListParts request
    if let Some(ref upload_id) = query.upload_id {
        let upload_state = state.storage.list_parts(&bucket_name, upload_id).await?;

        let mut parts: Vec<PartEntry> = upload_state
            .parts
            .values()
            .map(|p| PartEntry {
                part_number: p.part_number,
                last_modified: p.last_modified.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                etag: p.etag.clone(),
                size: p.size,
            })
            .collect();
        parts.sort_by_key(|p| p.part_number);

        let result = ListPartsResult {
            xmlns: S3_NAMESPACE.to_string(),
            bucket: bucket_name,
            key: key.to_string(),
            upload_id: upload_id.clone(),
            max_parts: 1000,
            is_truncated: false,
            parts,
        };

        let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize ListPartsResult: {e}"),
        })?;

        return Ok(xml_response(xml));
    }

    let (metadata, body) = state
        .storage
        .get_object(&bucket_name, key, query.version_id.as_deref())
        .await?;

    // Check conditional request headers
    if let Some(resp) = check_conditionals(&headers, &metadata) {
        return Ok(resp);
    }

    let total_size = body.len() as u64;
    let hdrs = metadata_headers(&metadata);

    // Check for Range header
    if let Some(range_value) = headers.get("range").and_then(|v| v.to_str().ok()) {
        return handle_range_request(range_value, &body, total_size, &hdrs, key);
    }

    let header_map = build_header_map(&hdrs);

    let mut response = (StatusCode::OK, body).into_response();
    let resp_headers = response.headers_mut();
    resp_headers.insert("accept-ranges", HeaderValue::from_static("bytes"));
    for (k, v) in header_map.iter() {
        resp_headers.insert(k, v.clone());
    }
    if let Some(ref vid) = metadata.version_id {
        resp_headers.insert(
            "x-amz-version-id",
            HeaderValue::from_str(vid).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }

    Ok(response)
}

pub async fn head_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    Query(query): Query<ObjectQuery>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    let metadata = state
        .storage
        .head_object(&bucket_name, key, query.version_id.as_deref())
        .await?;

    // Check conditional request headers
    if let Some(resp) = check_conditionals(&headers, &metadata) {
        return Ok(resp);
    }

    let hdrs = metadata_headers(&metadata);
    let header_map = build_header_map(&hdrs);

    let mut response = (StatusCode::OK, "").into_response();
    let resp_headers = response.headers_mut();
    resp_headers.insert("accept-ranges", HeaderValue::from_static("bytes"));
    for (k, v) in header_map.iter() {
        resp_headers.insert(k, v.clone());
    }
    if let Some(ref vid) = metadata.version_id {
        resp_headers.insert(
            "x-amz-version-id",
            HeaderValue::from_str(vid).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }

    Ok(response)
}

pub async fn delete_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    Query(query): Query<ObjectQuery>,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    // If tagging query param is present, this is DeleteObjectTagging
    if query.tagging.is_some() {
        state
            .storage
            .delete_object_tagging(&bucket_name, key)
            .await?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // If uploadId is present, this is an AbortMultipartUpload request
    if let Some(ref upload_id) = query.upload_id {
        state
            .storage
            .abort_multipart_upload(&bucket_name, upload_id)
            .await?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let result = state
        .storage
        .delete_object(&bucket_name, key, query.version_id.as_deref())
        .await?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(ref vid) = result.version_id {
        response.headers_mut().insert(
            "x-amz-version-id",
            HeaderValue::from_str(vid).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }
    if result.is_delete_marker {
        response
            .headers_mut()
            .insert("x-amz-delete-marker", HeaderValue::from_static("true"));
    }

    Ok(response)
}

/// POST handler for object-level operations (multipart upload).
pub async fn post_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    Query(query): Query<ObjectQuery>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let key = normalize_key(&key);

    // If query has "uploads" parameter, this is CreateMultipartUpload
    if query.uploads.is_some() {
        let upload_id = state
            .storage
            .create_multipart_upload(&bucket_name, key)
            .await?;

        let result = InitiateMultipartUploadResult {
            xmlns: S3_NAMESPACE.to_string(),
            bucket: bucket_name,
            key: key.to_string(),
            upload_id,
        };

        let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize InitiateMultipartUploadResult: {e}"),
        })?;

        return Ok(xml_response(xml));
    }

    // If query has "uploadId", this is CompleteMultipartUpload
    if let Some(ref upload_id) = query.upload_id {
        let request: CompleteMultipartUploadRequest = quick_xml::de::from_reader(body.as_ref())
            .map_err(|e| S3Error::InternalError {
                message: format!("Failed to parse CompleteMultipartUpload request: {e}"),
            })?;

        let parts: Vec<(i32, String)> = request
            .parts
            .into_iter()
            .map(|p| (p.part_number, p.etag))
            .collect();

        let metadata = state
            .storage
            .complete_multipart_upload(&bucket_name, key, upload_id, parts)
            .await?;

        let result = CompleteMultipartUploadResult {
            xmlns: S3_NAMESPACE.to_string(),
            location: format!("/{bucket_name}/{key}"),
            bucket: bucket_name,
            key: key.to_string(),
            etag: metadata.etag,
        };

        let xml = to_xml_string(&result).map_err(|e| S3Error::InternalError {
            message: format!("Failed to serialize CompleteMultipartUploadResult: {e}"),
        })?;

        return Ok(xml_response(xml));
    }

    Err(S3Error::InternalError {
        message: "Unsupported POST operation on object".to_string(),
    })
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

// --- Range request handling ---

/// Parse an HTTP Range header value (e.g. "bytes=0-499", "bytes=500-", "bytes=-200").
/// Returns (start, end_inclusive) for the requested range, or an error.
fn parse_range(range_header: &str, total_size: u64) -> Result<(u64, u64), ()> {
    let range_str = range_header.strip_prefix("bytes=").ok_or(())?;

    if let Some(suffix) = range_str.strip_prefix('-') {
        // bytes=-SUFFIX: last N bytes
        let suffix_len: u64 = suffix.parse().map_err(|_| ())?;
        if suffix_len == 0 || suffix_len > total_size {
            return Err(());
        }
        let start = total_size - suffix_len;
        Ok((start, total_size - 1))
    } else if let Some(start_str) = range_str.strip_suffix('-') {
        // bytes=START-: from start to end
        let start: u64 = start_str.parse().map_err(|_| ())?;
        if start >= total_size {
            return Err(());
        }
        Ok((start, total_size - 1))
    } else {
        // bytes=START-END
        let parts: Vec<&str> = range_str.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Err(());
        }
        let start: u64 = parts[0].parse().map_err(|_| ())?;
        let end: u64 = parts[1].parse().map_err(|_| ())?;
        if start > end || start >= total_size {
            return Err(());
        }
        // Clamp end to total_size - 1
        let end = end.min(total_size - 1);
        Ok((start, end))
    }
}

fn handle_range_request(
    range_header: &str,
    body: &[u8],
    total_size: u64,
    hdrs: &[(String, String)],
    key: &str,
) -> Result<Response, S3Error> {
    let (start, end) =
        parse_range(range_header, total_size).map_err(|()| S3Error::InvalidRange {
            key: key.to_string(),
        })?;

    let partial_body = body[start as usize..=end as usize].to_vec();
    let content_range = format!("bytes {start}-{end}/{total_size}");
    let content_length = (end - start + 1).to_string();

    // Build response headers, replacing content-length with partial length
    let mut header_map = HeaderMap::new();
    for (k, v) in hdrs {
        if k == "content-length" {
            continue; // We'll set our own
        }
        if let (Ok(name), Ok(val)) = (
            k.parse::<axum::http::HeaderName>(),
            HeaderValue::from_str(v),
        ) {
            header_map.insert(name, val);
        }
    }

    let mut response = (StatusCode::PARTIAL_CONTENT, partial_body).into_response();
    let resp_headers = response.headers_mut();
    resp_headers.insert("accept-ranges", HeaderValue::from_static("bytes"));
    resp_headers.insert(
        "content-range",
        HeaderValue::from_str(&content_range).unwrap(),
    );
    resp_headers.insert(
        "content-length",
        HeaderValue::from_str(&content_length).unwrap(),
    );
    for (k, v) in header_map.iter() {
        resp_headers.insert(k, v.clone());
    }

    Ok(response)
}

// --- Conditional request handling ---

/// Parse an HTTP date string (RFC 7231 format).
fn parse_http_date(s: &str) -> Option<DateTime<Utc>> {
    // RFC 7231 format: "Thu, 01 Jan 2024 00:00:00 GMT"
    // Strip trailing " GMT" and parse as NaiveDateTime, then assign UTC
    let stripped = s.strip_suffix(" GMT")?;
    let naive = chrono::NaiveDateTime::parse_from_str(stripped, "%a, %d %b %Y %H:%M:%S").ok()?;
    Some(naive.and_utc())
}

/// Check If-None-Match and If-Modified-Since headers against object metadata.
/// Returns Some(304 response) if conditions indicate the client has a fresh copy.
fn check_conditionals(
    headers: &HeaderMap,
    metadata: &crate::types::object::ObjectMetadata,
) -> Option<Response> {
    let if_none_match = headers.get("if-none-match").and_then(|v| v.to_str().ok());
    let if_modified_since = headers
        .get("if-modified-since")
        .and_then(|v| v.to_str().ok());

    // If-None-Match takes precedence
    if let Some(inm) = if_none_match {
        // Check if any of the ETags match (can be comma-separated list or *)
        let etags: Vec<&str> = inm.split(',').map(|s| s.trim()).collect();
        for etag in &etags {
            if *etag == "*" || *etag == metadata.etag {
                return Some(build_304_response(metadata));
            }
        }
        // If-None-Match was present but didn't match, skip If-Modified-Since
        return None;
    }

    // If-Modified-Since
    if let Some(ims) = if_modified_since
        && let Some(since_date) = parse_http_date(ims)
        && metadata.last_modified <= since_date
    {
        return Some(build_304_response(metadata));
    }

    None
}

fn build_304_response(metadata: &crate::types::object::ObjectMetadata) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    let resp_headers = response.headers_mut();
    resp_headers.insert(
        "etag",
        HeaderValue::from_str(&metadata.etag).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    resp_headers.insert(
        "last-modified",
        HeaderValue::from_str(&format_last_modified(&metadata.last_modified))
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    response
}

// --- Object Tagging handlers ---

async fn handle_put_object_tagging(
    state: &AppState,
    bucket: &str,
    key: &str,
    body: &[u8],
) -> Result<Response, S3Error> {
    let tagging: Tagging =
        quick_xml::de::from_reader(body).map_err(|e| S3Error::InternalError {
            message: format!("Failed to parse Tagging XML: {e}"),
        })?;

    let mut tags = HashMap::new();
    for tag in tagging.tag_set.tags {
        tags.insert(tag.key, tag.value);
    }

    state.storage.put_object_tagging(bucket, key, tags).await?;

    Ok((StatusCode::OK, [("content-length", "0")], "").into_response())
}

async fn handle_get_object_tagging(
    state: &AppState,
    bucket: &str,
    key: &str,
) -> Result<Response, S3Error> {
    let tags = state.storage.get_object_tagging(bucket, key).await?;

    let tagging = Tagging {
        tag_set: TagSet {
            tags: tags
                .into_iter()
                .map(|(k, v)| Tag { key: k, value: v })
                .collect(),
        },
    };

    let xml = to_xml_string(&tagging).map_err(|e| S3Error::InternalError {
        message: format!("Failed to serialize Tagging: {e}"),
    })?;

    Ok(xml_response(xml))
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

// --- Object ACL handlers ---

async fn handle_put_object_acl(
    state: &AppState,
    bucket: &str,
    key: &str,
    body: &[u8],
) -> Result<Response, S3Error> {
    let acl_xml = std::str::from_utf8(body).map_err(|e| S3Error::InternalError {
        message: format!("Invalid UTF-8 in ACL body: {e}"),
    })?;

    state.storage.put_object_acl(bucket, key, acl_xml).await?;

    Ok((StatusCode::OK, [("content-length", "0")], "").into_response())
}

async fn handle_get_object_acl(
    state: &AppState,
    bucket: &str,
    key: &str,
) -> Result<Response, S3Error> {
    let acl = state.storage.get_object_acl(bucket, key).await?;

    Ok((StatusCode::OK, [("content-type", "application/xml")], acl).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    // --- Range parsing tests ---

    #[test]
    fn test_parse_range_start_end() {
        let (start, end) = parse_range("bytes=0-499", 1000).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 499);
    }

    #[test]
    fn test_parse_range_start_only() {
        let (start, end) = parse_range("bytes=500-", 1000).unwrap();
        assert_eq!(start, 500);
        assert_eq!(end, 999);
    }

    #[test]
    fn test_parse_range_suffix() {
        let (start, end) = parse_range("bytes=-200", 1000).unwrap();
        assert_eq!(start, 800);
        assert_eq!(end, 999);
    }

    #[test]
    fn test_parse_range_entire_file() {
        let (start, end) = parse_range("bytes=0-999", 1000).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 999);
    }

    #[test]
    fn test_parse_range_end_exceeds_total() {
        // End beyond file size should be clamped
        let (start, end) = parse_range("bytes=500-2000", 1000).unwrap();
        assert_eq!(start, 500);
        assert_eq!(end, 999);
    }

    #[test]
    fn test_parse_range_invalid_no_bytes_prefix() {
        assert!(parse_range("0-499", 1000).is_err());
    }

    #[test]
    fn test_parse_range_start_beyond_total() {
        assert!(parse_range("bytes=1000-", 1000).is_err());
    }

    #[test]
    fn test_parse_range_suffix_exceeds_total() {
        assert!(parse_range("bytes=-1500", 1000).is_err());
    }

    #[test]
    fn test_parse_range_start_greater_than_end() {
        assert!(parse_range("bytes=500-200", 1000).is_err());
    }

    #[test]
    fn test_parse_range_zero_suffix() {
        assert!(parse_range("bytes=-0", 1000).is_err());
    }

    #[test]
    fn test_parse_range_single_byte() {
        let (start, end) = parse_range("bytes=0-0", 100).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn test_parse_range_last_byte() {
        let (start, end) = parse_range("bytes=-1", 100).unwrap();
        assert_eq!(start, 99);
        assert_eq!(end, 99);
    }

    // --- HTTP date parsing tests ---

    #[test]
    fn test_parse_http_date_valid() {
        // Test with a date we generate ourselves using format_last_modified
        let now = Utc::now();
        let formatted = format_last_modified(&now);
        let parsed = parse_http_date(&formatted);
        assert!(parsed.is_some(), "Failed to parse: {formatted}");
        let parsed = parsed.unwrap();
        // Seconds-level precision comparison (format drops sub-second)
        assert_eq!(
            parsed.timestamp(),
            now.timestamp(),
            "Parsed: {parsed:?}, Original: {now:?}"
        );
    }

    #[test]
    fn test_parse_http_date_known_date() {
        // Wed, 01 Jan 2025 12:00:00 GMT
        let dt = parse_http_date("Wed, 01 Jan 2025 12:00:00 GMT");
        assert!(dt.is_some(), "Failed to parse known date");
        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2025);
    }

    #[test]
    fn test_parse_http_date_invalid() {
        let dt = parse_http_date("not a date");
        assert!(dt.is_none());
    }
}
