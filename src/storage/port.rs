use async_trait::async_trait;
use url::Url;

use crate::domain::{HLSKey, ManifestKey, RawUploadKey, TransmuxKey, UploadContentType};

use super::R2StorageError;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
/// Storage contract for generating presigned upload URLs.
pub trait Storage: Send + Sync {
    /// Create a presigned PUT URL for uploading a raw object.
    async fn create_upload_url(
        &self,
        key: &RawUploadKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError>;

    /// Returns a presigned GET URL with a configured TTL, used by ffprobe to fetch metadata after upload.
    async fn create_download_url(&self, key: &RawUploadKey) -> Result<Url, R2StorageError>;

    /// Create a presigned PUT URL for uploading a transmuxed object.
    /// This is used by the worker to upload the output of ffmpeg after processing.
    async fn create_transmux_upload_url(
        &self,
        key: &TransmuxKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError>;

    /// Create a presigned GET URL for downloading a transmuxed file.
    async fn create_transmux_download_url(&self, key: &TransmuxKey) -> Result<Url, R2StorageError>;

    /// Create a presigned PUT URL for uploading an HLS manifest.
    async fn create_manifest_upload_url(
        &self,
        key: &ManifestKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError>;

    /// Create a presigned PUT URL for uploading an HLS segment.
    async fn create_hls_segment_upload_url(
        &self,
        key: &HLSKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError>;
}
