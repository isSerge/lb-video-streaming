use std::{num::NonZeroU64, sync::Arc, time::Duration};

use tokio::sync::mpsc;
use ulid::Ulid;

use crate::repository::VideoRepository;

/// Worker module responsible for background tasks like video processing and cleanup of stale jobs.
pub struct Worker {
    rx: mpsc::Receiver<Ulid>,
}

impl Worker {
    pub fn new(rx: mpsc::Receiver<Ulid>) -> Self {
        Self { rx }
    }

    /// Main loop for processing video jobs received from the API.
    pub async fn run_worker_loop(&mut self) {
        tracing::info!("worker loop started");

        while let Some(ulid) = self.rx.recv().await {
            tracing::info!(%ulid, "worker received job");

            // TODO (Step 5 & 6):
            // 1. Fetch video record
            // 2. Transmux (if required)
            // 3. Transcode to HLS
            // 4. Update status & upload to R2
        }
    }

    /// Background task to sweep and fail "zombie" jobs that have been pending for too long.
    /// NOTE: This runs in the same worker process for simplicity, but could be a separate service or scheduled task in a more complex architecture.
    pub async fn run_zombie_sweeper(
        repository: Arc<dyn VideoRepository>,
        timeout_secs: NonZeroU64,
        sweep_interval_secs: NonZeroU64,
    ) {
        tracing::info!(timeout_secs, sweep_interval_secs, "zombie sweeper started");
        let mut interval = tokio::time::interval(Duration::from_secs(sweep_interval_secs.get()));

        loop {
            interval.tick().await;
            match repository.mark_zombie_jobs_failed(timeout_secs).await {
                Ok(count) if count > 0 => {
                    tracing::warn!(count, "swept zombie jobs to failed status")
                }
                Err(e) => tracing::error!(error = %e, "failed to sweep zombie jobs"),
                _ => {} // Normal, no zombies found
            }
        }
    }
}
