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

#[async_trait::async_trait]
impl MediaProbe for Ffprobe {
    async fn probe_url(&self, url: &Url) -> Result<ProbedMediaMetadata, FfprobeError> {
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

        let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)?;
        Ok(parsed.into())
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

#[derive(Debug, PartialEq, Eq)]
pub struct ProbedMediaMetadata {
    pub container_format: Option<ContainerFormat>,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
}

impl From<FfprobeOutput> for ProbedMediaMetadata {
    fn from(output: FfprobeOutput) -> Self {
        fn normalized<'a>(value: Option<&'a str>) -> Option<&'a str> {
            value.map(str::trim).filter(|name| !name.is_empty())
        }

        let codec_for = |codec_type: FfprobeCodecType| {
            output
                .streams
                .iter()
                .find(|s| matches!(s.codec_type.as_ref(), Some(t) if *t == codec_type))
                .and_then(|s| s.codec_name.as_deref())
        };

        let container_format = normalized(
            output
                .format
                .as_ref()
                .and_then(|f| f.format_name.as_deref())
                .and_then(|formats| formats.split(',').next()),
        )
        .map(ContainerFormat::from);

        let video_codec = normalized(codec_for(FfprobeCodecType::Video)).map(VideoCodec::from);
        let audio_codec = normalized(codec_for(FfprobeCodecType::Audio)).map(AudioCodec::from);

        Self {
            container_format,
            video_codec,
            audio_codec,
        }
    }
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
    use super::{Ffprobe, FfprobeCodecType, FfprobeOutput, ProbedMediaMetadata};
    use crate::domain::{AudioCodec, ContainerFormat, VideoCodec};
    use url::Url;

    #[test]
    fn builds_expected_ffprobe_args() {
        let url =
            Url::parse("https://cdn.example.com/raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video").unwrap();

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

    #[test]
    fn metadata_from_output_extracts_container_and_codecs() {
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

        let output: FfprobeOutput = serde_json::from_str(raw).unwrap();
        let metadata = ProbedMediaMetadata::from(output);

        assert_eq!(metadata.container_format, Some(ContainerFormat::Mov));
        assert_eq!(metadata.video_codec, Some(VideoCodec::H264));
        assert_eq!(metadata.audio_codec, Some(AudioCodec::Aac));
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
}
