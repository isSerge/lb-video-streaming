mod api;
mod config;
mod domain;
mod file_transfer;
mod media_probe;
mod media_transcoder;
mod repository;
mod storage;
mod worker;

use config::Config;
use media_probe::{Ffprobe, MediaProbe};
use media_transcoder::MediaTranscoder;
use repository::{PgVideoRepository, VideoRepository};
use std::{path::PathBuf, sync::Arc};
use storage::{R2Storage, Storage};
use thiserror::Error;
use tracing_subscriber::EnvFilter;
use worker::{VideoProcessor, Worker};

use crate::file_transfer::{FileTransfer, HttpFileTransfer};

#[derive(Debug, Error)]
enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
}

// TODO: add graceful shutdown
#[tokio::main]
async fn main() -> Result<(), AppError> {
    let config = Config::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new(config.log_level.as_str()))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(cdn_domain = %config.public_cdn_domain, "configuration loaded");

    let video_repository = PgVideoRepository::new(&config).await?;
    tracing::info!("database connected and migrations applied");

    let r2_storage = R2Storage::new(&config);
    tracing::info!(bucket = %config.r2_bucket_name, "R2 storage client ready");

    let bind_addr = format!("{}:{}", config.server_host, config.server_port.get());

    // Initialize shared services and state for API handlers and worker tasks.
    let video_repository: Arc<dyn VideoRepository> = Arc::new(video_repository);
    let storage: Arc<dyn Storage> = Arc::new(r2_storage);
    let media_probe: Arc<dyn MediaProbe> = Arc::new(Ffprobe::default());
    let media_transcoder: Arc<dyn MediaTranscoder> = Arc::new(media_transcoder::Ffmpeg::default());
    let http_client = reqwest::Client::new(); // Default has no timeout which is desirable
    let file_transfer: Arc<dyn FileTransfer> = Arc::new(HttpFileTransfer::new(http_client));

    // Create a channel for communicating upload completion events to the worker.
    let (worker_tx, worker_rx) = tokio::sync::mpsc::channel(config.worker_channel_buffer_size);

    // Spawn worker tasks for processing uploads and sweeping zombies.

    let processor = VideoProcessor::new(
        Arc::clone(&video_repository),
        Arc::clone(&storage),
        Arc::clone(&media_probe),
        Arc::clone(&media_transcoder),
        Arc::clone(&file_transfer),
        PathBuf::from(&config.worker_temp_dir),
    );
    let mut worker = Worker::new(worker_rx, processor, config.max_concurrent_transcodes.get());
    let worker_video_repo_clone = Arc::clone(&video_repository);
    // TODO: use handlers during graceful shutdown to ensure all tasks are properly stopped and no jobs are lost
    let _worker_handle = tokio::spawn(async move { worker.run_worker_loop().await });
    let _cleanup_handle = tokio::spawn(async move {
        Worker::run_cleanup(
            worker_video_repo_clone,
            config.zombie_timeout_secs,
            config.zombie_sweep_interval_secs,
            config.pending_upload_ttl_secs,
        )
    });

    // Recover jobs lost during restart by sending all pending uploads to the worker.
    let recovered = video_repository.recover_pending_jobs().await?;
    for ulid in recovered {
        // TODO: consider batching, rate-limiting or other strategies
        // TODO: consider error handling and retry logic here to avoid losing jobs
        let _ = worker_tx.send(ulid).await;
    }

    let state = api::AppState::new(
        video_repository,
        storage,
        media_probe,
        Arc::new(config),
        worker_tx,
    );
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!(addr = %listener.local_addr()?, "server listening");
    axum::serve(listener, app).await?;

    Ok(())
}
