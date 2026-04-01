pub mod port;

pub use port::FileTransfer;

use std::path::Path;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::Url;

use crate::domain::UploadContentType;

#[derive(Debug, Error)]
pub enum FileTransferError {
    #[error("http transfer failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("file io failed: {0}")]
    Io(#[from] std::io::Error),
}

/// HTTP-based implementation for streaming files.
pub struct HttpFileTransfer {
    client: reqwest::Client,
}

impl HttpFileTransfer {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl FileTransfer for HttpFileTransfer {
    #[tracing::instrument(skip(self, url), fields(url = %url))]
    async fn download(&self, url: Url, dest: &Path) -> Result<(), FileTransferError> {
        let mut response = self.client.get(url).send().await?.error_for_status()?;
        let content_length = response.content_length();
        let mut file = tokio::fs::File::create(dest).await?;
        let mut bytes_written: u64 = 0;

        while let Some(chunk) = response.chunk().await? {
            bytes_written += chunk.len() as u64;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        tracing::debug!(
            bytes = bytes_written,
            content_length = ?content_length,
            "download completed"
        );
        Ok(())
    }

    #[tracing::instrument(skip(self, url, content_type), fields(url = %url, content_type = %**content_type))]
    async fn upload(
        &self,
        src: &Path,
        url: Url,
        content_type: &UploadContentType,
    ) -> Result<(), FileTransferError> {
        let file = tokio::fs::File::open(src).await?;
        let file_size = file.metadata().await?.len();
        tracing::debug!(bytes = file_size, "starting upload");

        let stream = FramedRead::new(file, BytesCodec::new());
        let body = reqwest::Body::wrap_stream(stream);

        self.client
            .put(url)
            .header("content-type", &**content_type)
            .header("content-length", file_size)
            .body(body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
