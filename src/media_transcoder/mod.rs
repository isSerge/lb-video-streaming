pub mod port;
pub use port::MediaTranscoder;

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use thiserror::Error;
use tokio::process::Command;

use crate::domain::ContainerFormat;

/// Errors that can occur during media transcoding operations.
#[derive(Debug, Error)]
pub enum TranscoderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ffmpeg transmux failed: {stderr}")]
    TransmuxFailed { stderr: String },

    #[error("unsupported container for transmux: {0:?}")]
    UnsupportedContainer(ContainerFormat),

    #[error("transcode failed: {stderr}")]
    TranscodeFailed { stderr: String },
}

/// Wrapper around the `ffmpeg` binary.
pub struct Ffmpeg {
    command: String,
}

impl Default for Ffmpeg {
    fn default() -> Self {
        Self::new("ffmpeg")
    }
}

impl Ffmpeg {
    /// Create a new ffmpeg wrapper using a specific binary path/name.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }
}

#[async_trait::async_trait]
impl MediaTranscoder for Ffmpeg {
    async fn transmux(
        &self,
        input_path: &Path,
        target_container: ContainerFormat,
        output_path: &Path,
    ) -> Result<(), TranscoderError> {
        // Validate that the target container is supported for transmuxing before running ffmpeg.
        if !target_container.is_transmux_target() {
            return Err(TranscoderError::UnsupportedContainer(target_container));
        }

        let output = Command::new(&self.command)
            .arg("-i")
            .arg(input_path.as_os_str())
            .arg("-c")
            .arg("copy")
            .arg(output_path.as_os_str())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(TranscoderError::TransmuxFailed { stderr });
        }

        Ok(())
    }

    async fn hls_transcode(
        &self,
        input_path: &Path,
        output_dir: &Path,
        progress_tx: tokio::sync::watch::Sender<()>,
        heartbeat_interval: Duration,
    ) -> Result<PathBuf, TranscoderError> {
        let manifest_path = output_dir.join("manifest.m3u8");

        // TODO: double check args and fine tune if necessary
        let mut cmd = Command::new(&self.command);
        cmd.arg("-i").arg(input_path.as_os_str());

        // Video filtering: scale to 720p (preserve aspect ratio, even width)
        cmd.arg("-vf").arg("scale=-2:720");

        // Video bitrate and codec
        cmd.arg("-b:v").arg("1500k");
        cmd.arg("-c:v").arg("libx264");

        // Audio codec and bitrate
        cmd.arg("-c:a").arg("aac");
        cmd.arg("-b:a").arg("128k");

        // HLS output format
        cmd.arg("-f").arg("hls");
        cmd.arg("-hls_time").arg("6"); // 6 seconds per segment
        cmd.arg("-hls_list_size").arg("0"); // keep all segments
        cmd.arg("-hls_segment_filename")
            .arg(output_dir.join("segment_%03d.ts").as_os_str());
        cmd.arg(&manifest_path);

        let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        // Spawn a task to periodically send progress updates until the transcoding process completes
        let progress_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(heartbeat_interval).await;
                if progress_tx.send(()).is_err() {
                    break; // receiver dropped
                }
            }
        });

        let output = child.wait_with_output().await?;
        drop(progress_handle);

        if !output.status.success() {
            // Capture stderr
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(TranscoderError::TranscodeFailed { stderr });
        }

        Ok(manifest_path)
    }
}
