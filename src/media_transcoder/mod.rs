pub mod port;
pub use port::MediaTranscoder;

use std::path::Path;
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
}

/// Wrapper around the `ffmpeg` binary.
#[derive(Default)]
pub struct Ffmpeg {
    command: String,
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
}
