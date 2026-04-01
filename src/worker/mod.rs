mod processor;

pub use processor::VideoProcessor;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::{Semaphore, mpsc};
use ulid::Ulid;

use crate::{
    domain::{ContainerFormat, UploadContentTypeError},
    file_transfer::FileTransferError,
    media_probe::FfprobeError,
    media_transcoder::TranscoderError,
    repository::VideoRepository,
    storage::R2StorageError,
};

/// Errors that can occur during worker operations, including video processing and cleanup tasks.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("storage error: {0}")]
    Storage(#[from] R2StorageError),

    #[error("media probe error: {0}")]
    MediaProbe(#[from] FfprobeError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ffmpeg transmux failed: {0}")]
    Ffmpeg(String),

    #[error("video not found: {0}")]
    NotFound(Ulid),

    #[error("invalid upload content type: {0}")]
    InvalidContentType(#[from] UploadContentTypeError),

    #[error("unsupported container for transmux: {0:?}")]
    UnsupportedContainer(ContainerFormat),

    #[error("cannot determine transmux target container")]
    NoTargetContainer,

    #[error("transcoder error: {0}")]
    Transcoder(#[from] TranscoderError),

    #[error("file transfer error: {0}")]
    Transfer(#[from] FileTransferError),

    #[error("tokio semaphore acquire error: {0}")]
    TokioAcquire(#[from] tokio::sync::AcquireError),

    #[error("tokio task join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
}

/// Worker module responsible for background tasks like video processing and cleanup of stale jobs.
pub struct Worker {
    /// Receiver for video processing jobs sent from the API when uploads are completed.
    rx: mpsc::Receiver<Ulid>,
    /// Core processor that encapsulates the logic for handling video processing steps like probing, transmuxing, and transcoding.
    processor: VideoProcessor,
    /// Semaphore to limit concurrent processing jobs and prevent resource exhaustion.
    semaphore: Arc<Semaphore>,
}

impl Worker {
    pub fn new(
        rx: mpsc::Receiver<Ulid>,
        processor: VideoProcessor,
        max_concurrent_jobs: usize,
    ) -> Self {
        Self {
            rx,
            processor,
            semaphore: Arc::new(Semaphore::new(max_concurrent_jobs)),
        }
    }

    /// Main loop for processing video jobs received from the API.
    pub async fn run_worker_loop(&mut self) {
        tracing::info!("worker loop started");

        while let Some(ulid) = self.rx.recv().await {
            let permit = self.semaphore.clone().acquire_owned().await.unwrap();
            let processor = self.processor.clone();
            tokio::spawn(async move {
                if let Err(e) = processor.process(ulid).await {
                    tracing::error!(%ulid, error = %e, "failed to process video");
                }
                drop(permit);
            });
        }
    }

    /// Background task to sweep and fail zombie jobs and clean up stale pending uploads.
    /// NOTE: This runs in the same worker process for simplicity, but could be broken out into separate tasks or even a separate service if necessary.
    pub async fn run_cleanup(
        repository: Arc<dyn VideoRepository>,
        timeout: Duration,
        sweep_interval: Duration,
        pending_upload_ttl: Duration,
    ) {
        tracing::info!(
            ?timeout,
            ?sweep_interval,
            ?pending_upload_ttl,
            "cleanup task started"
        );
        let mut interval = tokio::time::interval(sweep_interval);

        loop {
            interval.tick().await;

            // 1. Mark stuck processing jobs as failed
            match repository.mark_zombie_jobs_failed(timeout).await {
                Ok(count) if count > 0 => {
                    tracing::warn!(count, "swept zombie jobs to failed status")
                }
                Err(e) => tracing::error!(error = %e, "failed to sweep zombie jobs"),
                _ => {}
            }

            // 2. Delete stale pending_upload rows
            match repository
                .delete_stale_pending_uploads(pending_upload_ttl)
                .await
            {
                Ok(count) if count > 0 => {
                    tracing::info!(count, "deleted stale pending_upload rows")
                }
                Err(e) => tracing::error!(error = %e, "failed to delete stale pending uploads"),
                _ => {}
            }
        }
    }
}
