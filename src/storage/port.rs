use async_trait::async_trait;
use url::Url;

use crate::domain::{RawUploadKey, UploadContentType};

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

    /// Returns a presigned GET URL with a specified TTL, used by ffprobe to fetch metadata after upload.
    async fn create_download_url(
        &self,
        key: &RawUploadKey,
        ttl_secs: u64,
    ) -> Result<Url, R2StorageError>;
}
