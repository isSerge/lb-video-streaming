//! Domain types for upload content type validation.

use std::ops::Deref;
use std::str::FromStr;

use mime::Mime;
use serde::Deserialize;
use thiserror::Error;

/// Strongly typed content type value for uploads.
#[derive(Debug, Deserialize, Clone)]
#[serde(try_from = "String")]
pub struct UploadContentType(Mime);

/// Validation errors when parsing upload content type input.
#[derive(Debug, Error)]
pub enum UploadContentTypeError {
    #[error("content_type must be a valid MIME type")]
    Invalid,
}

impl Deref for UploadContentType {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl Default for UploadContentType {
    fn default() -> Self {
        Self(
            Mime::from_str("application/octet-stream")
                .expect("default content type must be valid MIME"),
        )
    }
}

impl FromStr for UploadContentType {
    type Err = UploadContentTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(
            Mime::from_str(value.trim()).map_err(|_| UploadContentTypeError::Invalid)?,
        ))
    }
}

impl TryFrom<Option<String>> for UploadContentType {
    type Error = UploadContentTypeError;

    fn try_from(value: Option<String>) -> Result<Self, Self::Error> {
        match value {
            Some(raw) => Self::from_str(&raw),
            None => Ok(Self::default()),
        }
    }
}

impl TryFrom<String> for UploadContentType {
    type Error = UploadContentTypeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::{UploadContentType, UploadContentTypeError};

    #[test]
    fn default_is_application_octet_stream() {
        let content_type = UploadContentType::default();
        assert_eq!(&*content_type, "application/octet-stream");
    }

    #[test]
    fn from_str_accepts_valid_mime_type() {
        let content_type: UploadContentType = "video/mp4".parse().unwrap();
        assert_eq!(&*content_type, "video/mp4");
    }

    #[test]
    fn from_str_rejects_invalid_mime_type() {
        let result: Result<UploadContentType, UploadContentTypeError> = "not-a-mime".parse();
        assert!(matches!(result, Err(UploadContentTypeError::Invalid)));
    }

    #[test]
    fn try_from_option_none_uses_default() {
        let content_type = UploadContentType::try_from(None).unwrap();
        assert_eq!(&*content_type, "application/octet-stream");
    }

    #[test]
    fn try_from_option_some_parses_value() {
        let content_type = UploadContentType::try_from(Some("video/webm".to_string())).unwrap();
        assert_eq!(&*content_type, "video/webm");
    }

    #[test]
    fn deserializes_valid_mime_type() {
        let parsed: UploadContentType = serde_json::from_str("\"video/mp4\"").unwrap();
        assert_eq!(&*parsed, "video/mp4");
    }

    #[test]
    fn trims_whitespace_during_deserialize() {
        let parsed: UploadContentType = serde_json::from_str("\"  video/webm  \"").unwrap();
        assert_eq!(&*parsed, "video/webm");
    }

    #[test]
    fn rejects_invalid_mime_type_during_deserialize() {
        let error = serde_json::from_str::<UploadContentType>("\"not-a-mime\"").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("content_type must be a valid MIME type")
        );
    }
}
