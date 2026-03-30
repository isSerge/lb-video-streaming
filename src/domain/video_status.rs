//! Domain type for video processing status.

use serde::Serialize;
use std::str::FromStr;
use thiserror::Error;

/// API-facing status of a video in the processing pipeline.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoStatus {
    PendingUpload,
    Uploaded,
    Transmuxing,
    Ready,
    Failed,
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
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            _ => Err(VideoStatusError::Invalid(value.to_string())),
        }
    }
}

#[allow(dead_code)]
pub mod typestate {
    use super::VideoStatus;
    use std::marker::PhantomData;

    /// Marker trait for typestate status markers.
    pub trait StatusMarker {
        const STATUS: VideoStatus;
    }

    pub struct PendingUpload;
    pub struct Uploaded;
    pub struct Transmuxing;
    pub struct Ready;
    pub struct Failed;

    impl StatusMarker for PendingUpload {
        const STATUS: VideoStatus = VideoStatus::PendingUpload;
    }

    impl StatusMarker for Uploaded {
        const STATUS: VideoStatus = VideoStatus::Uploaded;
    }

    impl StatusMarker for Transmuxing {
        const STATUS: VideoStatus = VideoStatus::Transmuxing;
    }

    impl StatusMarker for Ready {
        const STATUS: VideoStatus = VideoStatus::Ready;
    }

    impl StatusMarker for Failed {
        const STATUS: VideoStatus = VideoStatus::Failed;
    }

    /// Typestate model for valid video-processing transitions.
    ///
    /// Invalid transitions are unrepresentable because no method exists for them.
    pub struct VideoState<S> {
        _marker: PhantomData<S>,
    }

    impl<S> Default for VideoState<S> {
        fn default() -> Self {
            Self {
                _marker: PhantomData,
            }
        }
    }

    impl<S: StatusMarker> VideoState<S> {
        pub fn status(&self) -> VideoStatus {
            S::STATUS
        }
    }

    impl VideoState<PendingUpload> {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn mark_uploaded(self) -> VideoState<Uploaded> {
            VideoState::default()
        }
    }

    impl VideoState<Uploaded> {
        pub fn start_transmuxing(self) -> VideoState<Transmuxing> {
            VideoState::default()
        }

        pub fn fail(self) -> VideoState<Failed> {
            VideoState::default()
        }
    }

    impl VideoState<Transmuxing> {
        pub fn mark_ready(self) -> VideoState<Ready> {
            VideoState::default()
        }

        pub fn fail(self) -> VideoState<Failed> {
            VideoState::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{typestate::{PendingUpload, VideoState}, VideoStatus, VideoStatusError};

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

    #[test]
    fn typestate_allows_valid_transition_chain() {
        let state = VideoState::<PendingUpload>::new();
        let state = state.mark_uploaded();
        let state = state.start_transmuxing();
        let state = state.mark_ready();

        assert!(matches!(state.status(), VideoStatus::Ready));
    }

    #[test]
    fn typestate_allows_failure_from_processing_states() {
        let failed_from_uploaded = VideoState::<PendingUpload>::new().mark_uploaded().fail();
        let failed_from_transmuxing = VideoState::<PendingUpload>::new()
            .mark_uploaded()
            .start_transmuxing()
            .fail();

        assert!(matches!(failed_from_uploaded.status(), VideoStatus::Failed));
        assert!(matches!(failed_from_transmuxing.status(), VideoStatus::Failed));
    }
}
