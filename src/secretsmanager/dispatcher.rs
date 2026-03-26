use axum::body::Bytes;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::error::SmError;
use super::storage::SecretsStorage;
use super::types::{
    CreateSecretRequest, DeleteSecretRequest, DescribeSecretRequest, GetSecretValueRequest,
    ListSecretVersionIdsRequest, ListSecretsRequest, PutSecretValueRequest, RestoreSecretRequest,
    UpdateSecretRequest,
};

/// Dispatch an AWS Secrets Manager operation based on the X-Amz-Target header value.
pub async fn dispatch(storage: &SecretsStorage, operation: &str, body: Bytes) -> Response {
    match operation {
        "CreateSecret" => handle_create_secret(storage, body).await,
        "GetSecretValue" => handle_get_secret_value(storage, body).await,
        "PutSecretValue" => handle_put_secret_value(storage, body).await,
        "DeleteSecret" => handle_delete_secret(storage, body).await,
        "RestoreSecret" => handle_restore_secret(storage, body).await,
        "DescribeSecret" => handle_describe_secret(storage, body).await,
        "ListSecrets" => handle_list_secrets(storage, body).await,
        "UpdateSecret" => handle_update_secret(storage, body).await,
        "ListSecretVersionIds" => handle_list_secret_version_ids(storage, body).await,
        _ => SmError::InvalidParameterException {
            message: format!("Unknown operation: {operation}"),
        }
        .into_response(),
    }
}

async fn handle_create_secret(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: CreateSecretRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.create_secret(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_get_secret_value(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: GetSecretValueRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.get_secret_value(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_put_secret_value(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: PutSecretValueRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.put_secret_value(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_delete_secret(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: DeleteSecretRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.delete_secret(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_restore_secret(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: RestoreSecretRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.restore_secret(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_describe_secret(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: DescribeSecretRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.describe_secret(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_list_secrets(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: ListSecretsRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.list_secrets(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_update_secret(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: UpdateSecretRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.update_secret(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

async fn handle_list_secret_version_ids(storage: &SecretsStorage, body: Bytes) -> Response {
    let req: ListSecretVersionIdsRequest = match parse_json(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    match storage.list_secret_version_ids(req).await {
        Ok(resp) => json_response(StatusCode::OK, &resp),
        Err(e) => e.into_response(),
    }
}

/// Parse a JSON request body, returning an error response on failure.
fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Box<Response>> {
    serde_json::from_slice(body).map_err(|e| {
        Box::new(
            SmError::InvalidRequestException {
                message: format!("Invalid JSON in request body: {e}"),
            }
            .into_response(),
        )
    })
}

/// Build a JSON response with the SM content type.
fn json_response<T: serde::Serialize>(status: StatusCode, body: &T) -> Response {
    match serde_json::to_string(body) {
        Ok(json) => {
            let mut resp = (status, json).into_response();
            resp.headers_mut().insert(
                "content-type",
                HeaderValue::from_static("application/x-amz-json-1.1"),
            );
            resp
        }
        Err(_) => SmError::InternalServiceError {
            message: "Failed to serialize response".to_string(),
        }
        .into_response(),
    }
}
