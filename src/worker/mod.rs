mod processor;

use failsafe::{StateMachine, backoff, failure_policy, futures::CircuitBreaker};
pub use processor::VideoProcessor;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

use crate::{
    config::WorkerConfig,
    domain::{UploadContentTypeError, VideoStatus},
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

    #[error("video not found: {0}")]
    NotFound(Ulid),

    #[error("invalid upload content type: {0}")]
    InvalidContentType(#[from] UploadContentTypeError),

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

// Type alias for the specific failsafe state machine we are using
type WorkerCircuitBreaker =
    StateMachine<failure_policy::ConsecutiveFailures<backoff::Exponential>, ()>;

/// Worker module responsible for background tasks like video processing and cleanup of stale jobs.
pub struct Worker {
    /// Receiver for video processing jobs sent from the API when uploads are completed.
    rx: mpsc::Receiver<Ulid>,
    /// Sender to requeue jobs if necessary (e.g., after circuit breaker rejection).
    tx: mpsc::Sender<Ulid>,
    /// Core processor that encapsulates the logic for handling video processing steps like probing, transmuxing, and transcoding.
    processor: VideoProcessor,
    /// Repository for updating video statuses and metadata during processing and cleanup.
    repository: Arc<dyn VideoRepository>,
    /// Semaphore to limit concurrent processing jobs and prevent resource exhaustion.
    semaphore: Arc<Semaphore>,
    /// Circuit breaker to protect infrastructure from cascading failures.
    circuit_breaker: Arc<WorkerCircuitBreaker>,
    /// Delay in seconds before requeuing a job after a failure.
    job_requeue_delay_secs: u64,
}

impl Worker {
    pub fn new(
        rx: mpsc::Receiver<Ulid>,
        tx: mpsc::Sender<Ulid>,
        processor: VideoProcessor,
        repository: Arc<dyn VideoRepository>,
        config: WorkerConfig,
    ) -> Self {
        // Configure the Circuit Breaker with a policy of tripping after a certain number of consecutive failures
        let backoff = failsafe::backoff::exponential(
            Duration::from_secs(config.circuit_breaker.min_recovery_secs),
            Duration::from_secs(config.circuit_breaker.max_recovery_secs),
        );
        let policy =
            failure_policy::consecutive_failures(config.circuit_breaker.failure_threshold, backoff);

        let circuit_breaker = Arc::new(failsafe::Config::new().failure_policy(policy).build());

        Self {
            rx,
            tx,
            processor,
            repository,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_transcodes)),
            circuit_breaker,
            job_requeue_delay_secs: config.job_requeue_delay_secs,
        }
    }

    /// Main loop for processing video jobs received from the API.
    pub async fn run_worker_loop(&mut self, shutdown_token: CancellationToken) {
        tracing::info!("worker loop started");
        let mut active_jobs = tokio::task::JoinSet::new();
        let tx = self.tx.clone();

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    tracing::info!("Worker received cancellation. Waiting for active jobs to finish...");
                    break;
                }
                job = self.rx.recv() => {
                    let Some(ulid) = job else {
                        tracing::info!("worker channel closed, shutting down");
                        break;
                    };

                    let permit = self.semaphore.clone().acquire_owned().await.unwrap();
                    let processor = self.processor.clone();
                    let breaker = self.circuit_breaker.clone();
                    let tx_requeue = tx.clone();
                    let requeue_delay = self.job_requeue_delay_secs;
                    let repository = Arc::clone(&self.repository);

                    active_jobs.spawn(async move {
                        let _permit = permit;

                        // Wrap the entire processing pipeline in the circuit breaker
                        let result = breaker.call(async {
                            processor.process(ulid).await
                        }).await;

                        match result {
                            Ok(_) => {
                                tracing::info!(%ulid, "Job completed successfully");
                            }
                            Err(failsafe::Error::Rejected) => {
                                // The breaker is OPEN. R2 or Postgres is likely down.
                                tracing::error!(%ulid, "Circuit breaker OPEN: Job rejected to protect infrastructure");

                                tokio::time::sleep(Duration::from_secs(requeue_delay)).await;
                                if let Err(e) = tx_requeue.send(ulid).await {
                                    tracing::error!(%ulid, error = %e, "Failed to re-queue rejected job");
                                }
                            }
                            Err(failsafe::Error::Inner(e)) => {
                                // The job ran, but failed (e.g., FFmpeg crashed, or out of retries).
                                tracing::error!(%ulid, error = %e, "Job failed");

                                // Mark the job as failed in db
                                if let Err(e) = repository.update_status(ulid, VideoStatus::Failed).await {
                                    tracing::error!(%ulid, error = %e, "Failed to mark job as failed in database");
                                }
                            }
                        }
                    });
                }
                Some(res) = active_jobs.join_next() => {
                    if let Err(e) = res {
                        tracing::error!("Worker task panicked: {}", e);
                    }
                }
            }
        }

        while active_jobs.join_next().await.is_some() {}
        tracing::info!("worker loop shut down gracefully");
    }

    /// Background task to sweep and fail zombie jobs and clean up stale pending uploads.
    /// NOTE: This runs in the same worker process for simplicity, but could be broken out into separate tasks or even a separate service if necessary.
    pub async fn run_cleanup(
        repository: Arc<dyn VideoRepository>,
        timeout: Duration,
        sweep_interval: Duration,
        pending_upload_ttl: Duration,
        shutdown_token: CancellationToken,
    ) {
        tracing::info!(
            ?timeout,
            ?sweep_interval,
            ?pending_upload_ttl,
            "cleanup task started"
        );
        let mut interval = tokio::time::interval(sweep_interval);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    tracing::info!("cleanup task received shutdown signal");
                    break;
                }
                _ = interval.tick() => {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::WorkerConfig, file_transfer::port::MockFileTransfer,
        media_probe::port::MockMediaProbe, media_transcoder::port::MockMediaTranscoder,
        repository::port::MockVideoRepository, storage::port::MockStorage,
    };
    use mockall::predicate::*;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    fn dummy_worker_config() -> WorkerConfig {
        crate::config::Config::test().worker
    }

    #[tokio::test]
    async fn worker_loop_shuts_down_on_cancellation() {
        let (_rx_tx, rx) = mpsc::channel(1);
        let (tx, _rx_dummy) = mpsc::channel(1);
        let repo = Arc::new(MockVideoRepository::new());

        let processor = VideoProcessor::new(
            repo.clone(),
            Arc::new(MockStorage::new()),
            Arc::new(MockMediaProbe::new()),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(MockFileTransfer::new()),
            dummy_worker_config().processor,
        );

        let mut worker = Worker::new(rx, tx, processor, repo, dummy_worker_config());

        let cancel_token = CancellationToken::new();
        let cancel_clone = cancel_token.clone();

        // Immediately trigger cancellation
        cancel_clone.cancel();

        // The loop should exit immediately without hanging
        let result =
            tokio::time::timeout(Duration::from_secs(1), worker.run_worker_loop(cancel_token))
                .await;
        assert!(
            result.is_ok(),
            "Worker loop did not shut down on cancellation"
        );
    }

    #[tokio::test]
    async fn worker_loop_marks_job_failed_on_fatal_error() {
        let ulid = Ulid::new();
        let (rx_tx, rx) = mpsc::channel(1);
        let (tx, _rx_dummy) = mpsc::channel(1);

        let mut repo = MockVideoRepository::new();

        // 1. Mock a fatal database error to simulate an unrecoverable failure during the process() call.
        repo.expect_find_video_by_ulid()
            .once()
            .returning(|_| Err(sqlx::Error::RowNotFound));

        // 2. The Worker loop MUST catch this and explicitly mark the job as Failed.
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Failed))
            .once()
            .returning(|_, _| Ok(()));

        let repo_arc = Arc::new(repo);

        let processor = VideoProcessor::new(
            repo_arc.clone(),
            Arc::new(MockStorage::new()),
            Arc::new(MockMediaProbe::new()),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(MockFileTransfer::new()),
            dummy_worker_config().processor,
        );

        let mut worker = Worker::new(rx, tx, processor, repo_arc, dummy_worker_config());

        rx_tx.send(ulid).await.unwrap();

        let cancel_token = CancellationToken::new();
        let cancel_clone = cancel_token.clone();

        tokio::spawn(async move {
            // Give the worker time to process the job, then cancel the loop
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });

        // Run the worker
        worker.run_worker_loop(cancel_token).await;
    }
}
