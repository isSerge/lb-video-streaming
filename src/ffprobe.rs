//! Async ffprobe wrapper for media metadata probing over HTTP URLs.
//! TODO: explain why ffprobe wrapper instead of alternatives

use serde::Deserialize;
use thiserror::Error;
use tokio::process::Command;
use url::Url;

/// Wrapper around the `ffprobe` binary.
#[allow(dead_code)]
pub struct Ffprobe {
    command: String,
}

#[allow(dead_code)]
impl Ffprobe {
    /// Create an ffprobe wrapper using a specific binary path/name.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }

    /// Probe media metadata from a URL and return parsed ffprobe JSON output.
    pub async fn probe_url(&self, url: &Url) -> Result<FfprobeOutput, FfprobeError> {
        let output = Command::new(&self.command)
            .args(Self::args(url))
            .output()
            .await?;

        if !output.status.success() {
            return Err(FfprobeError::NonZeroExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// Build ffprobe command-line arguments for probing a URL with JSON output.
    fn args(url: &Url) -> Vec<String> {
        vec![
            "-v".to_string(),
            "error".to_string(),
            "-print_format".to_string(),
            "json".to_string(),
            "-show_format".to_string(),
            "-show_streams".to_string(),
            url.as_str().to_string(),
        ]
    }
}

impl Default for Ffprobe {
    fn default() -> Self {
        Self::new("ffprobe")
    }
}

/// Top-level ffprobe JSON output.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct FfprobeOutput {
    pub streams: Vec<FfprobeStream>,
    pub format: Option<FfprobeFormat>,
}

/// Stream entry from ffprobe output.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct FfprobeStream {
    pub codec_type: Option<FfprobeCodecType>,
    pub codec_name: Option<String>,
}

/// Normalized codec type values reported by ffprobe.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FfprobeCodecType {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
    #[serde(other)]
    Unknown,
}

/// Container format entry from ffprobe output.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct FfprobeFormat {
    pub format_name: Option<String>,
    pub format_long_name: Option<String>,
}

#[derive(Debug, Error)]
pub enum FfprobeError {
    #[error("failed to spawn ffprobe: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("ffprobe exited with code {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },

    #[error("failed to parse ffprobe JSON output: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::{Ffprobe, FfprobeCodecType, FfprobeOutput};
    use url::Url;

    #[test]
    fn builds_expected_ffprobe_args() {
        let url = Url::parse("https://cdn.example.com/raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video")
            .unwrap();

        let args = Ffprobe::args(&url);
        assert_eq!(
            args,
            vec![
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                "https://cdn.example.com/raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video",
            ]
        );
    }

    #[test]
    fn parses_ffprobe_json_output() {
        let raw = r#"
        {
          "streams": [
            { "codec_type": "video", "codec_name": "h264" },
            { "codec_type": "audio", "codec_name": "aac" }
          ],
          "format": {
            "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
            "format_long_name": "QuickTime / MOV"
          }
        }
        "#;

        let parsed: FfprobeOutput = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.streams.len(), 2);
        assert_eq!(parsed.streams[0].codec_type, Some(FfprobeCodecType::Video));
        assert_eq!(parsed.streams[0].codec_name.as_deref(), Some("h264"));
        assert_eq!(parsed.streams[1].codec_type, Some(FfprobeCodecType::Audio));
        assert_eq!(parsed.streams[1].codec_name.as_deref(), Some("aac"));
        assert_eq!(
            parsed
                .format
                .as_ref()
                .and_then(|f| f.format_name.as_deref()),
            Some("mov,mp4,m4a,3gp,3g2,mj2")
        );
    }

    #[test]
    fn maps_unknown_codec_type_to_unknown_variant() {
        let raw = r#"{ "streams": [{ "codec_type": "telemetry" }], "format": null }"#;
        let parsed: FfprobeOutput = serde_json::from_str(raw).unwrap();

        assert_eq!(
            parsed.streams[0].codec_type,
            Some(FfprobeCodecType::Unknown)
        );
    }
}
