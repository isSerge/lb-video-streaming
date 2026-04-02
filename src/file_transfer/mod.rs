pub mod port;

pub use port::FileTransfer;

use backon::{ExponentialBuilder, Retryable};
use std::{path::Path, time::Duration};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::Url;

use crate::{config::FileTransferConfig, domain::UploadContentType};

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
    retry_policy: ExponentialBuilder,
}

fn it_retrieable_error(e: &FileTransferError) -> bool {
    matches!(e, FileTransferError::Http(re) if re.is_timeout() || re.is_connect())
}

impl HttpFileTransfer {
    pub fn new(client: reqwest::Client, config: FileTransferConfig) -> Self {
        // Configure the retry policy based on worker config parameters
        let retry_policy = ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(config.retry_min_delay_ms))
            .with_max_delay(Duration::from_millis(config.retry_max_delay_ms))
            .with_max_times(config.retry_max_times)
            .with_jitter();

        Self {
            client,
            retry_policy,
        }
    }
}

#[async_trait::async_trait]
impl FileTransfer for HttpFileTransfer {
    #[tracing::instrument(skip(self, url), fields(url = %url))]
    async fn download(&self, url: Url, dest: &Path) -> Result<(), FileTransferError> {
        // Define the download operation as a closure so it can be retried by the backoff policy.
        let download_op = || async {
            let mut response = self
                .client
                .get(url.clone())
                .send()
                .await?
                .error_for_status()?;
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
            Ok::<(), FileTransferError>(())
        };

        // Use the retry policy to execute the download operation
        download_op
            .retry(self.retry_policy)
            .when(it_retrieable_error)
            .await?;

        Ok(())
    }

    #[tracing::instrument(skip(self, url, content_type), fields(url = %url, content_type = %**content_type))]
    async fn upload(
        &self,
        src: &Path,
        url: Url,
        content_type: &UploadContentType,
    ) -> Result<(), FileTransferError> {
        // Define the upload operation as a closure for retrying.
        let upload_op = || async {
            let file = tokio::fs::File::open(src).await?;
            let file_size = file.metadata().await?.len();
            tracing::debug!(bytes = file_size, "starting upload");

            let stream = FramedRead::new(file, BytesCodec::new());
            let body = reqwest::Body::wrap_stream(stream);

            self.client
                .put(url.clone())
                .header("content-type", &**content_type)
                .header("content-length", file_size)
                .body(body)
                .send()
                .await?
                .error_for_status()?;

            Ok::<(), FileTransferError>(())
        };

        // Use the retry policy to execute the upload operation
        upload_op
            .retry(self.retry_policy)
            .when(it_retrieable_error)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::UploadContentType;
    use std::io::Write;
    use std::str::FromStr;
    use tempfile::NamedTempFile;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> FileTransferConfig {
        FileTransferConfig {
            retry_min_delay_ms: 1,
            retry_max_delay_ms: 10,
            retry_max_times: 5,
            connect_timeout_secs: 1,
            read_timeout_secs: 1,
        }
    }

    #[tokio::test]
    async fn test_download_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello world"))
            .mount(&server)
            .await;

        let ft = HttpFileTransfer::new(reqwest::Client::new(), test_config());
        let dest = NamedTempFile::new().unwrap();
        let url = Url::parse(&format!("{}/file", server.uri())).unwrap();

        ft.download(url, dest.path()).await.unwrap();

        let content = std::fs::read_to_string(dest.path()).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_upload_success() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/upload"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let ft = HttpFileTransfer::new(reqwest::Client::new(), test_config());
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "upload content").unwrap();

        let url = Url::parse(&format!("{}/upload", server.uri())).unwrap();
        let ct = UploadContentType::from_str("text/plain").unwrap();

        ft.upload(file.path(), url, &ct).await.unwrap();
    }

    #[tokio::test]
    async fn test_download_retries_on_connect_error() {
        let server = MockServer::start().await;

        // First request is delayed to trigger a timeout and retry
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("done")
                    .set_delay(Duration::from_millis(100)), // delay longer than client timeout to trigger retry
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        // Subsequent request succeeds immediately
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(ResponseTemplate::new(200).set_body_string("recovered"))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50)) // 50ms < 100ms delay
            .build()
            .unwrap();

        let ft = HttpFileTransfer::new(client, test_config());
        let dest = NamedTempFile::new().unwrap();
        let url = Url::parse(&format!("{}/slow", server.uri())).unwrap();

        ft.download(url, dest.path()).await.unwrap();
        let content = std::fs::read_to_string(dest.path()).unwrap();
        assert_eq!(content, "recovered");
    }

    #[tokio::test]
    async fn test_download_fails_after_max_retries() {
        let server = MockServer::start().await;

        // Always slow, always timing out
        Mock::given(method("GET"))
            .and(path("/timeout"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(10))
            .build()
            .unwrap();

        let mut config = test_config();
        config.retry_max_times = 2; // Fail fast

        let ft = HttpFileTransfer::new(client, config);
        let dest = NamedTempFile::new().unwrap();
        let url = Url::parse(&format!("{}/timeout", server.uri())).unwrap();

        let result = ft.download(url, dest.path()).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), FileTransferError::Http(e) if e.is_timeout());
    }

    #[tokio::test]
    async fn test_upload_retries_on_timeout() {
        let server = MockServer::start().await;

        // First request times out
        Mock::given(method("PUT"))
            .and(path("/upload-slow"))
            .respond_with(ResponseTemplate::new(201).set_delay(Duration::from_millis(100)))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        // Second request succeeds
        Mock::given(method("PUT"))
            .and(path("/upload-slow"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();

        let ft = HttpFileTransfer::new(client, test_config());
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "upload content").unwrap();

        let url = Url::parse(&format!("{}/upload-slow", server.uri())).unwrap();
        let ct = UploadContentType::from_str("text/plain").unwrap();

        ft.upload(file.path(), url, &ct)
            .await
            .expect("Upload should have recovered after retry");
    }

    #[tokio::test]
    async fn test_upload_fails_after_max_retries() {
        let server = MockServer::start().await;

        // Always slow
        Mock::given(method("PUT"))
            .and(path("/upload-timeout"))
            .respond_with(ResponseTemplate::new(201).set_delay(Duration::from_millis(100)))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(10))
            .build()
            .unwrap();

        let mut config = test_config();
        config.retry_max_times = 2; // Fail fast

        let ft = HttpFileTransfer::new(client, config);
        let file = NamedTempFile::new().unwrap();
        let url = Url::parse(&format!("{}/upload-timeout", server.uri())).unwrap();
        let ct = UploadContentType::from_str("text/plain").unwrap();

        let result = ft.upload(file.path(), url, &ct).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), FileTransferError::Http(e) if e.is_timeout());
    }
}
