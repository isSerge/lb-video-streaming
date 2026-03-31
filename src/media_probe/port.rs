use async_trait::async_trait;
use url::Url;

use super::FfprobeError;
use crate::domain::MediaMetadata;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
/// Media probing contract for extracting normalized metadata from media URLs.
pub trait MediaProbe: Send + Sync {
    /// Probe media metadata from a URL and return normalized fields used by the API.
    async fn probe_url(&self, url: &Url) -> Result<MediaMetadata, FfprobeError>;

    /// Probe media metadata from a local file path, used by the worker when processing downloaded videos.
    async fn probe_file(&self, path: &std::path::Path) -> Result<MediaMetadata, FfprobeError>;
}
