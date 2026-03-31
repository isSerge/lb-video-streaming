use async_trait::async_trait;
use std::num::NonZeroU64;
use ulid::Ulid;

use crate::domain::{
    FormatCompatibility, RawUploadKey, TransmuxKey, UploadContentType, VideoStatus,
};

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

    /// Reset stuck jobs to 'uploaded' and return ALL 'uploaded' jobs for the queue.
    /// This is used to recover jobs that were in a pending state but never completed.
    ///
    /// Returns a list of ULIDs for videos that are ready to be processed.
    async fn recover_pending_jobs(&self) -> Result<Vec<Ulid>, sqlx::Error>;

    /// Sweep and fail jobs stuck in processing for over the specified timeout.
    ///
    /// Returns the number of jobs that were marked as failed.
    async fn mark_zombie_jobs_failed(&self, timeout: NonZeroU64) -> Result<u64, sqlx::Error>;

    /// Delete rows that have been in `pending_upload` state for longer than the specified duration.
    ///
    /// Returns the number of deleted rows.
    async fn delete_stale_pending_uploads(
        &self,
        older_than: std::time::Duration,
    ) -> Result<u64, sqlx::Error>;

    /// Update the processing status of a video job, used by the worker to mark progress.
    async fn update_status(&self, ulid: Ulid, status: VideoStatus) -> Result<(), sqlx::Error>;

    /// Set the R2 key for a successfully processed video, used by the worker after uploading the output.
    async fn set_transmux_key(&self, ulid: Ulid, key: &TransmuxKey) -> Result<(), sqlx::Error>;
}
