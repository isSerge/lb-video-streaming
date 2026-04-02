use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;
use url::Url;

/// All runtime configuration for the application.
/// Mandatory fields have no default and will return an error at startup if absent.
/// Optional fields fall back to the constants in the `defaults` module.
/// Actual environment variables always take precedence over `.env` file values
/// because `dotenvy::dotenv()` never overwrites variables that are already set.
pub struct Config {
    // --- mandatory ---
    pub database_url: String,
    pub r2_account_id: String,
    pub r2_access_key_id: String,
    pub r2_secret_access_key: String,
    pub r2_bucket_name: String,
    pub public_cdn_domain: String,

    pub worker: WorkerConfig,
    pub server: ServerConfig,
    pub storage: StorageConfig,
}

// TODO: break it down
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Maximum number of videos that can be transcoded in parallel.
    pub max_concurrent_transcodes: usize,
    /// Root directory for the worker to store temporary files during processing.
    pub temp_dir: PathBuf,
    /// Number of segments to upload in parallel when uploading HLS outputs to storage.
    pub segment_upload_concurrency: usize,
    /// Interval in seconds at which the worker sends heartbeat logs during transcoding to indicate progress.
    pub transcode_heartbeat_interval_secs: u64,
    /// Duration in seconds after which pending uploads are considered "zombies" and eligible for cleanup.
    pub zombie_timeout_secs: u64,
    /// Interval in seconds at which the zombie sweeper runs.
    pub zombie_sweep_interval_secs: u64,
    /// Buffer size for the channel used to communicate upload completion events to the worker.
    pub worker_channel_buffer_size: usize,
    /// Timeout in seconds for establishing a connection during file transfers.
    pub http_connect_timeout_secs: u64,
    /// Timeout in seconds for reading a chunk of data during file transfers.
    pub http_read_timeout_secs: u64,
    /// Timeout in seconds for the transmuxing process.
    pub transmux_timeout_secs: u64,
    /// Timeout in seconds for the HLS transcoding process.
    pub transcode_timeout_secs: u64,
    /// Minimum delay for file transfer retries.
    pub file_transfer_retry_min_delay_ms: u64,
    /// Maximum delay for file transfer retries.
    pub file_transfer_retry_max_delay_ms: u64,
    /// Maximum number of retry attempts for file transfers.
    pub file_transfer_retry_max_times: usize,
    /// Number of consecutive failures before the worker circuit breaker opens.
    pub circuit_breaker_failure_threshold: u32,
    /// Minimum recovery delay in seconds for the circuit breaker.
    pub circuit_breaker_min_recovery_secs: u64,
    /// Maximum recovery delay in seconds for the circuit breaker.
    pub circuit_breaker_max_recovery_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
    pub log_level: LogLevel,
    pub ui_origin: String,
    pub max_upload_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub presigned_upload_ttl_secs: u64,
    /// TTL in seconds for presigned ffprobe URLs used to fetch metadata after upload.
    pub presigned_probe_ttl_secs: u64,
    /// Duration in seconds after which pending uploads that haven't completed are automatically marked as failed.
    pub pending_upload_ttl_secs: u64,
}

impl Config {
    /// Load configuration from the process environment, loading `.env` first
    /// (existing env vars win). Returns an error on any missing mandatory
    /// field or invalid value.
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        let map: HashMap<String, String> = std::env::vars().collect();
        Self::build(&map)
    }

    fn build(map: &HashMap<String, String>) -> Result<Self, ConfigError> {
        Ok(Config {
            database_url: require(map, "DATABASE_URL")?,
            r2_account_id: require(map, "R2_ACCOUNT_ID")?,
            r2_access_key_id: require(map, "R2_ACCESS_KEY_ID")?,
            r2_secret_access_key: require(map, "R2_SECRET_ACCESS_KEY")?,
            r2_bucket_name: require(map, "R2_BUCKET_NAME")?,
            public_cdn_domain: require(map, "PUBLIC_CDN_DOMAIN")?,
            worker: WorkerConfig {
                max_concurrent_transcodes: parse(map, "MAX_CONCURRENT_TRANSCODES", 1usize)?,
                temp_dir: parse(
                    map,
                    "WORKER_TEMP_DIR",
                    std::env::temp_dir().join("video-worker"),
                )?,
                segment_upload_concurrency: parse(map, "SEGMENT_UPLOAD_CONCURRENCY", 5usize)?,
                transcode_heartbeat_interval_secs: parse(
                    map,
                    "TRANSCODE_HEARTBEAT_INTERVAL_SECS",
                    30u64,
                )?,
                zombie_timeout_secs: parse(map, "ZOMBIE_TIMEOUT_SECS", 7200u64)?,
                zombie_sweep_interval_secs: parse(map, "ZOMBIE_SWEEP_INTERVAL_SECS", 3600u64)?,
                worker_channel_buffer_size: parse(map, "WORKER_CHANNEL_BUFFER_SIZE", 100usize)?,
                http_connect_timeout_secs: parse(map, "HTTP_CONNECT_TIMEOUT_SECS", 10u64)?,
                http_read_timeout_secs: parse(map, "HTTP_READ_TIMEOUT_SECS", 30u64)?,
                transmux_timeout_secs: parse(map, "TRANSMUX_TIMEOUT_SECS", 300u64)?,
                transcode_timeout_secs: parse(map, "TRANSCODE_TIMEOUT_SECS", 1800u64)?,
                file_transfer_retry_min_delay_ms: parse(
                    map,
                    "FILE_TRANSFER_RETRY_MIN_DELAY_MS",
                    500u64,
                )?,
                file_transfer_retry_max_delay_ms: parse(
                    map,
                    "FILE_TRANSFER_RETRY_MAX_DELAY_MS",
                    10000u64,
                )?,
                file_transfer_retry_max_times: parse(map, "FILE_TRANSFER_RETRY_MAX_TIMES", 5usize)?,
                circuit_breaker_failure_threshold: parse(
                    map,
                    "CIRCUIT_BREAKER_FAILURE_THRESHOLD",
                    5u32,
                )?,
                circuit_breaker_min_recovery_secs: parse(
                    map,
                    "CIRCUIT_BREAKER_MIN_RECOVERY_SECS",
                    10u64,
                )?,
                circuit_breaker_max_recovery_secs: parse(
                    map,
                    "CIRCUIT_BREAKER_MAX_RECOVERY_SECS",
                    60u64,
                )?,
            },
            server: ServerConfig {
                host: parse(map, "SERVER_HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED))?,
                port: parse(map, "SERVER_PORT", 3000u16)?,
                log_level: parse(map, "LOG_LEVEL", LogLevel::Info)?,
                ui_origin: opt(map, "UI_ORIGIN", "http://localhost:5173"),
                max_upload_bytes: parse(map, "MAX_UPLOAD_BYTES", 1_073_741_824u64)?,
            },
            storage: StorageConfig {
                presigned_upload_ttl_secs: parse(map, "PRESIGNED_UPLOAD_TTL_SECS", 900u64)?,
                presigned_probe_ttl_secs: parse(map, "PRESIGNED_PROBE_TTL_SECS", 300u64)?,
                pending_upload_ttl_secs: parse(map, "PENDING_UPLOAD_TTL_SECS", 3600u64)?,
            },
        })
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
    fn from_iter<I: IntoIterator<Item = (String, String)>>(vars: I) -> Result<Self, ConfigError> {
        let map: HashMap<String, String> = vars.into_iter().collect();
        Self::build(&map)
    }

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

/// Helper to extract a required field from the config map, returning a clear error if missing.
fn require(map: &HashMap<String, String>, key: &str) -> Result<String, ConfigError> {
    map.get(key)
        .cloned()
        .ok_or_else(|| ConfigError::MissingVar(key.to_owned()))
}

/// Return the value of `key` from `map`, or `default` if the key is absent.
fn opt(map: &HashMap<String, String>, key: &str, default: &str) -> String {
    map.get(key)
        .map(String::as_str)
        .unwrap_or(default)
        .to_owned()
}

/// Return the parsed value of `key` from `map`, or `default` if the key is absent.
/// Returns an error when the key is present but its value cannot be parsed.
fn parse<T>(map: &HashMap<String, String>, key: &str, default: T) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    match map.get(key) {
        Some(raw) => raw.parse().map_err(|e: T::Err| ConfigError::InvalidVar {
            key: key.to_owned(),
            err: e.to_string(),
        }),
        None => Ok(default),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl FromStr for LogLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            other => Err(format!(
                "unknown log level '{other}', expected one of: trace, debug, info, warn, error"
            )),
        }
    }
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
    #[error("missing required environment variable: {0}")]
    MissingVar(String),
    #[error("invalid value for environment variable {key}: {err}")]
    InvalidVar { key: String, err: String },
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
        assert_eq!(cfg.worker.max_concurrent_transcodes, 1);
    }

    #[test]
    fn defaults_server_host_port_and_log_level() {
        let cfg = Config::from_iter(mandatory_vars()).unwrap();
        assert_eq!(cfg.server.host, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(cfg.server.port, 3000);
        assert!(matches!(cfg.server.log_level, LogLevel::Info));
        assert_eq!(cfg.server.max_upload_bytes, 1_073_741_824);
        assert_eq!(cfg.storage.presigned_upload_ttl_secs, 900);
        assert_eq!(cfg.server.ui_origin, "http://localhost:5173");
    }

    #[test]
    fn env_overrides_default_max_concurrent_transcodes() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "4".into()));
        let cfg = Config::from_iter(vars).unwrap();
        assert_eq!(cfg.worker.max_concurrent_transcodes, 4);
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
        assert_eq!(cfg.server.host, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(cfg.server.port, 8080);
        assert!(matches!(cfg.server.log_level, LogLevel::Debug));
        assert_eq!(cfg.server.max_upload_bytes, 12345);
        assert_eq!(cfg.storage.presigned_upload_ttl_secs, 120);
        assert_eq!(cfg.server.ui_origin, "http://127.0.0.1:5173");
    }

    #[test]
    fn missing_mandatory_field_returns_error() {
        // Remove DATABASE_URL to trigger a missing-field error.
        let vars: Vec<_> = mandatory_vars()
            .into_iter()
            .filter(|(k, _)| k != "DATABASE_URL")
            .collect();
        assert!(Config::from_iter(vars).is_err());
    }

    #[test]
    fn non_numeric_transcodes_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "abc".into()));
        assert!(Config::from_iter(vars).is_err());
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
