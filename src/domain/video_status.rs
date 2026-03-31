//! Domain type for video processing status.

use serde::Serialize;
use std::str::FromStr;
use thiserror::Error;

/// API-facing status of a video in the processing pipeline.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoStatus {
    PendingUpload,
    Uploaded,
    Transmuxing,
    Transcoding,
    Ready,
    Failed,
}

impl AsRef<str> for VideoStatus {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::PendingUpload => "pending_upload",
            Self::Uploaded => "uploaded",
            Self::Transmuxing => "transmuxing",
            Self::Transcoding => "transcoding",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

/// Validation errors when converting persisted status into domain status.
#[derive(Debug, Error)]
pub enum VideoStatusError {
    #[error("invalid video status in database: {0}")]
    Invalid(String),
}

impl FromStr for VideoStatus {
    type Err = VideoStatusError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending_upload" => Ok(Self::PendingUpload),
            "uploaded" => Ok(Self::Uploaded),
            "transmuxing" => Ok(Self::Transmuxing),
            "transcoding" => Ok(Self::Transcoding),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            _ => Err(VideoStatusError::Invalid(value.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VideoStatus, VideoStatusError};

    #[test]
    fn parses_pending_upload_status() {
        let status: VideoStatus = "pending_upload".parse().unwrap();
        assert!(matches!(status, VideoStatus::PendingUpload));
    }

    #[test]
    fn parses_uploaded_status() {
        let status: VideoStatus = "uploaded".parse().unwrap();
        assert!(matches!(status, VideoStatus::Uploaded));
    }

    #[test]
    fn parses_transmuxing_status() {
        let status: VideoStatus = "transmuxing".parse().unwrap();
        assert!(matches!(status, VideoStatus::Transmuxing));
    }

    #[test]
    fn parses_transcoding_status() {
        let status: VideoStatus = "transcoding".parse().unwrap();
        assert!(matches!(status, VideoStatus::Transcoding));
    }

    #[test]
    fn parses_ready_status() {
        let status: VideoStatus = "ready".parse().unwrap();
        assert!(matches!(status, VideoStatus::Ready));
    }

    #[test]
    fn parses_failed_status() {
        let status: VideoStatus = "failed".parse().unwrap();
        assert!(matches!(status, VideoStatus::Failed));
    }

    #[test]
    fn rejects_unknown_status_and_preserves_value() {
        let value = "processing".to_string();
        let result = value.parse::<VideoStatus>();

        assert!(matches!(result, Err(VideoStatusError::Invalid(v)) if v == value));
    }

    #[test]
    fn serializes_to_snake_case() {
        let encoded = serde_json::to_string(&VideoStatus::PendingUpload).unwrap();
        assert_eq!(encoded, "\"pending_upload\"");

        let encoded = serde_json::to_string(&VideoStatus::Transmuxing).unwrap();
        assert_eq!(encoded, "\"transmuxing\"");
    }
}
