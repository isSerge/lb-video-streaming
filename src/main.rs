mod api;
mod config;
mod db;
mod storage;

use config::Config;
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

    let database = db::Database::new(&config).await?;
    tracing::info!("database connected and migrations applied");

    let _storage = storage::Storage::new(&config);
    tracing::info!(bucket = %config.r2_bucket_name, "R2 storage client ready");

    let app = api::router();
    let bind_addr = format!("{}:{}", config.server_host, config.server_port.get());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!(addr = %listener.local_addr()?, "server listening");
    axum::serve(listener, app).await?;

    drop(database);

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
