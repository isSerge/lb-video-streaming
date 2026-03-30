mod api;
mod config;
mod domain;
mod r2_storage;
mod video_repository;

use config::Config;
use std::sync::Arc;
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

    let video_repository = video_repository::VideoRepository::new(&config).await?;
    tracing::info!("database connected and migrations applied");

    let r2_storage = r2_storage::R2Storage::new(&config);
    tracing::info!(bucket = %config.r2_bucket_name, "R2 storage client ready");

    let bind_addr = format!("{}:{}", config.server_host, config.server_port.get());

    let state = api::AppState::new(
        Arc::new(video_repository),
        Arc::new(r2_storage),
        Arc::new(config),
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
