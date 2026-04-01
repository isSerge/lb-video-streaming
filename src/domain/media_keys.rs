//! Domain key types for persisted media object locations.

use std::ops::Deref;

use ulid::Ulid;

use crate::domain::ContainerFormat;

/// Storage key for transmuxed MP4 output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmuxKey(String);

impl TransmuxKey {
    pub fn new(ulid: Ulid, container: ContainerFormat) -> Self {
        let ext = match container {
            ContainerFormat::Mp4 => "mp4",
            ContainerFormat::Webm => "webm",
            _ => "mp4", // fallback, should not happen
        };
        Self(format!("transmux/{}/output.{}", ulid, ext))
    }
}

/// Storage key for HLS manifest object.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ManifestKey(String);

impl From<String> for TransmuxKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<String> for ManifestKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for TransmuxKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for ManifestKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Storage key for HLS segment object.
#[derive(Debug)]
pub struct HLSKey(String);

impl HLSKey {
    pub fn new(ulid: Ulid, segment_name: &str) -> Self {
        Self(format!("hls/{}/{}", ulid, segment_name))
    }
}

impl Deref for HLSKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
