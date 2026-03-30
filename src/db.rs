use sqlx::PgPool;

use crate::config::Config;

/// Holds database resources and startup routines.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Connect to Postgres and run any pending migrations.
    pub async fn new(config: &Config) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(&config.database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Get a reference to the connection pool for executing queries.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
