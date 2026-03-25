use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::types::headers;

pub async fn s3_headers_middleware(request: Request, next: Next) -> Response {
    let request_id = headers::generate_request_id();
    let mut response = next.run(request).await;

    let headers_mut = response.headers_mut();
    for (key, value) in headers::s3_headers(&request_id) {
        if let Some(key) = key {
            headers_mut.insert(key, value);
        }
    }

    response
}
