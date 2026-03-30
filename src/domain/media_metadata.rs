//! Domain types for normalized media metadata extracted from ffprobe.

use std::convert::Infallible;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContainerFormat {
    Mov,
    Mp4,
    Matroska,
    Webm,
    Avi,
    MpegTs,
    Flv,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Vp8,
    Mpeg4,
    Mpeg2Video,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Ac3,
    Eac3,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatCompatibility {
    BrowserCompatible,
    TransmuxRequired,
    TranscodeRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaMetadata {
    pub container_format: Option<ContainerFormat>,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
}

impl FormatCompatibility {
    pub const fn browser_compatible(self) -> bool {
        matches!(self, Self::BrowserCompatible)
    }

    pub const fn transmux_required(self) -> bool {
        matches!(self, Self::TransmuxRequired)
    }

    pub const fn transcode_required(self) -> bool {
        matches!(self, Self::TranscodeRequired)
    }
}

impl From<MediaMetadata> for FormatCompatibility {
    fn from(media: MediaMetadata) -> Self {
        if media.is_browser_compatible() {
            return Self::BrowserCompatible;
        }

        if media.is_transmux_candidate() {
            return Self::TransmuxRequired;
        }

        Self::TranscodeRequired
    }
}

impl MediaMetadata {
    fn is_browser_compatible(self) -> bool {
        matches!(self.container_format, Some(ContainerFormat::Mp4 | ContainerFormat::Mov))
            && matches!(self.video_codec, Some(VideoCodec::H264))
            && self.has_supported_audio()
    }

    fn is_transmux_candidate(self) -> bool {
        matches!(self.container_format, Some(ContainerFormat::Matroska | ContainerFormat::Avi))
            && matches!(self.video_codec, Some(VideoCodec::H264))
            && self.has_supported_audio()
    }

    fn has_supported_audio(self) -> bool {
        matches!(self.audio_codec, Some(AudioCodec::Aac | AudioCodec::Mp3) | None)
    }
}

impl FromStr for ContainerFormat {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.to_ascii_lowercase().as_str() {
            "mov" => Self::Mov,
            "mp4" => Self::Mp4,
            "matroska" => Self::Matroska,
            "webm" => Self::Webm,
            "avi" => Self::Avi,
            "mpegts" => Self::MpegTs,
            "flv" => Self::Flv,
            _ => Self::Unknown,
        })
    }
}

impl From<&str> for ContainerFormat {
    fn from(value: &str) -> Self {
        value.parse::<Self>().expect("infallible parse")
    }
}

impl FromStr for VideoCodec {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.to_ascii_lowercase().as_str() {
            "h264" => Self::H264,
            "h265" | "hevc" => Self::Hevc,
            "av1" => Self::Av1,
            "vp9" => Self::Vp9,
            "vp8" => Self::Vp8,
            "mpeg4" => Self::Mpeg4,
            "mpeg2video" => Self::Mpeg2Video,
            _ => Self::Unknown,
        })
    }
}

impl From<&str> for VideoCodec {
    fn from(value: &str) -> Self {
        value.parse::<Self>().expect("infallible parse")
    }
}

impl FromStr for AudioCodec {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.to_ascii_lowercase().as_str() {
            "aac" => Self::Aac,
            "mp3" => Self::Mp3,
            "opus" => Self::Opus,
            "vorbis" => Self::Vorbis,
            "flac" => Self::Flac,
            "ac3" => Self::Ac3,
            "eac3" => Self::Eac3,
            _ => Self::Unknown,
        })
    }
}

impl From<&str> for AudioCodec {
    fn from(value: &str) -> Self {
        value.parse::<Self>().expect("infallible parse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_compatible_mp4_h264_aac() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Mp4),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Aac),
        });

        assert_eq!(c, FormatCompatibility::BrowserCompatible);
        assert!(c.browser_compatible());
        assert!(!c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transmux_candidate_mkv_h264_aac() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Aac),
        });

        assert_eq!(c, FormatCompatibility::TransmuxRequired);
        assert!(!c.browser_compatible());
        assert!(c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transcode_required_for_non_supported_codec() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Mp4),
            video_codec: Some(VideoCodec::Hevc),
            audio_codec: Some(AudioCodec::Aac),
        });

        assert_eq!(c, FormatCompatibility::TranscodeRequired);
        assert!(!c.browser_compatible());
        assert!(!c.transmux_required());
        assert!(c.transcode_required());
    }
}
