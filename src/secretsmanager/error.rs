use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SmError {
    #[error("{message}")]
    ResourceNotFoundException { message: String },
    #[error("{message}")]
    ResourceExistsException { message: String },
    #[error("{message}")]
    InvalidParameterException { message: String },
    #[error("{message}")]
    InvalidRequestException { message: String },
    #[error("{message}")]
    InternalServiceError { message: String },
}

impl SmError {
    fn error_type(&self) -> &'static str {
        match self {
            SmError::ResourceNotFoundException { .. } => "ResourceNotFoundException",
            SmError::ResourceExistsException { .. } => "ResourceExistsException",
            SmError::InvalidParameterException { .. } => "InvalidParameterException",
            SmError::InvalidRequestException { .. } => "InvalidRequestException",
            SmError::InternalServiceError { .. } => "InternalServiceError",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            SmError::InternalServiceError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

#[derive(Debug, Serialize)]
struct SmErrorResponse {
    #[serde(rename = "__type")]
    error_type: String,
    #[serde(rename = "Message")]
    message: String,
}

impl IntoResponse for SmError {
    fn into_response(self) -> Response {
        let body = SmErrorResponse {
            error_type: self.error_type().to_string(),
            message: self.to_string(),
        };

        let json = match serde_json::to_string(&body) {
            Ok(j) => j,
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
                    .into_response();
            }
        };

        let mut response = (self.status_code(), json).into_response();
        response.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("application/x-amz-json-1.1"),
        );
        response
    }
}
