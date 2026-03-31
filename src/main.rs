mod api;
mod config;
mod domain;
mod media_probe;
mod repository;
mod storage;
mod worker;

use config::Config;
use media_probe::{Ffprobe, MediaProbe};
use repository::{PgVideoRepository, VideoRepository};
use std::sync::Arc;
use storage::{R2Storage, Storage};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

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

    let ffprobe = Ffprobe::default();

    let bind_addr = format!("{}:{}", config.server_host, config.server_port.get());

    let video_repository: Arc<dyn VideoRepository> = Arc::new(video_repository);
    let storage: Arc<dyn Storage> = Arc::new(r2_storage);
    let media_probe: Arc<dyn MediaProbe> = Arc::new(ffprobe);

    // Create a channel for communicating upload completion events to the worker.
    let (worker_tx, worker_rx) = tokio::sync::mpsc::channel(config.worker_channel_buffer_size);

    // Recover jobs lost during restart by sending all pending uploads to the worker.
    let recovered = video_repository.recover_pending_jobs().await?;
    for ulid in recovered {
        // TODO: consider batching, rate-limiting or other strategies
        // TODO: consider error handling and retry logic here to avoid losing jobs
        let _ = worker_tx.send(ulid).await;
    }

    // Spawn worker tasks for processing uploads and sweeping zombies.
    let mut worker = worker::Worker::new(worker_rx); // has to be mutable to receive from the channel
    let worker_video_repo_clone = Arc::clone(&video_repository);
    // TODO: use handlers during graceful shutdown to ensure all tasks are properly stopped and no jobs are lost
    let _worker_handle = tokio::spawn(async move { worker.run_worker_loop().await });
    let _zombie_sweeper_handle = tokio::spawn(async move {
        worker::Worker::run_zombie_sweeper(
            worker_video_repo_clone,
            config.zombie_timeout_secs,
            config.zombie_sweep_interval_secs,
        )
    });

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

#[derive(Debug, Error)]
enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
}
