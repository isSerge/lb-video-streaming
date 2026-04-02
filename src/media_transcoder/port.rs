use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use super::TranscoderError;
use crate::domain::ContainerFormat;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MediaTranscoder: Send + Sync {
    /// Transmux a media file (copy codecs, change container) without re-encoding.
    /// The `timeout` parameter specifies the maximum allowed duration for the transmuxing process, after which it will be forcefully terminated to prevent hanging.
    async fn transmux(
        &self,
        input_path: &Path,
        target_container: ContainerFormat,
        output_path: &Path,
        timeout: Duration,
    ) -> Result<(), TranscoderError>;

    /// Transcode a media file to HLS format with segmented output and manifest generation.
    /// The `timeout` parameter specifies the maximum allowed duration for the transcoding process, after which it will be forcefully terminated to prevent hanging.
    async fn hls_transcode(
        &self,
        input_path: &Path,
        output_dir: &Path,
        timeout: Duration,
    ) -> Result<PathBuf, TranscoderError>;
}
