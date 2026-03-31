//! Domain types for normalized media metadata extracted from ffprobe.

use std::convert::Infallible;
use std::str::FromStr;

/// Normalized container format types relevant for compatibility checks and API responses.
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

impl ContainerFormat {
    /// Returns true if this container is a valid output target for transmuxing.
    pub fn is_transmux_target(&self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm)
    }
}

/// Normalized video codec types relevant for compatibility checks and API responses.
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

/// Normalized audio codec types relevant for compatibility checks and API responses.
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

/// Compatibility classification for a media file based on its metadata, used to determine processing requirements for browser playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatCompatibility {
    BrowserCompatible,
    TransmuxRequired,
    TranscodeRequired,
}

/// Normalized media metadata extracted from ffprobe output, used for API responses and compatibility checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaMetadata {
    /// Normalized container format, if identified.
    pub container_format: Option<ContainerFormat>,
    /// Normalized video codec, if identified.
    pub video_codec: Option<VideoCodec>,
    /// Normalized audio codec, if identified.
    pub audio_codec: Option<AudioCodec>,
}

impl FormatCompatibility {
    /// Determine if the media is natively compatible with browser playback without transformation.
    pub const fn browser_compatible(self) -> bool {
        matches!(self, Self::BrowserCompatible)
    }

    /// Determine if the media is a candidate for transmuxing to achieve browser compatibility without transcoding.
    pub const fn transmux_required(self) -> bool {
        matches!(self, Self::TransmuxRequired)
    }

    /// Determine if the media requires transcoding to achieve browser compatibility.
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
    /// Determine if the media is natively compatible with browser playback without transformation.
    fn is_browser_compatible(self) -> bool {
        let is_h264_mp4 = matches!(
            self.container_format,
            Some(ContainerFormat::Mp4 | ContainerFormat::Mov)
        ) && matches!(self.video_codec, Some(VideoCodec::H264))
            && matches!(
                self.audio_codec,
                Some(AudioCodec::Aac | AudioCodec::Mp3) | None
            );

        let is_vp_webm = matches!(self.container_format, Some(ContainerFormat::Webm))
            && matches!(self.video_codec, Some(VideoCodec::Vp8 | VideoCodec::Vp9))
            && matches!(
                self.audio_codec,
                Some(AudioCodec::Opus | AudioCodec::Vorbis) | None
            );

        is_h264_mp4 || is_vp_webm
    }

    /// Determine if the media is a candidate for transmuxing to achieve browser compatibility without transcoding.
    fn is_transmux_candidate(self) -> bool {
        let is_h264_mkv = matches!(
            self.container_format,
            Some(ContainerFormat::Matroska | ContainerFormat::Avi)
        ) && matches!(self.video_codec, Some(VideoCodec::H264))
            && matches!(
                self.audio_codec,
                Some(AudioCodec::Aac | AudioCodec::Mp3) | None
            );

        let is_vp_mkv = matches!(
            self.container_format,
            Some(ContainerFormat::Matroska | ContainerFormat::Avi)
        ) && matches!(self.video_codec, Some(VideoCodec::Vp8 | VideoCodec::Vp9))
            && matches!(
                self.audio_codec,
                Some(AudioCodec::Opus | AudioCodec::Vorbis) | None
            );

        is_h264_mkv || is_vp_mkv
    }

    /// Determine the target container format for transmuxing.
    /// Returns `None` if the media is not a transmux candidate or if the target container cannot be determined.
    pub fn transmux_target_container(&self) -> Option<ContainerFormat> {
        match self.video_codec {
            Some(VideoCodec::H264) => Some(ContainerFormat::Mp4),
            Some(VideoCodec::Vp8 | VideoCodec::Vp9) => Some(ContainerFormat::Webm),
            _ => None,
        }
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
    fn browser_compatible_webm_vp9_opus() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Webm),
            video_codec: Some(VideoCodec::Vp9),
            audio_codec: Some(AudioCodec::Opus),
        });

        assert_eq!(c, FormatCompatibility::BrowserCompatible);
        assert!(c.browser_compatible());
        assert!(!c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn browser_compatible_webm_vp8_vorbis() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Webm),
            video_codec: Some(VideoCodec::Vp8),
            audio_codec: Some(AudioCodec::Vorbis),
        });

        assert_eq!(c, FormatCompatibility::BrowserCompatible);
        assert!(c.browser_compatible());
        assert!(!c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transmux_candidate_avi_h264_mp3() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Avi),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Mp3),
        });

        assert_eq!(c, FormatCompatibility::TransmuxRequired);
        assert!(!c.browser_compatible());
        assert!(c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transmux_candidate_mkv_vp9_vorbis() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::Vp9),
            audio_codec: Some(AudioCodec::Vorbis),
        });

        assert_eq!(c, FormatCompatibility::TransmuxRequired);
        assert!(!c.browser_compatible());
        assert!(c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transmux_candidate_mkv_h264_no_audio() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::H264),
            audio_codec: None,
        });

        assert_eq!(c, FormatCompatibility::TransmuxRequired);
        assert!(!c.browser_compatible());
        assert!(c.transmux_required());
        assert!(!c.transcode_required());
    }

    #[test]
    fn transmux_candidate_avi_h264_no_audio() {
        let c = FormatCompatibility::from(MediaMetadata {
            container_format: Some(ContainerFormat::Avi),
            video_codec: Some(VideoCodec::H264),
            audio_codec: None,
        });

        assert_eq!(c, FormatCompatibility::TransmuxRequired);
        assert!(!c.browser_compatible());
        assert!(c.transmux_required());
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

    #[test]
    fn transmux_target_container_returns_mp4_for_h264() {
        let metadata = MediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Aac),
        };

        assert_eq!(
            metadata.transmux_target_container(),
            Some(ContainerFormat::Mp4)
        );
    }

    #[test]
    fn transmux_target_container_returns_webm_for_vp9() {
        let metadata = MediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::Vp9),
            audio_codec: Some(AudioCodec::Vorbis),
        };

        assert_eq!(
            metadata.transmux_target_container(),
            Some(ContainerFormat::Webm)
        );
    }

    #[test]
    fn transmux_target_container_returns_none_for_non_transmux_candidate() {
        let metadata = MediaMetadata {
            container_format: Some(ContainerFormat::Flv),
            video_codec: Some(VideoCodec::Mpeg4),
            audio_codec: Some(AudioCodec::Mp3),
        };

        assert_eq!(metadata.transmux_target_container(), None);
    }
}
