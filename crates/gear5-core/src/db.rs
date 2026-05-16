use crate::config::DatabaseConfig;
use crate::Result;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub async fn connect(cfg: &DatabaseConfig) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&cfg.url)
        .await?;
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    crate::MIGRATOR.run(pool).await?;
    Ok(())
}
