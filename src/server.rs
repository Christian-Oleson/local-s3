use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use tokio::net::TcpListener;

use crate::middleware::s3_headers_middleware;
use crate::routes::bucket;
use crate::routes::object;
use crate::secretsmanager;
use crate::secretsmanager::storage::SecretsStorage;
use crate::storage::FileSystemStorage;

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<FileSystemStorage>,
    pub secrets_storage: Arc<SecretsStorage>,
}

async fn fallback(req: Request) -> impl IntoResponse {
    tracing::warn!(
        "Unmatched request: {} {} (headers: {:?})",
        req.method(),
        req.uri(),
        req.headers()
    );
    (
        axum::http::StatusCode::NOT_FOUND,
        format!("Not Found: {} {}", req.method(), req.uri()),
    )
}

/// Handle OPTIONS preflight requests for CORS.
/// If the bucket has CORS configured, returns permissive CORS headers reflecting the Origin.
async fn options_bucket_handler(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    headers: HeaderMap,
) -> Response {
    cors_preflight_response(&state, &bucket_name, &headers).await
}

async fn options_object_handler(
    State(state): State<AppState>,
    Path((bucket_name, _key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    cors_preflight_response(&state, &bucket_name, &headers).await
}

async fn cors_preflight_response(
    state: &AppState,
    bucket_name: &str,
    headers: &HeaderMap,
) -> Response {
    // Check if bucket has CORS config
    let has_cors = state.storage.get_bucket_cors(bucket_name).await.is_ok();

    if !has_cors {
        return StatusCode::FORBIDDEN.into_response();
    }

    // For local dev, be permissive and reflect the Origin
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("*");

    let request_method = headers
        .get("access-control-request-method")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("GET");

    let request_headers = headers
        .get("access-control-request-headers")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("*");

    let mut response = StatusCode::OK.into_response();
    let resp_headers = response.headers_mut();
    resp_headers.insert(
        "access-control-allow-origin",
        HeaderValue::from_str(origin).unwrap_or_else(|_| HeaderValue::from_static("*")),
    );
    resp_headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_str(request_method)
            .unwrap_or_else(|_| HeaderValue::from_static("GET, PUT, POST, DELETE, HEAD")),
    );
    resp_headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_str(request_headers).unwrap_or_else(|_| HeaderValue::from_static("*")),
    );
    resp_headers.insert("access-control-max-age", HeaderValue::from_static("3600"));

    response
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "healthy")
}

/// Handle POST / — dispatches to Secrets Manager if X-Amz-Target header is present,
/// otherwise returns Method Not Allowed (S3 list_buckets only supports GET on /).
async fn secretsmanager_post_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok())
        && let Some(operation) = target.strip_prefix("secretsmanager.")
    {
        return secretsmanager::dispatcher::dispatch(&state.secrets_storage, operation, body).await;
    }
    // Not a Secrets Manager request — POST on / is not a valid S3 operation
    (StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response()
}

pub fn build_router(state: AppState) -> Router {
    let bucket_methods = put(bucket::create_bucket)
        .delete(bucket::delete_bucket)
        .head(bucket::head_bucket)
        .get(bucket::get_bucket)
        .post(bucket::delete_objects_handler)
        .options(options_bucket_handler);

    Router::new()
        .route("/_health", get(health_handler))
        .route(
            "/",
            get(bucket::list_buckets).post(secretsmanager_post_handler),
        )
        .route("/{bucket_name}", bucket_methods.clone())
        .route("/{bucket_name}/", bucket_methods)
        .route(
            "/{bucket_name}/{*key}",
            put(object::put_object_handler)
                .get(object::get_object_handler)
                .head(object::head_object_handler)
                .delete(object::delete_object_handler)
                .post(object::post_object_handler)
                .options(options_object_handler),
        )
        .fallback(fallback)
        .layer(middleware::from_fn(s3_headers_middleware))
        .with_state(state)
}

pub async fn run_server(port: u16, data_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let storage = FileSystemStorage::new(data_dir.clone()).await.map_err(
        |e| -> Box<dyn std::error::Error> { format!("Failed to initialize storage: {e}").into() },
    )?;

    let secrets_storage =
        SecretsStorage::new(data_dir)
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to initialize secrets storage: {e}").into()
            })?;

    let state = AppState {
        storage: Arc::new(storage),
        secrets_storage: Arc::new(secrets_storage),
    };

    let app = build_router(state);

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("local-s3 listening on 0.0.0.0:{port}");

    axum::serve(listener, app).await?;
    Ok(())
}
