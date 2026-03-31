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
    async fn download(&self, url: Url, dest: &Path) -> Result<(), FileTransferError> {
        let mut response = self.client.get(url).send().await?.error_for_status()?;
        let mut file = tokio::fs::File::create(dest).await?;

        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }

    async fn upload(
        &self,
        src: &Path,
        url: Url,
        content_type: &UploadContentType,
    ) -> Result<(), FileTransferError> {
        let file = tokio::fs::File::open(src).await?;
        let file_size = file.metadata().await?.len();

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
