use serde::Deserialize;
use std::net::IpAddr;
use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use thiserror::Error;
use url::Url;

// TODO: consider breaking monolithic config into smaller domain-specific structs
// TODO: double check test coverage for all fields and error cases

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

    #[serde(default = "defaults::ui_origin")]
    pub ui_origin: String,

    /// Duration in seconds after which pending uploads are considered "zombies" and eligible for cleanup.
    #[serde(default = "defaults::zombie_timeout_secs")]
    pub zombie_timeout_secs: NonZeroU64,

    /// Interval in seconds at which the zombie sweeper runs.
    #[serde(default = "defaults::zombie_sweep_interval_secs")]
    pub zombie_sweep_interval_secs: NonZeroU64,

    /// Buffer size for the channel used to communicate upload completion events to the worker.
    #[serde(default = "defaults::worker_channel_buffer_size")]
    pub worker_channel_buffer_size: usize,

    /// TTL in seconds for presigned ffprobe URLs used to fetch metadata after upload.
    #[serde(default = "defaults::presigned_probe_ttl_secs")]
    pub presigned_probe_ttl_secs: NonZeroU64,

    /// Duration in seconds after which pending uploads that haven't completed are automatically marked as failed.
    #[serde(default = "defaults::pending_upload_ttl_secs")]
    pub pending_upload_ttl_secs: NonZeroU64,

    #[serde(default = "defaults::worker_temp_dir")]
    pub worker_temp_dir: PathBuf,

    #[serde(default = "defaults::segment_upload_concurrency")]
    pub segment_upload_concurrency: usize,

    #[serde(default = "defaults::transcode_heartbeat_interval_secs")]
    pub transcode_heartbeat_interval_secs: NonZeroU64,
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

#[cfg(test)]
impl Config {
    /// Build a minimal config suitable for unit and integration tests.
    pub fn test() -> Self {
        Self::from_iter([
            ("DATABASE_URL".into(), "postgres://localhost/test".into()),
            ("R2_ACCOUNT_ID".into(), "test".into()),
            ("R2_ACCESS_KEY_ID".into(), "test".into()),
            ("R2_SECRET_ACCESS_KEY".into(), "test".into()),
            ("R2_BUCKET_NAME".into(), "test".into()),
            ("PUBLIC_CDN_DOMAIN".into(), "https://cdn.example.com".into()),
        ])
        .expect("test config is valid")
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
    use std::{
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
    };

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

    pub fn ui_origin() -> String {
        "http://localhost:5173".to_string()
    }

    pub fn zombie_timeout_secs() -> NonZeroU64 {
        NonZeroU64::new(7200).expect("default zombie_timeout_secs must be non-zero") // 2 hours
    }

    pub fn zombie_sweep_interval_secs() -> NonZeroU64 {
        NonZeroU64::new(3600).expect("default zombie_sweep_interval_secs must be non-zero") // 1 hour
    }

    pub fn worker_channel_buffer_size() -> usize {
        100
    }

    pub fn presigned_probe_ttl_secs() -> NonZeroU64 {
        NonZeroU64::new(300).expect("default presigned_probe_ttl_secs must be non-zero") // 5 minutes
    }

    pub fn pending_upload_ttl_secs() -> NonZeroU64 {
        NonZeroU64::new(3600).expect("default pending_upload_ttl_secs must be non-zero") // 1 hour
    }

    pub fn worker_temp_dir() -> PathBuf {
        std::env::temp_dir().join("video-worker")
    }

    pub fn segment_upload_concurrency() -> usize {
        5
    }

    pub fn transcode_heartbeat_interval_secs() -> NonZeroU64 {
        NonZeroU64::new(30).expect("default transcode_heartbeat_interval_secs must be non-zero")
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
        assert_eq!(cfg.ui_origin, "http://localhost:5173");
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
        vars.push(("UI_ORIGIN".into(), "http://127.0.0.1:5173".into()));
        let cfg = Config::from_iter(vars).unwrap();
        assert_eq!(cfg.server_host, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(cfg.server_port.get(), 8080);
        assert!(matches!(cfg.log_level, LogLevel::Debug));
        assert_eq!(cfg.max_upload_bytes.get(), 12345);
        assert_eq!(cfg.presigned_upload_ttl_secs.get(), 120);
        assert_eq!(cfg.ui_origin, "http://127.0.0.1:5173");
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
