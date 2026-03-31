pub mod port;

use std::path::Path;

pub use port::MediaProbe;

use serde::Deserialize;
use thiserror::Error;
use tokio::process::Command;
use url::Url;

use crate::domain::{AudioCodec, ContainerFormat, MediaMetadata, VideoCodec};

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

    /// Execute ffprobe with the given target and optional extension hint, returning parsed media metadata or an error.
    async fn execute_probe(
        &self,
        target: &str,
        ext_hint: Option<&str>,
    ) -> Result<MediaMetadata, FfprobeError> {
        let output = Command::new(&self.command)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                target,
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(FfprobeError::NonZeroExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let parsed = serde_json::from_slice::<FfprobeOutput>(&output.stdout)?.with_hint(ext_hint);
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
    async fn probe_url(&self, url: &Url) -> Result<MediaMetadata, FfprobeError> {
        let ext = Path::new(url.path()).extension().and_then(|e| e.to_str());
        self.execute_probe(url.as_str(), ext).await
    }

    async fn probe_file(&self, path: &std::path::Path) -> Result<MediaMetadata, FfprobeError> {
        let ext = path.extension().and_then(|e| e.to_str());
        self.execute_probe(path.to_str().ok_or(FfprobeError::InvalidPath)?, ext)
            .await
    }
}

/// Top-level ffprobe JSON output.
#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
    #[serde(skip)]
    ext_hint: Option<String>,
}

impl FfprobeOutput {
    /// Extension hint extracted from the file path, used to help infer container format when ffprobe output is ambiguous.
    fn with_hint(mut self, hint: Option<&str>) -> Self {
        self.ext_hint = hint.map(|s| s.to_lowercase());
        self
    }

    /// Resolve the container format from ffprobe's format_name field, which may contain multiple comma-separated aliases.
    fn resolve_container(format_names: &str, ext_hint: Option<&str>) -> ContainerFormat {
        let parts: Vec<&str> = format_names.split(',').map(str::trim).collect();

        // if ffprobe returns a single demuxer name it's already unambiguous and we pass it straight to the Into<ContainerFormat> conversion
        if parts.len() == 1 {
            return parts[0].into();
        }

        // For ambiguous groups (e.g. "matroska,webm" or "mov,mp4,m4a,..."),
        // resolve via file extension which reflects what the user actually uploaded.
        match ext_hint.map(|e| e.trim_start_matches('.')) {
            Some("webm") => ContainerFormat::Webm,
            Some("mkv") => ContainerFormat::Matroska,
            Some("mp4") => ContainerFormat::Mp4,
            Some("mov") => ContainerFormat::Mov,
            Some("avi") => ContainerFormat::Avi,
            _ => parts.first().copied().unwrap_or("").into(), // fallback to first format or unknown if empty
        }
    }
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

/// Conversion from ffprobe output to our normalized media metadata used by the API.
impl From<FfprobeOutput> for MediaMetadata {
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

        // Extract container format from ffprobe output, using the format_name field and falling back to extension hint if needed
        let container_format = output
            .format
            .as_ref()
            .and_then(|f| f.format_name.as_deref())
            .map(|formats| FfprobeOutput::resolve_container(formats, output.ext_hint.as_deref()))
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
    use super::{FfprobeCodecType, FfprobeOutput};
    use crate::domain::{AudioCodec, ContainerFormat, MediaMetadata, VideoCodec};

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
        let metadata = MediaMetadata::from(output);

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
        let metadata = MediaMetadata::from(output);

        assert_eq!(metadata.container_format, Some(ContainerFormat::Mp4));
        assert_eq!(metadata.video_codec, None);
        assert_eq!(metadata.audio_codec, None);
    }

    #[test]
    fn metadata_from_output_handles_multiple_formats() {
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
        let metadata = MediaMetadata::from(output);

        // Should prefer MP4 over WebM if both are listed
        assert_eq!(metadata.container_format, Some(ContainerFormat::Mp4));
    }
}
