use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client};

use crate::config::Config;

/// Holds storage resources for interacting with Cloudflare R2.
#[derive(Clone)]
pub struct Storage {
    client: Client,
}

impl Storage {
    /// Build an S3-compatible client pointed at Cloudflare R2.
    ///
    /// R2's S3 endpoint is `https://<account_id>.r2.cloudflarestorage.com`.
    /// Path-style addressing is used (`force_path_style = true`) because R2's
    /// virtual-hosted-style requires per-bucket DNS which is not available on the
    /// free `.r2.dev` plan.
    pub fn new(config: &Config) -> Self {
        let endpoint_url = format!(
            "https://{}.r2.cloudflarestorage.com",
            config.r2_account_id
        );

        let credentials = Credentials::new(
            &config.r2_access_key_id,
            &config.r2_secret_access_key,
            None, // session token — not used with R2 static keys
            None, // expiry
            "r2",
        );

        let sdk_config = aws_sdk_s3::config::Builder::new()
            .endpoint_url(endpoint_url)
            .credentials_provider(credentials)
            .region(Region::new("auto"))
            .force_path_style(true)
            .build();

        Self {
            client: Client::from_conf(sdk_config),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
