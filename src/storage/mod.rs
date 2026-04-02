pub mod port;

pub use port::Storage;

use aws_credential_types::Credentials;
use aws_sdk_s3::{Client, config::Region, presigning::PresigningConfig};
use std::time::Duration;
use thiserror::Error;
use url::{ParseError, Url};

use crate::{
    config::Config,
    domain::{HLSKey, ManifestKey, RawUploadKey, TransmuxKey, UploadContentType},
};

/// Holds storage resources for interacting with Cloudflare R2.
pub struct R2Storage {
    client: Client,
    bucket_name: String,
    /// Default TTL for presigned upload and download URLs.
    url_ttl_secs: u64,
    /// TTL specifically for presigned ffprobe probe URLs.
    probe_ttl_secs: u64,
}

#[derive(Debug, Error)]
pub enum R2StorageError {
    #[error("invalid presigned URL ttl: {0}")]
    InvalidTtl(String),

    #[error("failed to create presigned upload URL: {0}")]
    Presign(String),

    #[error(transparent)]
    InvalidUrl(#[from] ParseError),

    #[error("storage operation failed: {0}")]
    Internal(String),
}

impl R2Storage {
    /// Build an S3-compatible client pointed at Cloudflare R2.
    ///
    /// R2's S3 endpoint is `https://<account_id>.r2.cloudflarestorage.com`.
    /// Path-style addressing is used (`force_path_style = true`) because R2's
    /// virtual-hosted-style requires per-bucket DNS which is not available on the
    /// free `.r2.dev` plan.
    pub fn new(config: &Config) -> Self {
        let endpoint_url = format!("https://{}.r2.cloudflarestorage.com", config.r2_account_id);

        let credentials = Credentials::new(
            &config.r2_access_key_id,
            &config.r2_secret_access_key,
            None,
            None,
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
            url_ttl_secs: config.storage.presigned_upload_ttl_secs,
            probe_ttl_secs: config.storage.presigned_probe_ttl_secs,
        }
    }

    /// Helper method to generate a presigned PUT URL for a given key and content type.
    async fn presign_put(&self, key: &str, content_type: &str) -> Result<Url, R2StorageError> {
        let presign_cfg = PresigningConfig::expires_in(Duration::from_secs(self.url_ttl_secs))
            .map_err(|e| R2StorageError::InvalidTtl(e.to_string()))?;

        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .content_type(content_type)
            .presigned(presign_cfg)
            .await
            .map_err(|e| R2StorageError::Presign(e.to_string()))?;

        Url::parse(presigned.uri()).map_err(R2StorageError::from)
    }

    /// Helper method to generate a presigned GET URL for a given key and TTL.
    async fn presign_get(&self, key: &str, ttl_secs: u64) -> Result<Url, R2StorageError> {
        let presign_cfg = PresigningConfig::expires_in(Duration::from_secs(ttl_secs))
            .map_err(|e| R2StorageError::InvalidTtl(e.to_string()))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .presigned(presign_cfg)
            .await
            .map_err(|e| R2StorageError::Presign(e.to_string()))?;

        Url::parse(presigned.uri()).map_err(R2StorageError::from)
    }
}

#[async_trait::async_trait]
impl Storage for R2Storage {
    async fn create_upload_url(
        &self,
        key: &RawUploadKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError> {
        self.presign_put(key, content_type).await
    }

    async fn create_download_url(&self, key: &RawUploadKey) -> Result<Url, R2StorageError> {
        self.presign_get(key, self.probe_ttl_secs).await
    }

    async fn create_transmux_upload_url(
        &self,
        key: &TransmuxKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError> {
        self.presign_put(key, content_type).await
    }

    async fn create_transmux_download_url(&self, key: &TransmuxKey) -> Result<Url, R2StorageError> {
        self.presign_get(key, self.url_ttl_secs).await
    }

    async fn create_manifest_upload_url(
        &self,
        key: &ManifestKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError> {
        self.presign_put(key, content_type).await
    }

    async fn create_hls_segment_upload_url(
        &self,
        key: &HLSKey,
        content_type: &UploadContentType,
    ) -> Result<Url, R2StorageError> {
        self.presign_put(key, content_type).await
    }

    async fn delete_object(&self, key: &str) -> Result<(), R2StorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| R2StorageError::Internal(e.to_string()))?;

        Ok(())
    }

    async fn ping(&self) -> Result<(), R2StorageError> {
        self.client
            .head_bucket()
            .bucket(&self.bucket_name)
            .send()
            .await
            .map_err(|e| R2StorageError::Internal(e.to_string()))?;

        Ok(())
    }
}
