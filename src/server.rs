use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, put};
use tokio::net::TcpListener;

use crate::middleware::s3_headers_middleware;
use crate::routes::bucket;
use crate::routes::object;
use crate::storage::FileSystemStorage;

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<FileSystemStorage>,
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

pub fn build_router(state: AppState) -> Router {
    let bucket_methods = put(bucket::create_bucket)
        .delete(bucket::delete_bucket)
        .head(bucket::head_bucket)
        .get(bucket::get_bucket)
        .post(bucket::delete_objects_handler);

    Router::new()
        .route("/", get(bucket::list_buckets))
        .route("/{bucket_name}", bucket_methods.clone())
        .route("/{bucket_name}/", bucket_methods)
        .route(
            "/{bucket_name}/{*key}",
            put(object::put_object_handler)
                .get(object::get_object_handler)
                .head(object::head_object_handler)
                .delete(object::delete_object_handler),
        )
        .fallback(fallback)
        .layer(middleware::from_fn(s3_headers_middleware))
        .with_state(state)
}

pub async fn run_server(port: u16, data_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let storage =
        FileSystemStorage::new(data_dir)
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to initialize storage: {e}").into()
            })?;

    let state = AppState {
        storage: Arc::new(storage),
    };

    let app = build_router(state);

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("local-s3 listening on 0.0.0.0:{port}");

    axum::serve(listener, app).await?;
    Ok(())
}
