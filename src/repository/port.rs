use async_trait::async_trait;
use ulid::Ulid;

use crate::domain::{FormatCompatibility, RawUploadKey, UploadContentType};

use super::VideoRecord;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
/// Repository contract for creating, updating, and reading video rows.
pub trait VideoRepository: Send + Sync {
    /// Insert a new video row in `pending_upload` state before direct-to-R2 upload starts.
    async fn create_pending_video(
        &self,
        ulid: Ulid,
        raw_key: &RawUploadKey,
        content_type: &UploadContentType,
        size_bytes: i64,
    ) -> Result<(), sqlx::Error>;

    /// Mark an existing video as `uploaded` and persist format compatibility flags.
    ///
    /// Returns `true` when a row was updated and `false` when no video matched the ULID.
    async fn mark_uploaded_with_compatibility(
        &self,
        ulid: Ulid,
        compatibility: FormatCompatibility,
    ) -> Result<bool, sqlx::Error>;

    /// Fetch a video by ULID for API responses.
    ///
    /// Returns `Ok(None)` when the ULID does not exist.
    async fn find_video_by_ulid(&self, ulid: Ulid) -> Result<Option<VideoRecord>, sqlx::Error>;
}
