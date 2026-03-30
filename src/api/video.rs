//! Video metadata API handlers.

use axum::extract::{Json, Path, State};
use serde::Serialize;
use ulid::Ulid;
use url::Url;

use super::{errors::ApiError, state::AppState};
use crate::domain::VideoStatus;

/// Public API representation of a video.
#[derive(Debug, Serialize)]
pub struct VideoMetadataResponse {
    ulid: Ulid,
    status: VideoStatus,
    browser_compatible: bool,
    transmux_required: bool,
    transcode_required: bool,
    raw_url: Option<Url>,
    transmux_url: Option<Url>,
    manifest_url: Option<Url>,
}

/// Return metadata for a video by ULID.
pub async fn get_video_metadata(
    State(state): State<AppState>,
    Path(ulid): Path<Ulid>,
) -> Result<Json<VideoMetadataResponse>, ApiError> {
    let row = state
        .video_repository
        .find_video_by_ulid(ulid)
        .await?
        .ok_or(ApiError::NotFound)?;

    let status: VideoStatus = row.status.parse()?;
    let raw_url = Some(state.config.public_object_url(&row.raw_key)?);
    let transmux_url = row
        .transmux_key
        .map(|k| state.config.public_object_url(&k))
        .transpose()?;
    let manifest_url = row
        .manifest_key
        .map(|k| state.config.public_object_url(&k))
        .transpose()?;

    Ok(Json(VideoMetadataResponse {
        ulid: row.ulid,
        status,
        browser_compatible: row.browser_compatible,
        transmux_required: row.transmux_required,
        transcode_required: row.transcode_required,
        raw_url,
        transmux_url,
        manifest_url,
    }))
}
