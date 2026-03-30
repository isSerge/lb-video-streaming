//! Domain types for normalized media metadata extracted from ffprobe.

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

impl From<&str> for ContainerFormat {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "mov" => Self::Mov,
            "mp4" => Self::Mp4,
            "matroska" => Self::Matroska,
            "webm" => Self::Webm,
            "avi" => Self::Avi,
            "mpegts" => Self::MpegTs,
            "flv" => Self::Flv,
            _ => Self::Unknown,
        }
    }
}

impl From<&str> for VideoCodec {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "h264" => Self::H264,
            "h265" | "hevc" => Self::Hevc,
            "av1" => Self::Av1,
            "vp9" => Self::Vp9,
            "vp8" => Self::Vp8,
            "mpeg4" => Self::Mpeg4,
            "mpeg2video" => Self::Mpeg2Video,
            _ => Self::Unknown,
        }
    }
}

impl From<&str> for AudioCodec {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "aac" => Self::Aac,
            "mp3" => Self::Mp3,
            "opus" => Self::Opus,
            "vorbis" => Self::Vorbis,
            "flac" => Self::Flac,
            "ac3" => Self::Ac3,
            "eac3" => Self::Eac3,
            _ => Self::Unknown,
        }
    }
}
