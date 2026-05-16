use crate::state::AppState;
use chrono::{Datelike, Duration as ChronoDuration, TimeZone, Timelike, Utc};
use gear5_core::scraper::run_once;
use std::time::Duration;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        if state.cfg.scrape.run_at_startup {
            run(state.clone()).await;
        }
        loop {
            let wait = next_delay(state.cfg.scrape.cron_hour_utc);
            tracing::info!(
                hours = wait.as_secs() / 3600,
                minutes = (wait.as_secs() % 3600) / 60,
                "next scheduled scrape"
            );
            tokio::time::sleep(wait).await;
            run(state.clone()).await;
        }
    });
}

async fn run(state: AppState) {
    let _guard = state.scrape_lock.lock().await;
    tracing::info!("starting scheduled scrape");
    match run_once(
        &state.pool,
        &state.http,
        &state.cfg.scrape,
        &state.cfg.images.dir,
    )
    .await
    {
        Ok(r) => tracing::info!(
            run_id = r.run_id,
            status = %r.status,
            cards_seen = r.cards_seen,
            cards_inserted = r.cards_inserted,
            cards_updated = r.cards_updated,
            "scheduled scrape finished"
        ),
        Err(e) => tracing::error!(error = %e, "scheduled scrape failed"),
    }
}

fn next_delay(hour_utc: u32) -> Duration {
    let now = Utc::now();
    let target_today = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), hour_utc.min(23), 0, 0)
        .single()
        .unwrap_or(now);
    let target = if target_today > now {
        target_today
    } else {
        target_today + ChronoDuration::days(1)
    };
    let diff = target - now;
    let secs = diff.num_seconds().max(60);
    let _ = now.second();
    Duration::from_secs(secs as u64)
}
