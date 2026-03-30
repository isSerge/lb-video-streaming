//! Domain key types for persisted media object locations.

use std::ops::Deref;

/// Storage key for transmuxed MP4 output.
#[derive(Debug)]
pub struct TransmuxKey(String);

/// Storage key for HLS manifest object.
#[derive(Debug)]
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
