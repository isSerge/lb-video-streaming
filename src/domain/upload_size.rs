//! Domain types for upload size validation.

use crate::config::Config;
use serde::Deserialize;
use thiserror::Error;

/// Strongly typed upload size value in bytes.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "i64")]
pub struct UploadSizeBytes(u64);

/// Maximum allowed upload size in bytes.
#[derive(Debug, Clone, Copy)]
pub struct MaxUploadBytes(u64);

/// Validation errors when parsing upload size input.
#[derive(Debug, Error)]
pub enum UploadSizeError {
    #[error("size_bytes must be greater than or equal to 0")]
    Negative,

    #[error("size_bytes exceeds configured upload limit")]
    ExceedsLimit,
}

impl Default for UploadSizeBytes {
    fn default() -> Self {
        Self(0)
    }
}

impl TryFrom<Option<i64>> for UploadSizeBytes {
    type Error = UploadSizeError;

    fn try_from(raw_size_bytes: Option<i64>) -> Result<Self, Self::Error> {
        match raw_size_bytes {
            Some(raw) => Self::try_from(raw),
            None => Ok(Self::default()),
        }
    }
}

impl TryFrom<i64> for UploadSizeBytes {
    type Error = UploadSizeError;

    fn try_from(raw_size_bytes: i64) -> Result<Self, Self::Error> {
        let value = u64::try_from(raw_size_bytes).map_err(|_| UploadSizeError::Negative)?;
        Ok(Self(value))
    }
}

impl TryFrom<(Option<i64>, MaxUploadBytes)> for UploadSizeBytes {
    type Error = UploadSizeError;

    fn try_from(value: (Option<i64>, MaxUploadBytes)) -> Result<Self, Self::Error> {
        let (raw_size_bytes, max_upload_bytes) = value;
        let size_bytes = Self::try_from(raw_size_bytes)?;

        if size_bytes.0 > max_upload_bytes.0 {
            return Err(UploadSizeError::ExceedsLimit);
        }

        Ok(size_bytes)
    }
}

impl TryFrom<(UploadSizeBytes, MaxUploadBytes)> for UploadSizeBytes {
    type Error = UploadSizeError;

    fn try_from(value: (UploadSizeBytes, MaxUploadBytes)) -> Result<Self, Self::Error> {
        let (size_bytes, max_upload_bytes) = value;

        if size_bytes.0 > max_upload_bytes.0 {
            return Err(UploadSizeError::ExceedsLimit);
        }

        Ok(size_bytes)
    }
}

impl From<&Config> for MaxUploadBytes {
    fn from(config: &Config) -> Self {
        Self(config.max_upload_bytes.get())
    }
}

impl From<UploadSizeBytes> for i64 {
    fn from(size_bytes: UploadSizeBytes) -> Self {
        size_bytes.0 as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{MaxUploadBytes, UploadSizeBytes, UploadSizeError};

    #[test]
    fn default_is_zero() {
        assert_eq!(i64::from(UploadSizeBytes::default()), 0);
    }

    #[test]
    fn try_from_option_none_defaults_to_zero() {
        let parsed = UploadSizeBytes::try_from(None).unwrap();
        assert_eq!(i64::from(parsed), 0);
    }

    #[test]
    fn try_from_option_some_accepts_positive_value() {
        let parsed = UploadSizeBytes::try_from(Some(42)).unwrap();
        assert_eq!(i64::from(parsed), 42);
    }

    #[test]
    fn rejects_negative_values() {
        assert!(matches!(
            UploadSizeBytes::try_from((Some(-1), MaxUploadBytes(100))),
            Err(UploadSizeError::Negative)
        ));
    }

    #[test]
    fn rejects_values_over_limit() {
        assert!(matches!(
            UploadSizeBytes::try_from((Some(101), MaxUploadBytes(100))),
            Err(UploadSizeError::ExceedsLimit)
        ));
    }

    #[test]
    fn accepts_values_at_limit() {
        assert!(UploadSizeBytes::try_from((Some(100), MaxUploadBytes(100))).is_ok());
    }

    #[test]
    fn try_from_size_and_limit_accepts_when_within_limit() {
        let size = UploadSizeBytes::try_from(Some(100)).unwrap();
        let validated = UploadSizeBytes::try_from((size, MaxUploadBytes(100))).unwrap();
        assert_eq!(i64::from(validated), 100);
    }

    #[test]
    fn try_from_size_and_limit_rejects_when_over_limit() {
        let size = UploadSizeBytes::try_from(Some(101)).unwrap();
        let result = UploadSizeBytes::try_from((size, MaxUploadBytes(100)));
        assert!(matches!(result, Err(UploadSizeError::ExceedsLimit)));
    }

    #[test]
    fn deserializes_positive_integer() {
        let parsed: UploadSizeBytes = serde_json::from_str("123").unwrap();
        assert_eq!(i64::from(parsed), 123);
    }

    #[test]
    fn deserializes_zero_integer() {
        let parsed: UploadSizeBytes = serde_json::from_str("0").unwrap();
        assert_eq!(i64::from(parsed), 0);
    }

    #[test]
    fn rejects_negative_integer_during_deserialize() {
        let error = serde_json::from_str::<UploadSizeBytes>("-1").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("size_bytes must be greater than or equal to 0")
        );
    }

    #[test]
    fn rejects_non_integer_during_deserialize() {
        let error = serde_json::from_str::<UploadSizeBytes>("\"abc\"").unwrap_err();
        assert!(error.to_string().contains("invalid type"));
    }

    #[test]
    fn supports_i64_max_value() {
        let parsed = UploadSizeBytes::try_from(Some(i64::MAX)).unwrap();
        assert_eq!(i64::from(parsed), i64::MAX);
    }
}
