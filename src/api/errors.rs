//! API error definitions and HTTP response mapping.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use thiserror::Error;
use url::ParseError;

use crate::{
    domain::{UploadContentTypeError, UploadSizeError, VideoStatusError},
    ffprobe::FfprobeError,
    r2_storage::R2StorageError,
};

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Error)]
/// Canonical API error type for HTTP handlers.
pub enum ApiError {
    #[error(transparent)]
    UploadSize(#[from] UploadSizeError),

    #[error(transparent)]
    UploadContentType(#[from] UploadContentTypeError),

    #[error("not found")]
    NotFound,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    VideoStatus(#[from] VideoStatusError),

    #[error(transparent)]
    UrlParse(#[from] ParseError),

    #[error(transparent)]
    R2Storage(#[from] R2StorageError),

    #[error(transparent)]
    Ffprobe(#[from] FfprobeError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            Self::UploadSize(_) | Self::UploadContentType(_) => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Database(_)
            | Self::VideoStatus(_)
            | Self::UrlParse(_)
            | Self::R2Storage(_)
            | Self::Ffprobe(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        let body = Json(ErrorResponse {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}
