use async_trait::async_trait;
use std::path::{Path, PathBuf};

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

    /// Transcode a media file to HLS format with segmented output and manifest generation.
    async fn hls_transcode(
        &self,
        input_path: &Path,
        output_dir: &Path,
        // progress_tx: Option<tokio::sync::watch::Sender<()>>, // TODO: add progress reporting channel for worker to update job status in real time
    ) -> Result<PathBuf, TranscoderError>;
}
