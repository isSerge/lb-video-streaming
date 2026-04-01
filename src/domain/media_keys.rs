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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ContainerFormat;
    use std::str::FromStr;

    fn test_ulid() -> Ulid {
        Ulid::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()
    }

    #[test]
    fn transmux_key_generates_correct_path_for_mp4() {
        let key = TransmuxKey::new(test_ulid(), ContainerFormat::Mp4);
        assert_eq!(&*key, "transmux/01ARZ3NDEKTSV4RRFFQ69G5FAV/output.mp4");
    }

    #[test]
    fn transmux_key_generates_correct_path_for_webm() {
        let key = TransmuxKey::new(test_ulid(), ContainerFormat::Webm);
        assert_eq!(&*key, "transmux/01ARZ3NDEKTSV4RRFFQ69G5FAV/output.webm");
    }

    #[test]
    fn transmux_key_restores_from_persisted() {
        let path = "transmux/custom/path.mp4".to_string();
        let key = TransmuxKey::from_persisted(path.clone());
        assert_eq!(&*key, path.as_str());
    }

    #[test]
    fn manifest_key_generates_correct_path() {
        let key = ManifestKey::new(test_ulid());
        assert_eq!(&*key, "hls/01ARZ3NDEKTSV4RRFFQ69G5FAV/manifest.m3u8");
    }

    #[test]
    fn manifest_key_restores_from_persisted() {
        let path = "hls/custom/manifest.m3u8".to_string();
        let key = ManifestKey::from_persisted(path.clone());
        assert_eq!(&*key, path.as_str());
    }

    #[test]
    fn hls_key_generates_correct_path() {
        let key = HLSKey::new(test_ulid(), "segment_001.ts");
        assert_eq!(&*key, "hls/01ARZ3NDEKTSV4RRFFQ69G5FAV/segment_001.ts");
    }
}
