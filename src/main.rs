mod api;
mod config;
mod domain;
mod media_probe;
mod repository;
mod storage;

use config::Config;
use media_probe::{Ffprobe, MediaProbe};
use repository::{PgVideoRepository, VideoRepository};
use std::sync::Arc;
use storage::{R2Storage, Storage};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

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

    let state = api::AppState::new(video_repository, storage, media_probe, Arc::new(config));
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
