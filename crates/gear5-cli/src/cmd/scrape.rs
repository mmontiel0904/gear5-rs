use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use gear5_core::config::Config;
use gear5_core::db;
use gear5_core::scraper::{run_once, run_one, HttpClient};
use sqlx::Row;

#[derive(Subcommand, Debug)]
pub enum ScrapeCmd {
    /// Run a scrape once and exit. Without --set, scrapes every dropdown entry.
    Run(RunArgs),
    /// Show recent scrape runs (latest 10) or per-set detail for a single run.
    Status(StatusArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Optional `source_series` value from the site's dropdown (e.g. `569101` for OP-01).
    /// When set, only that one set is scraped. Useful for verifying the POST series filter.
    #[arg(long = "set", value_name = "SOURCE_SERIES")]
    pub set: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Show per-set breakdown for the given run id instead of the aggregate run list.
    #[arg(long = "detail", value_name = "RUN_ID")]
    pub detail: Option<i64>,
}

pub async fn run(cmd: ScrapeCmd) -> Result<()> {
    let cfg = Config::load().context("load config")?;
    let pool = db::connect(&cfg.database).await?;
    db::migrate(&pool).await?;
    match cmd {
        ScrapeCmd::Run(args) => {
            tokio::fs::create_dir_all(&cfg.images.dir).await?;
            let http = HttpClient::new(&cfg.scrape).context("http client")?;
            let report = match args.set.as_deref() {
                Some(series) => {
                    run_one(&pool, &http, &cfg.scrape, &cfg.images.dir, series).await?
                }
                None => run_once(&pool, &http, &cfg.scrape, &cfg.images.dir).await?,
            };
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
        ScrapeCmd::Status(args) => match args.detail {
            Some(run_id) => print_run_detail(&pool, run_id).await?,
            None => print_recent_runs(&pool).await?,
        },
    }
    Ok(())
}

async fn print_recent_runs(pool: &sqlx::PgPool) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT id, started_at, finished_at, status,
               sets_total, sets_ok, cards_seen, cards_inserted, cards_updated, error
        FROM scrape_runs
        ORDER BY id DESC
        LIMIT 10
        "#,
    )
    .fetch_all(pool)
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
    Ok(())
}

async fn print_run_detail(pool: &sqlx::PgPool, run_id: i64) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT source_series, set_id, cards_seen, status, error, started_at, finished_at
        FROM scrape_run_sets
        WHERE run_id = $1
        ORDER BY source_series
        "#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        println!("no per-set rows for run {run_id}");
        return Ok(());
    }
    println!(
        "{:<14}  {:<8}  {:>6}  {:<8}  {:<25}",
        "source_series", "set_id", "cards", "status", "finished"
    );
    for r in rows {
        let source: String = r.try_get("source_series")?;
        let set_id: Option<String> = r.try_get("set_id")?;
        let cards_seen: i32 = r.try_get("cards_seen")?;
        let status: String = r.try_get("status")?;
        let finished: Option<chrono::DateTime<chrono::Utc>> = r.try_get("finished_at")?;
        let err: Option<String> = r.try_get("error")?;
        println!(
            "{:<14}  {:<8}  {:>6}  {:<8}  {:<25}{}",
            source,
            set_id.as_deref().unwrap_or("-"),
            cards_seen,
            status,
            finished
                .map(|t| t.format("%Y-%m-%d %H:%M:%SZ").to_string())
                .unwrap_or_else(|| "-".to_string()),
            err.map(|e| format!("  err={e}")).unwrap_or_default(),
        );
    }
    Ok(())
}
