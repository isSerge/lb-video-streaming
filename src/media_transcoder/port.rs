use async_trait::async_trait;
use std::path::Path;

use super::TranscoderError;
use crate::domain::ContainerFormat;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MediaTranscoder: Send + Sync {
    /// Transmux a media file (copy codecs, change container) without re-encoding.
    async fn transmux(
        &self,
        input_path: &Path,
        target_container: ContainerFormat,
        output_path: &Path,
    ) -> Result<(), TranscoderError>;
}
