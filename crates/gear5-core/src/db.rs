use crate::config::DatabaseConfig;
use crate::Result;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub async fn connect(cfg: &DatabaseConfig) -> Result<PgPool> {
    let statement_timeout_ms = cfg.statement_timeout_ms;
    let idle_tx_timeout_ms = cfg.idle_tx_timeout_ms;
    let pool = PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                if statement_timeout_ms > 0 {
                    sqlx::query(&format!("SET statement_timeout = {statement_timeout_ms}",))
                        .execute(&mut *conn)
                        .await?;
                }
                if idle_tx_timeout_ms > 0 {
                    sqlx::query(&format!(
                        "SET idle_in_transaction_session_timeout = {idle_tx_timeout_ms}",
                    ))
                    .execute(&mut *conn)
                    .await?;
                }
                Ok(())
            })
        })
        .connect(&cfg.url)
        .await?;
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    crate::MIGRATOR.run(pool).await?;
    Ok(())
}
