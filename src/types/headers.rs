use axum::http::{HeaderMap, HeaderName, HeaderValue};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use uuid::Uuid;

pub const X_AMZ_REQUEST_ID: &str = "x-amz-request-id";
pub const X_AMZ_ID_2: &str = "x-amz-id-2";
pub const SERVER_HEADER_VALUE: &str = "local-s3";

pub fn generate_request_id() -> String {
    Uuid::new_v4().to_string().replace('-', "").to_uppercase()
}

pub fn generate_id_2() -> String {
    let bytes: [u8; 24] = rand_bytes();
    STANDARD.encode(bytes)
}

fn rand_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    for (i, byte) in buf.iter_mut().enumerate() {
        // Simple deterministic-enough random for local dev using request id entropy
        *byte = (uuid::Uuid::new_v4().as_bytes()[i % 16]) ^ (i as u8);
    }
    buf
}

pub fn s3_headers(request_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static(X_AMZ_REQUEST_ID),
        HeaderValue::from_str(request_id).unwrap_or_else(|_| HeaderValue::from_static("unknown")),
    );
    headers.insert(
        HeaderName::from_static(X_AMZ_ID_2),
        HeaderValue::from_str(&generate_id_2())
            .unwrap_or_else(|_| HeaderValue::from_static("unknown")),
    );
    headers.insert(
        axum::http::header::SERVER,
        HeaderValue::from_static(SERVER_HEADER_VALUE),
    );
    headers
}
