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
use std::{sync::Arc, time::Duration};
use storage::{R2Storage, Storage};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
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

    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let config = Config::from_env()?;

    // Ensure the worker temp directory exists before starting the application
    let temp_root = config.worker.temp_dir.clone();
    std::fs::create_dir_all(&temp_root).map_err(AppError::Io)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new(config.server.log_level.as_str()))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(cdn_domain = %config.public_cdn_domain, "configuration loaded");

    let video_repository = PgVideoRepository::new(&config).await?;
    tracing::info!("database connected and migrations applied");

    let r2_storage = R2Storage::new(&config);
    tracing::info!(bucket = %config.r2_bucket_name, "R2 storage client ready");

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);

    // Cancellation token to signal shutdown to worker tasks
    let cancel_token = CancellationToken::new();

    // Spawn a task to listen for shutdown signals and trigger graceful shutdown.
    let shutdown_token = cancel_token.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        shutdown_token.cancel();
    });

    // Initialize shared services and state for API handlers and worker tasks.
    let video_repository: Arc<dyn VideoRepository> = Arc::new(video_repository);
    let storage: Arc<dyn Storage> = Arc::new(r2_storage);
    let media_probe: Arc<dyn MediaProbe> = Arc::new(Ffprobe::default());
    let media_transcoder: Arc<dyn MediaTranscoder> = Arc::new(media_transcoder::Ffmpeg::default());

    // connect_timeout and a read_timeout apply to gaps between chunks
    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(config.worker.http_connect_timeout_secs))
        .read_timeout(Duration::from_secs(config.worker.http_read_timeout_secs))
        .build()?;

    let file_transfer: Arc<dyn FileTransfer> =
        Arc::new(HttpFileTransfer::new(http_client, config.worker.clone()));

    // Create a channel for communicating upload completion events to the worker.
    let (worker_tx, worker_rx) =
        tokio::sync::mpsc::channel(config.worker.worker_channel_buffer_size);

    // Spawn worker tasks for processing uploads and sweeping zombies.

    let processor = VideoProcessor::new(
        Arc::clone(&video_repository),
        Arc::clone(&storage),
        Arc::clone(&media_probe),
        Arc::clone(&media_transcoder),
        Arc::clone(&file_transfer),
        config.worker.clone(),
    );
    let mut worker = Worker::new(worker_rx, processor, config.worker.clone());
    let worker_video_repo_clone = Arc::clone(&video_repository);
    // TODO: use handlers during graceful shutdown to ensure all tasks are properly stopped and no jobs are lost
    let worker_token_clone = cancel_token.clone();
    let worker_handle =
        tokio::spawn(async move { worker.run_worker_loop(worker_token_clone).await });

    let cleanup_token_clone = cancel_token.clone();
    let cleanup_handle = tokio::spawn(async move {
        Worker::run_cleanup(
            worker_video_repo_clone,
            Duration::from_secs(config.worker.zombie_timeout_secs),
            Duration::from_secs(config.worker.zombie_sweep_interval_secs),
            Duration::from_secs(config.storage.pending_upload_ttl_secs),
            cleanup_token_clone,
        )
        .await
    });

    // Recover jobs lost during restart by sending all pending uploads to the worker.
    let recovered = video_repository.recover_pending_jobs().await?;
    for ulid in recovered {
        // TODO: consider batching, rate-limiting or other strategies
        // TODO: consider error handling and retry logic here to avoid losing jobs
        let _ = worker_tx.send(ulid).await;
    }

    let config_arc = Arc::new(config);
    let state = api::AppState::new(
        video_repository,
        storage,
        media_probe,
        config_arc.clone(),
        worker_tx,
    );
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!(addr = %listener.local_addr()?, "server listening");
    let server_token_clone = cancel_token.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(server_token_clone.cancelled_owned())
        .await?;

    let bg_tasks = async {
        let (cleanup_result, worker_result) = tokio::join!(cleanup_handle, worker_handle);
        if let Err(e) = cleanup_result {
            tracing::error!("Cleanup task panicked: {}", e);
        }
        if let Err(e) = worker_result {
            tracing::error!("Worker task panicked: {}", e);
        }
    };

    match tokio::time::timeout(
        Duration::from_secs(config_arc.server.shutdown_timeout_secs),
        bg_tasks,
    )
    .await
    {
        Ok(_) => {
            tracing::info!("Graceful shutdown complete.");
        }
        Err(_) => {
            tracing::warn!("Graceful shutdown timed out. Hard-killing remaining tasks. Jobs should be recovered on next startup");
        }
    }

    tracing::info!("application shutdown complete");

    Ok(())
}

/// Wait for either a CTRL+C (SIGINT) or a SIGTERM signal.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C (SIGINT), initiating graceful shutdown...");
        },
        _ = sigterm => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown...");
        },
    }
}
