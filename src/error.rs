use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use quick_xml::se::to_string as to_xml_string;
use serde::Serialize;

use crate::types::headers;

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("The specified bucket does not exist")]
    NoSuchBucket { bucket_name: String },

    #[error("The specified key does not exist")]
    NoSuchKey { key: String },

    #[error("Your previous request to create the named bucket succeeded and you already own it")]
    BucketAlreadyOwnedByYou { bucket_name: String },

    #[error("The requested bucket name is not available")]
    BucketAlreadyExists { bucket_name: String },

    #[error("The bucket you tried to delete is not empty")]
    BucketNotEmpty { bucket_name: String },

    #[error("The specified bucket is not valid")]
    InvalidBucketName { bucket_name: String },

    #[error("The specified upload does not exist")]
    NoSuchUpload { upload_id: String },

    #[error("Invalid part: {message}")]
    InvalidPart { message: String },

    #[error("The requested range is not satisfiable")]
    InvalidRange { key: String },

    #[error("The CORS configuration does not exist")]
    NoSuchCORSConfiguration { bucket_name: String },

    #[error("The bucket policy does not exist")]
    NoSuchBucketPolicy { bucket_name: String },

    #[error("The lifecycle configuration does not exist")]
    NoSuchLifecycleConfiguration { bucket_name: String },

    #[error("{message}")]
    MethodNotAllowed { message: String },

    #[error("Internal server error: {message}")]
    InternalError { message: String },
}

impl S3Error {
    pub fn code(&self) -> &'static str {
        match self {
            S3Error::NoSuchBucket { .. } => "NoSuchBucket",
            S3Error::NoSuchKey { .. } => "NoSuchKey",
            S3Error::BucketAlreadyOwnedByYou { .. } => "BucketAlreadyOwnedByYou",
            S3Error::BucketAlreadyExists { .. } => "BucketAlreadyExists",
            S3Error::BucketNotEmpty { .. } => "BucketNotEmpty",
            S3Error::InvalidBucketName { .. } => "InvalidBucketName",
            S3Error::NoSuchUpload { .. } => "NoSuchUpload",
            S3Error::InvalidPart { .. } => "InvalidPart",
            S3Error::InvalidRange { .. } => "InvalidRange",
            S3Error::NoSuchCORSConfiguration { .. } => "NoSuchCORSConfiguration",
            S3Error::NoSuchBucketPolicy { .. } => "NoSuchBucketPolicy",
            S3Error::NoSuchLifecycleConfiguration { .. } => "NoSuchLifecycleConfiguration",
            S3Error::MethodNotAllowed { .. } => "MethodNotAllowed",
            S3Error::InternalError { .. } => "InternalError",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            S3Error::NoSuchBucket { .. } => StatusCode::NOT_FOUND,
            S3Error::NoSuchKey { .. } => StatusCode::NOT_FOUND,
            S3Error::BucketAlreadyOwnedByYou { .. } => StatusCode::CONFLICT,
            S3Error::BucketAlreadyExists { .. } => StatusCode::CONFLICT,
            S3Error::BucketNotEmpty { .. } => StatusCode::CONFLICT,
            S3Error::InvalidBucketName { .. } => StatusCode::BAD_REQUEST,
            S3Error::NoSuchUpload { .. } => StatusCode::NOT_FOUND,
            S3Error::InvalidPart { .. } => StatusCode::BAD_REQUEST,
            S3Error::InvalidRange { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
            S3Error::NoSuchCORSConfiguration { .. } => StatusCode::NOT_FOUND,
            S3Error::NoSuchBucketPolicy { .. } => StatusCode::NOT_FOUND,
            S3Error::NoSuchLifecycleConfiguration { .. } => StatusCode::NOT_FOUND,
            S3Error::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            S3Error::InternalError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn resource(&self) -> String {
        match self {
            S3Error::NoSuchBucket { bucket_name } => format!("/{bucket_name}"),
            S3Error::NoSuchKey { key } => format!("/{key}"),
            S3Error::BucketAlreadyOwnedByYou { bucket_name } => format!("/{bucket_name}"),
            S3Error::BucketAlreadyExists { bucket_name } => format!("/{bucket_name}"),
            S3Error::BucketNotEmpty { bucket_name } => format!("/{bucket_name}"),
            S3Error::InvalidBucketName { bucket_name } => format!("/{bucket_name}"),
            S3Error::NoSuchUpload { upload_id } => format!("/{upload_id}"),
            S3Error::InvalidPart { .. } => "/".to_string(),
            S3Error::InvalidRange { key } => format!("/{key}"),
            S3Error::NoSuchCORSConfiguration { bucket_name } => format!("/{bucket_name}"),
            S3Error::NoSuchBucketPolicy { bucket_name } => format!("/{bucket_name}"),
            S3Error::NoSuchLifecycleConfiguration { bucket_name } => format!("/{bucket_name}"),
            S3Error::MethodNotAllowed { .. } => "/".to_string(),
            S3Error::InternalError { .. } => "/".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename = "Error")]
struct ErrorResponse {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "Resource")]
    resource: String,
    #[serde(rename = "RequestId")]
    request_id: String,
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let request_id = headers::generate_request_id();
        let error_response = ErrorResponse {
            code: self.code().to_string(),
            message: self.to_string(),
            resource: self.resource(),
            request_id: request_id.clone(),
        };

        let xml_body = match to_xml_string(&error_response) {
            Ok(xml) => format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}"),
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
                    .into_response();
            }
        };

        let mut response = (self.status_code(), xml_body).into_response();
        let headers_mut = response.headers_mut();
        headers_mut.insert("content-type", "application/xml".parse().unwrap());
        for (key, value) in headers::s3_headers(&request_id) {
            headers_mut.insert(key.unwrap(), value);
        }
        response
    }
}
