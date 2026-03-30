use serde::Deserialize;
use thiserror::Error;

/// All runtime configuration for the application.
/// Mandatory fields have no default and will cause a panic at startup if absent.
/// Optional fields fall back to the constants in the `defaults` module.
/// Actual environment variables always take precedence over `.env` file values
/// because `dotenvy::dotenv()` never overwrites variables that are already set.
#[derive(Debug, Clone, Deserialize)]
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
    pub max_concurrent_transcodes: usize,
}

impl Config {
    /// Load configuration from the process environment, loading `.env` first
    /// (existing env vars win). Panics on any missing mandatory field or
    /// an invalid value so startup fails loudly rather than misbehaving at
    /// runtime.
    pub fn from_env() -> Self {
        // Load .env but never overwrite vars already set in the environment.
        let _ = dotenvy::dotenv();
        Self::from_iter(std::env::vars())
            .unwrap_or_else(|e| panic!("Failed to load configuration: {}", e))
    }

    /// Deserialise configuration from an arbitrary key-value iterator.
    /// Used by `from_env` and unit tests.
    fn from_iter<I>(vars: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let config: Config = envy::from_iter(vars)?;
        if config.max_concurrent_transcodes == 0 {
            return Err(ConfigError::ZeroTranscodes);
        }
        Ok(config)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration error: {0}")]
    Env(#[from] envy::Error),

    #[error("MAX_CONCURRENT_TRANSCODES must be greater than 0")]
    ZeroTranscodes,
}

mod defaults {
    pub fn max_concurrent_transcodes() -> usize {
        1
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
        assert_eq!(cfg.max_concurrent_transcodes, 1);
    }

    #[test]
    fn env_overrides_default_max_concurrent_transcodes() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "4".into()));
        let cfg = Config::from_iter(vars).unwrap();
        assert_eq!(cfg.max_concurrent_transcodes, 4);
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
        assert!(matches!(
            Config::from_iter(vars),
            Err(ConfigError::ZeroTranscodes)
        ));
    }

    #[test]
    fn non_numeric_transcodes_returns_error() {
        let mut vars = mandatory_vars();
        vars.push(("MAX_CONCURRENT_TRANSCODES".into(), "abc".into()));
        assert!(matches!(Config::from_iter(vars), Err(ConfigError::Env(_))));
    }
}
