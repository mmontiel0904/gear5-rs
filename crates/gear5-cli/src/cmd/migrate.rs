use anyhow::{Context, Result};
use gear5_core::config::Config;
use gear5_core::db;

pub async fn run() -> Result<()> {
    let cfg = Config::load().context("load config")?;
    let pool = db::connect(&cfg.database).await?;
    db::migrate(&pool).await?;
    println!("migrations applied");
    Ok(())
}
