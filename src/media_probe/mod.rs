pub mod port;

pub use port::MediaProbe;

use serde::Deserialize;
use thiserror::Error;
use tokio::process::Command;
use url::Url;

use crate::domain::{AudioCodec, ContainerFormat, VideoCodec};

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

    async fn probe_path(&self, path: &str) -> Result<ProbedMediaMetadata, FfprobeError> {
        let output = Command::new(&self.command)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                path,
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(FfprobeError::NonZeroExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)?;
        Ok(parsed.into())
    }
}

impl Default for Ffprobe {
    fn default() -> Self {
        Self::new("ffprobe")
    }
}

#[async_trait::async_trait]
impl MediaProbe for Ffprobe {
    async fn probe_url(&self, url: &Url) -> Result<ProbedMediaMetadata, FfprobeError> {
        self.probe_path(url.as_str()).await
    }

    async fn probe_file(
        &self,
        path: &std::path::Path,
    ) -> Result<ProbedMediaMetadata, FfprobeError> {
        self.probe_path(path.to_str().ok_or(FfprobeError::InvalidPath)?)
            .await
    }
}

/// Top-level ffprobe JSON output.
#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

/// Stream entry from ffprobe output.
#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<FfprobeCodecType>,
    codec_name: Option<String>,
}

/// Normalized codec type values reported by ffprobe.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FfprobeCodecType {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
    #[serde(other)]
    Unknown,
}

/// Container format entry from ffprobe output.
#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    format_name: Option<String>,
    #[serde(rename = "format_long_name")]
    _format_long_name: Option<String>,
}

/// Normalized media metadata extracted from ffprobe output, used by the API.
#[derive(Debug, PartialEq, Eq)]
pub struct ProbedMediaMetadata {
    pub container_format: Option<ContainerFormat>,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
}

impl From<FfprobeOutput> for ProbedMediaMetadata {
    fn from(output: FfprobeOutput) -> Self {
        // Helper to trim and filter out empty codec names
        fn normalized<'a>(value: Option<&'a str>) -> Option<&'a str> {
            value.map(str::trim).filter(|name| !name.is_empty())
        }

        // Extract codec names for video and audio streams, if present
        let codec_for = |codec_type: FfprobeCodecType| {
            output
                .streams
                .iter()
                .find(|s| matches!(s.codec_type.as_ref(), Some(t) if *t == codec_type))
                .and_then(|s| s.codec_name.as_deref())
        };

        // Extract container format, preferring web-friendly formats if multiple are listed
        let container_format = output
            .format
            .as_ref()
            .and_then(|f| f.format_name.as_deref())
            .map(|formats| {
                let parts: Vec<&str> = formats.split(',').collect();
                // Prefer specific web-friendly containers if they are in the list
                if parts.contains(&"webm") {
                    return ContainerFormat::Webm;
                }
                if parts.contains(&"mp4") {
                    return ContainerFormat::Mp4;
                }
                // Otherwise grab the first one
                parts.first().copied().unwrap_or("").trim().into()
            })
            .filter(|f| *f != ContainerFormat::Unknown);

        // Extract and normalize codec names for video and audio streams
        let video_codec = normalized(codec_for(FfprobeCodecType::Video)).map(VideoCodec::from);
        let audio_codec = normalized(codec_for(FfprobeCodecType::Audio)).map(AudioCodec::from);

        Self {
            container_format,
            video_codec,
            audio_codec,
        }
    }
}

/// Errors that can occur during ffprobe execution and parsing.
#[derive(Debug, Error)]
pub enum FfprobeError {
    #[error("failed to spawn ffprobe: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("ffprobe exited with code {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },

    #[error("failed to parse ffprobe JSON output: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid file path for ffprobe: cannot convert to string")]
    InvalidPath,
}

#[cfg(test)]
mod tests {
    use super::{FfprobeCodecType, FfprobeOutput, ProbedMediaMetadata};
    use crate::domain::{AudioCodec, ContainerFormat, VideoCodec};

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

    #[test]
    fn metadata_from_output_handles_missing_fields() {
        let raw = r#"{ "streams": [{ "codec_type": "video" }], "format": null }"#;
        let output: FfprobeOutput = serde_json::from_str(raw).unwrap();
        let metadata = ProbedMediaMetadata::from(output);

        assert_eq!(metadata.container_format, None);
        assert_eq!(metadata.video_codec, None);
        assert_eq!(metadata.audio_codec, None);
    }

    #[test]
    fn metadata_from_output_filters_empty_codec_names() {
        let raw = r#"
        {
            "streams": [
                { "codec_type": "video", "codec_name": "   " },
                { "codec_type": "audio", "codec_name": "" }
            ],
            "format": {
                "format_name": "mp4",
                "format_long_name": "MP4 format"
            }
        }
        "#;

        let output: FfprobeOutput = serde_json::from_str(raw).unwrap();
        let metadata = ProbedMediaMetadata::from(output);

        assert_eq!(metadata.container_format, Some(ContainerFormat::Mp4));
        assert_eq!(metadata.video_codec, None);
        assert_eq!(metadata.audio_codec, None);
    }

    #[test]
    fn metadata_from_output_prefers_web_friendly_containers() {
        let raw = r#"
        {
            "streams": [],
            "format": {
                "format_name": "mp4,webm",
                "format_long_name": "MP4 and WebM formats"
            }
        }
        "#;

        let output: FfprobeOutput = serde_json::from_str(raw).unwrap();
        let metadata = ProbedMediaMetadata::from(output);

        // Should prefer WebM over MP4 if both are listed
        assert_eq!(metadata.container_format, Some(ContainerFormat::Webm));
    }

    #[test]
    fn metadata_from_output_extracts_container_and_codecs_if_web_unfriendly() {
        let raw = r#"
        {
            "streams": [
                { "codec_type": "video", "codec_name": "h264" },
                { "codec_type": "audio", "codec_name": "aac" }
            ],
            "format": {
                "format_name": "mov,3gp,3g2,mj2",
                "format_long_name": "QuickTime / MOV"
            }
        }
        "#; // MP4 and WebM are not listed, so should fall back to MOV as container format

        let output: FfprobeOutput = serde_json::from_str(raw).unwrap();
        let metadata = ProbedMediaMetadata::from(output);

        assert_eq!(metadata.container_format, Some(ContainerFormat::Mov));
        assert_eq!(metadata.video_codec, Some(VideoCodec::H264));
        assert_eq!(metadata.audio_codec, Some(AudioCodec::Aac));
    }
}
