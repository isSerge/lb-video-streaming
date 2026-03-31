//! Upload-related API handlers.

use std::num::NonZeroU64;

use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use ulid::Ulid;
use url::Url;

use super::{errors::ApiError, state::AppState};
use crate::domain::{
    FormatCompatibility, MaxUploadBytes, MediaMetadata, RawUploadKey, UploadCompletePath,
    UploadContentType, UploadSizeBytes, VideoMetadataPath,
};

/// Input payload for requesting a presigned upload URL.
#[derive(Debug, Deserialize)]
pub struct UploadUrlRequest {
    content_type: Option<UploadContentType>,
    size_bytes: Option<UploadSizeBytes>,
}

/// Response payload for a newly created upload session.
#[derive(Debug, Serialize)]
pub struct UploadUrlResponse {
    ulid: Ulid,
    upload_url: Url,
    upload_complete_url: UploadCompletePath,
    video_url: VideoMetadataPath,
    expires_in_secs: NonZeroU64,
}

/// Create an upload session and return a short-lived presigned PUT URL.
pub async fn create_upload_url(
    State(state): State<AppState>,
    Json(req): Json<UploadUrlRequest>,
) -> Result<Json<UploadUrlResponse>, ApiError> {
    let content_type = req.content_type.unwrap_or_default();
    let size_bytes = UploadSizeBytes::try_from((
        req.size_bytes.unwrap_or_default(),
        MaxUploadBytes::from(state.config.as_ref()),
    ))?;

    let ulid = Ulid::new();
    let raw_key = RawUploadKey::from(ulid);

    state
        .video_repository
        .create_pending_video(ulid, &raw_key, &content_type, i64::from(size_bytes))
        .await?;

    let upload_url = state
        .storage
        .create_upload_url(&raw_key, &content_type)
        .await?;

    Ok(Json(UploadUrlResponse {
        ulid,
        upload_url,
        upload_complete_url: UploadCompletePath::from(ulid),
        video_url: VideoMetadataPath::from(ulid),
        expires_in_secs: state.config.presigned_upload_ttl_secs,
    }))
}

/// Mark a previously created upload session as uploaded.
pub async fn mark_upload_complete(
    State(state): State<AppState>,
    axum::extract::Path(ulid): axum::extract::Path<Ulid>,
) -> Result<StatusCode, ApiError> {
    let row = state
        .video_repository
        .find_video_by_ulid(ulid)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Generate a presigned download URL for ffprobe to fetch the uploaded video and extract metadata.
    let probe_url = state
        .storage
        .create_download_url(&row.raw_key)
        .await?;
    let metadata = state.media_probe.probe_url(&probe_url).await?;

    tracing::info!(
        %ulid,
        probe_url = %probe_url,
        container_format = ?metadata.container_format,
        video_codec = ?metadata.video_codec,
        audio_codec = ?metadata.audio_codec,
        "ffprobe metadata extracted"
    );

    let compatibility = FormatCompatibility::from(MediaMetadata {
        container_format: metadata.container_format,
        video_codec: metadata.video_codec,
        audio_codec: metadata.audio_codec,
    });

    let found = state
        .video_repository
        .mark_uploaded_with_compatibility(ulid, compatibility)
        .await?;

    if !found {
        return Err(ApiError::NotFound);
    }

    // Push Ulid to the worker queue for processing
    // TODO: double check if this is reliable enough
    if let Err(e) = state.worker_tx.send(ulid).await {
        tracing::error!(%ulid, error = %e, "failed to queue transcode job");
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::UploadUrlRequest;

    #[test]
    fn request_deserialize_accepts_valid_typed_fields() {
        let req: UploadUrlRequest =
            serde_json::from_str(r#"{"content_type":"video/mp4","size_bytes":123}"#).unwrap();

        assert_eq!(&*req.content_type.unwrap(), "video/mp4");
        assert_eq!(i64::from(req.size_bytes.unwrap()), 123);
    }

    #[test]
    fn request_deserialize_allows_missing_optional_fields() {
        let req: UploadUrlRequest = serde_json::from_str("{}").unwrap();
        assert!(req.content_type.is_none());
        assert!(req.size_bytes.is_none());
    }

    #[test]
    fn request_deserialize_rejects_invalid_content_type() {
        let error = serde_json::from_str::<UploadUrlRequest>(
            r#"{"content_type":"not-a-mime","size_bytes":123}"#,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("content_type must be a valid MIME type")
        );
    }

    #[test]
    fn request_deserialize_rejects_negative_size() {
        let error = serde_json::from_str::<UploadUrlRequest>(
            r#"{"content_type":"video/mp4","size_bytes":-1}"#,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("size_bytes must be greater than or equal to 0")
        );
    }
}
