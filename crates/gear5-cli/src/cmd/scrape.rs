use anyhow::{Context, Result};
use clap::Subcommand;
use gear5_core::config::Config;
use gear5_core::db;
use gear5_core::scraper::{run_once, HttpClient};
use sqlx::Row;

#[derive(Subcommand, Debug)]
pub enum ScrapeCmd {
    /// Run a full scrape once and exit.
    Run,
    /// Show the last 10 scrape runs.
    Status,
}

pub async fn run(cmd: ScrapeCmd) -> Result<()> {
    let cfg = Config::load().context("load config")?;
    let pool = db::connect(&cfg.database).await?;
    db::migrate(&pool).await?;
    match cmd {
        ScrapeCmd::Run => {
            tokio::fs::create_dir_all(&cfg.images.dir).await?;
            let http = HttpClient::new(&cfg.scrape).context("http client")?;
            let report = run_once(&pool, &http, &cfg.scrape, &cfg.images.dir).await?;
            println!(
                "run={} status={} sets_total={} sets_ok={} cards_seen={} inserted={} updated={}",
                report.run_id,
                report.status,
                report.sets_total,
                report.sets_ok,
                report.cards_seen,
                report.cards_inserted,
                report.cards_updated,
            );
        }
        ScrapeCmd::Status => {
            let rows = sqlx::query(
                r#"
                SELECT id, started_at, finished_at, status,
                       sets_total, sets_ok, cards_seen, cards_inserted, cards_updated, error
                FROM scrape_runs
                ORDER BY id DESC
                LIMIT 10
                "#,
            )
            .fetch_all(&pool)
            .await?;
            println!(
                "{:>5}  {:<25}  {:<10}  {:>5}/{:<5}  {:>6}  {:>5}  {:>5}",
                "id", "started", "status", "ok", "total", "cards", "ins", "upd"
            );
            for r in rows {
                let id: i64 = r.try_get("id")?;
                let started: chrono::DateTime<chrono::Utc> = r.try_get("started_at")?;
                let status: String = r.try_get("status")?;
                let total: Option<i32> = r.try_get("sets_total")?;
                let ok: Option<i32> = r.try_get("sets_ok")?;
                let seen: Option<i32> = r.try_get("cards_seen")?;
                let ins: Option<i32> = r.try_get("cards_inserted")?;
                let upd: Option<i32> = r.try_get("cards_updated")?;
                let err: Option<String> = r.try_get("error")?;
                println!(
                    "{:>5}  {:<25}  {:<10}  {:>5}/{:<5}  {:>6}  {:>5}  {:>5}{}",
                    id,
                    started.format("%Y-%m-%d %H:%M:%SZ"),
                    status,
                    ok.unwrap_or(0),
                    total.unwrap_or(0),
                    seen.unwrap_or(0),
                    ins.unwrap_or(0),
                    upd.unwrap_or(0),
                    err.map(|e| format!("  err={e}")).unwrap_or_default(),
                );
            }
        }
    }
    Ok(())
}
