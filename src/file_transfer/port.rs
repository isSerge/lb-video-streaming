use async_trait::async_trait;
use std::path::Path;
use url::Url;

use super::FileTransferError;
use crate::domain::UploadContentType;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
/// Contract for streaming large files between local disk and remote URLs.
pub trait FileTransfer: Send + Sync {
    /// Stream a file from a remote URL directly to the local filesystem.
    async fn download(&self, url: Url, dest: &Path) -> Result<(), FileTransferError>;

    /// Stream a file from the local filesystem to a remote URL.
    async fn upload(
        &self,
        src: &Path,
        url: Url,
        content_type: &UploadContentType,
    ) -> Result<(), FileTransferError>;
}
