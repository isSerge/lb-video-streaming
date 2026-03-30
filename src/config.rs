use serde::Deserialize;
use std::net::IpAddr;
use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use thiserror::Error;
use url::Url;

/// All runtime configuration for the application.
/// Mandatory fields have no default and will return an error at startup if absent.
/// Optional fields fall back to the constants in the `defaults` module.
/// Actual environment variables always take precedence over `.env` file values
/// because `dotenvy::dotenv()` never overwrites variables that are already set.
#[derive(Debug, Deserialize)]
pub struct Config {
    // --- mandatory ---
    pub database_url: String,
    pub r2_account_id: String,
    pub r2_access_key_id: String,
    pub r2_secret_access_key: String,
    pub r2_bucket_name: String,
    pub r2_account_token: String,
    pub public_cdn_domain: String,

    // --- optional (env overrides default) ---
    #[serde(default = "defaults::max_concurrent_transcodes")]
    pub max_concurrent_transcodes: NonZeroUsize,

    #[serde(default = "defaults::server_host")]
    pub server_host: IpAddr,

    #[serde(default = "defaults::server_port")]
    pub server_port: NonZeroU16,

    #[serde(default = "defaults::log_level")]
    pub log_level: LogLevel,

    #[serde(default = "defaults::max_upload_bytes")]
    pub max_upload_bytes: NonZeroU64,

    #[serde(default = "defaults::presigned_upload_ttl_secs")]
    pub presigned_upload_ttl_secs: NonZeroU64,
}

impl Config {
    /// Load configuration from the process environment, loading `.env` first
    /// (existing env vars win). Returns an error on any missing mandatory
    /// field or invalid value.
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load .env but never overwrite vars already set in the environment.
        let _ = dotenvy::dotenv();
        Self::from_iter(std::env::vars())
    }

    /// Deserialise configuration from an arbitrary key-value iterator.
    /// Used by `from_env` and unit tests.
    fn from_iter<I>(vars: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        Ok(envy::from_iter(vars)?)
    }

    /// Build a public CDN URL for an object key.
    pub fn public_object_url(&self, key: &str) -> Result<Url, url::ParseError> {
        Url::parse(&format!(
            "{}/{}",
            self.public_cdn_domain.trim_end_matches('/'),
            key
        ))
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration error: {0}")]
    Env(#[from] envy::Error),
}

mod defaults {
    use super::{LogLevel, NonZeroU16, NonZeroU64, NonZeroUsize};
    use std::net::{IpAddr, Ipv4Addr};

    pub fn max_concurrent_transcodes() -> NonZeroUsize {
        NonZeroUsize::new(1).expect("default max_concurrent_transcodes must be non-zero")
    }

    pub fn server_host() -> IpAddr {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    }

    pub fn server_port() -> NonZeroU16 {
        NonZeroU16::new(3000).expect("default server_port must be non-zero")
    }

    pub fn log_level() -> LogLevel {
        LogLevel::Info
    }

    pub fn max_upload_bytes() -> NonZeroU64 {
        NonZeroU64::new(1_073_741_824).expect("default max_upload_bytes must be non-zero")
    }

    pub fn presigned_upload_ttl_secs() -> NonZeroU64 {
        NonZeroU64::new(900).expect("default presigned_upload_ttl_secs must be non-zero")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a complete set of mandatory vars into a vec so individual tests
    /// can push overrides or remove entries without repeating boilerplate.
    fn mandatory_vars() -> Vec<(String, String)> {
        vec![
            ("DATABASE_URL".into(), "postgres://localhost/test".into()),
            ("R2_ACCOUNT_ID".into(), "acct123".into()),
            ("R2_ACCESS_KEY_ID".into(), "key123".into()),
            ("R2_SECRET_ACCESS_KEY".into(), "secret123".into()),
            ("R2_BUCKET_NAME".into(), "my-bucket".into()),
            ("R2_ACCOUNT_TOKEN".into(), "token123".into()),
            ("PUBLIC_CDN_DOMAIN".into(), "https://cdn.example.com".into()),
        ]
    }

    #[test]
    fn loads_valid_config() {
        let cfg = Config::from_iter(mandatory_vars()).unwrap();
        assert_eq!(cfg.database_url, "postgres://localhost/test");
        assert_eq!(cfg.r2_bucket_name, "my-bucket");
        assert_eq!(cfg.public_cdn_domain, "https://cdn.example.com");
    }

    #[test]
    fn default_max_concurrent_transcodes_is_one() {
        let cfg = Config::from_iter(mandatory_vars()).unwrap();
        assert_eq!(cfg.max_concurrent_transcodes.get(), 1);
    }

    #[test]
    fn defaults_server_host_port_and_log_level() {
        let cfg = Config::from_iter(mandatory_vars()).unwrap();
        assert_eq!(cfg.server_host, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(cfg.server_port.get(), 3000);
        assert!(matches!(cfg.log_level, LogLevel::Info));
        assert_eq!(cfg.max_upload_bytes.get(), 1_073_741_824);
        assert_eq!(cfg.presigned_upload_ttl_secs.get(), 900);
    }

    #[test]
    fn env_overrides_default_max_concurrent_transcodes() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "4".into()));
        let cfg = Config::from_iter(vars).unwrap();
        assert_eq!(cfg.max_concurrent_transcodes.get(), 4);
    }

    #[test]
    fn env_overrides_server_defaults() {
        let mut vars = mandatory_vars();
        vars.push(("SERVER_HOST".into(), "127.0.0.1".into()));
        vars.push(("SERVER_PORT".into(), "8080".into()));
        vars.push(("LOG_LEVEL".into(), "debug".into()));
        vars.push(("MAX_UPLOAD_BYTES".into(), "12345".into()));
        vars.push(("PRESIGNED_UPLOAD_TTL_SECS".into(), "120".into()));
        let cfg = Config::from_iter(vars).unwrap();
        assert_eq!(cfg.server_host, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(cfg.server_port.get(), 8080);
        assert!(matches!(cfg.log_level, LogLevel::Debug));
        assert_eq!(cfg.max_upload_bytes.get(), 12345);
        assert_eq!(cfg.presigned_upload_ttl_secs.get(), 120);
    }

    #[test]
    fn missing_mandatory_field_returns_error() {
        // Remove DATABASE_URL to trigger a missing-field error.
        let vars: Vec<_> = mandatory_vars()
            .into_iter()
            .filter(|(k, _)| k != "DATABASE_URL")
            .collect();
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn zero_transcodes_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "0".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn non_numeric_transcodes_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "abc".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn zero_server_port_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("SERVER_PORT".into(), "0".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn zero_max_upload_bytes_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_UPLOAD_BYTES".into(), "0".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn zero_presigned_upload_ttl_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("PRESIGNED_UPLOAD_TTL_SECS".into(), "0".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }

    #[test]
    fn public_object_url_builds_valid_url_and_trims_trailing_slash() {
        let cfg = Config::from_iter(mandatory_vars()).unwrap();
        let url = cfg
            .public_object_url("raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video")
            .unwrap();

        assert_eq!(
            url.as_str(),
            "https://cdn.example.com/raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video"
        );
    }

    #[test]
    fn public_object_url_returns_error_for_invalid_cdn_domain() {
        let mut vars = mandatory_vars();
        vars.retain(|(k, _)| k != "PUBLIC_CDN_DOMAIN");
        vars.push(("PUBLIC_CDN_DOMAIN".into(), "not-a-valid-url".into()));
        let cfg = Config::from_iter(vars).unwrap();

        let result = cfg.public_object_url("raw/01ARZ3NDEKTSV4RRFFQ69G5FAV/video");
        assert!(result.is_err());
    }
}
