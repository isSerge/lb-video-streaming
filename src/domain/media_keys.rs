//! Domain key types for persisted media object locations.

use std::ops::Deref;

use ulid::Ulid;

use crate::domain::ContainerFormat;

/// Storage key for transmuxed MP4 output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmuxKey(String);

impl TransmuxKey {
    /// Create a new, strongly-typed key for a newly transmuxed file.
    pub fn new(ulid: Ulid, container: ContainerFormat) -> Self {
        Self(format!(
            "transmux/{}/output.{}",
            ulid,
            container.extension()
        ))
    }

    /// Reconstruct a key from a string previously persisted in the database.
    pub fn from_persisted(key: String) -> Self {
        Self(key)
    }
}

/// Storage key for HLS manifest object.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ManifestKey(String);

impl ManifestKey {
    /// Create a new, strongly-typed key for a newly generated HLS manifest.
    pub fn new(ulid: Ulid) -> Self {
        Self(format!("hls/{}/manifest.m3u8", ulid))
    }

    /// Reconstruct a key from a string previously persisted in the database.
    pub fn from_persisted(key: String) -> Self {
        Self(key)
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
