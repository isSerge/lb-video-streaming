//! Domain types for API-relative response paths.

use serde::Serialize;
use ulid::Ulid;

/// Relative API path for upload-complete confirmation endpoint.
#[derive(Debug, Serialize)]
pub struct UploadCompletePath(String);

/// Relative API path for video metadata endpoint.
#[derive(Debug, Serialize)]
pub struct VideoMetadataPath(String);

impl From<Ulid> for UploadCompletePath {
    fn from(ulid: Ulid) -> Self {
        Self(format!("/api/upload-complete/{}", ulid))
    }
}

impl From<Ulid> for VideoMetadataPath {
    fn from(ulid: Ulid) -> Self {
        Self(format!("/api/video/{}", ulid))
    }
}
