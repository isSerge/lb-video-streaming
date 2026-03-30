use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, presigning::PresigningConfig, Client};
use std::time::Duration;
use thiserror::Error;
use url::{ParseError, Url};

use crate::{config::Config, domain::{RawUploadKey, UploadContentType}};

/// Holds storage resources for interacting with Cloudflare R2.
pub struct R2Storage {
    client: Client,
    bucket_name: String,
    upload_url_ttl_secs: u64,
}

#[derive(Debug, Error)]
pub enum R2StorageError {
    #[error("invalid presigned URL ttl: {0}")]
    InvalidTtl(String),

    #[error("failed to create presigned upload URL: {0}")]
    Presign(String),

    #[error(transparent)]
    InvalidUrl(#[from] ParseError),
}

impl R2Storage {
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
            bucket_name: config.r2_bucket_name.clone(),
            upload_url_ttl_secs: config.presigned_upload_ttl_secs.get(),
        }
    }

    /// Create a presigned PUT URL for uploading a raw object.
    pub async fn create_upload_url(
        &self,
        key: &RawUploadKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError> {
        let presign_cfg = PresigningConfig::expires_in(Duration::from_secs(self.upload_url_ttl_secs))
            .map_err(|e| R2StorageError::InvalidTtl(e.to_string()))?;

        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&**key)
            .content_type(&**content_type)
            .presigned(presign_cfg)
            .await
            .map_err(|e| R2StorageError::Presign(e.to_string()))?;

        Url::parse(&presigned.uri().to_string()).map_err(R2StorageError::from)
    }
}
