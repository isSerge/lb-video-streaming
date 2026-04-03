//! Domain type for raw upload object keys in storage.

use std::ops::Deref;

use ulid::Ulid;

/// Object key for a raw uploaded video in storage.
#[derive(Debug)]
pub struct RawUploadKey(String);

impl From<String> for RawUploadKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for RawUploadKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl RawUploadKey {
    /// Create a raw upload key with a file extension derived from the upload content type.
    /// The extension is embedded in the key so that ffprobe can use it as a disambiguation hint.
    pub fn with_extension(ulid: Ulid, ext: &str) -> Self {
        Self(format!("raw/{}/video.{}", ulid, ext))
    }

    /// Extract the file extension from the stored key, if present.
    pub fn extension(&self) -> Option<&str> {
        std::path::Path::new(&self.0)
            .extension()
            .and_then(|e| e.to_str())
    }
}

impl From<Ulid> for RawUploadKey {
    fn from(ulid: Ulid) -> Self {
        Self(format!("raw/{}/video", ulid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_extension_includes_ext_in_key() {
        let ulid = Ulid::new();
        let key = RawUploadKey::with_extension(ulid, "webm");
        assert_eq!(&*key, format!("raw/{}/video.webm", ulid));
    }

    #[test]
    fn extension_returns_some_when_present() {
        let ulid = Ulid::new();
        let key = RawUploadKey::with_extension(ulid, "mp4");
        assert_eq!(key.extension(), Some("mp4"));
    }

    #[test]
    fn extension_returns_none_for_legacy_key() {
        let ulid = Ulid::new();
        let key = RawUploadKey::from(ulid);
        assert_eq!(key.extension(), None);
    }

    #[test]
    fn from_persisted_string_preserves_extension() {
        let key = RawUploadKey::from("raw/01ABC/video.mkv".to_string());
        assert_eq!(key.extension(), Some("mkv"));
    }
}
