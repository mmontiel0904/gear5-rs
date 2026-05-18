use crate::config::ScrapeConfig;
use crate::model::{ParsedCard, ParsedSet, ScrapeReport};
use crate::scraper::fetch::HttpClient;
use crate::scraper::parse;
use crate::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// On startup, mark any scrape_runs / scrape_run_sets rows that were left in the 'running' state
/// (e.g. because the process was killed mid-scrape) as 'failed'. Without this, health checks and
/// history queries would permanently show stale ghost runs.
pub async fn cleanup_stale_runs(pool: &PgPool) -> Result<()> {
    let run_rows = sqlx::query(
        r#"
        UPDATE scrape_runs
        SET status      = 'failed',
            finished_at = COALESCE(finished_at, now()),
            error       = 'process terminated unexpectedly'
        WHERE status = 'running'
        RETURNING id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let set_rows = sqlx::query(
        r#"
        UPDATE scrape_run_sets
        SET status      = 'failed',
            finished_at = COALESCE(finished_at, now()),
            error       = 'process terminated unexpectedly'
        WHERE status = 'running'
        "#,
    )
    .execute(pool)
    .await?;

    if !run_rows.is_empty() {
        tracing::warn!(
            runs = run_rows.len(),
            sets = set_rows.rows_affected(),
            "cleaned up stale scrape_runs left by a previous crash"
        );
    }

    Ok(())
}

/// Top-level scrape entry point. Single-flight; callers are expected to serialise.
pub async fn run_once(
    pool: &PgPool,
    http: &HttpClient,
    cfg: &ScrapeConfig,
    image_dir: &Path,
) -> Result<ScrapeReport> {
    fs::create_dir_all(image_dir).await?;
    let run_id = open_run(pool).await?;
    let mut report = ScrapeReport {
        run_id,
        ..Default::default()
    };

    let result = run_inner(pool, http, cfg, image_dir, &mut report).await;
    match result {
        Ok(()) => {
            report.status = if report.sets_ok == report.sets_total && report.sets_total > 0 {
                "success".to_string()
            } else if report.sets_ok > 0 {
                "partial".to_string()
            } else {
                "failed".to_string()
            };
            if let Err(e) = close_run(pool, &report).await {
                tracing::error!(
                    error = %e,
                    run_id = report.run_id,
                    status = %report.status,
                    sets_total = report.sets_total,
                    sets_ok = report.sets_ok,
                    cards_seen = report.cards_seen,
                    cards_inserted = report.cards_inserted,
                    cards_updated = report.cards_updated,
                    cards_unchanged = report.cards_unchanged,
                    "failed to persist close_run — run data lost to DB but captured here"
                );
                return Err(e);
            }
        }
        Err(e) => {
            report.status = "failed".to_string();
            report.error = Some(e.to_string());
            if let Err(ce) = close_run(pool, &report).await {
                tracing::error!(
                    close_error = %ce,
                    scrape_error = %e,
                    run_id = report.run_id,
                    sets_total = report.sets_total,
                    sets_ok = report.sets_ok,
                    cards_seen = report.cards_seen,
                    cards_inserted = report.cards_inserted,
                    cards_updated = report.cards_updated,
                    cards_unchanged = report.cards_unchanged,
                    "failed to persist close_run after scrape error — run data lost to DB but captured here"
                );
                return Err(ce);
            }
            return Err(e);
        }
    }
    Ok(report)
}

async fn run_inner(
    pool: &PgPool,
    http: &HttpClient,
    _cfg: &ScrapeConfig,
    image_dir: &Path,
    report: &mut ScrapeReport,
) -> Result<()> {
    let index = http.fetch_index().await?;
    let sets = parse::parse_sets(&index)?;
    report.sets_total = sets.len() as i32;
    tracing::info!(sets = sets.len(), "discovered sets");

    for set in &sets {
        scrape_set_tracked(pool, http, image_dir, set, report).await;
    }
    Ok(())
}

/// Public entry point for scraping a single dropdown entry. Used by the CLI's `--set` flag and
/// by future targeted re-scrapes. Creates and closes its own `scrape_runs` row.
pub async fn run_one(
    pool: &PgPool,
    http: &HttpClient,
    cfg: &ScrapeConfig,
    image_dir: &Path,
    source_series: &str,
) -> Result<ScrapeReport> {
    let _ = cfg;
    fs::create_dir_all(image_dir).await?;
    let run_id = open_run(pool).await?;
    let mut report = ScrapeReport {
        run_id,
        sets_total: 1,
        ..Default::default()
    };
    let set = ParsedSet {
        source_series: source_series.to_string(),
        name: source_series.to_string(),
        display_label: source_series.to_string(),
    };
    scrape_set_tracked(pool, http, image_dir, &set, &mut report).await;
    report.status = if report.sets_ok == 1 {
        "success".to_string()
    } else {
        "failed".to_string()
    };
    close_run(pool, &report).await?;
    Ok(report)
}

async fn scrape_set_tracked(
    pool: &PgPool,
    http: &HttpClient,
    image_dir: &Path,
    set: &ParsedSet,
    report: &mut ScrapeReport,
) {
    if let Err(e) = open_run_set(pool, report.run_id, &set.source_series).await {
        tracing::error!(series = %set.source_series, error = %e, "telemetry open failed");
    }
    let before_seen = report.cards_seen;
    let before_unchanged = report.cards_unchanged;
    match scrape_one_set(pool, http, image_dir, set, report).await {
        Ok((primary_set_id, html_hash)) => {
            report.sets_ok += 1;
            let cards_this_set = report.cards_seen - before_seen;
            let unchanged_this_set = report.cards_unchanged - before_unchanged;
            if let Err(e) = close_run_set(
                pool,
                report.run_id,
                &set.source_series,
                primary_set_id.as_deref(),
                cards_this_set,
                unchanged_this_set,
                html_hash.as_deref(),
                "ok",
                None,
            )
            .await
            {
                tracing::warn!(series = %set.source_series, error = %e, "telemetry close failed");
            }
        }
        Err(e) => {
            tracing::error!(series = %set.source_series, error = %e, "set scrape failed");
            let msg = e.to_string();
            let cards_this_set = report.cards_seen - before_seen;
            let unchanged_this_set = report.cards_unchanged - before_unchanged;
            if let Err(te) = close_run_set(
                pool,
                report.run_id,
                &set.source_series,
                None,
                cards_this_set,
                unchanged_this_set,
                None,
                "failed",
                Some(&msg),
            )
            .await
            {
                tracing::warn!(series = %set.source_series, error = %te, "telemetry close failed");
            }
        }
    }
}

async fn scrape_one_set(
    pool: &PgPool,
    http: &HttpClient,
    image_dir: &Path,
    set: &ParsedSet,
    report: &mut ScrapeReport,
) -> Result<(Option<String>, Option<Vec<u8>>)> {
    let html = http.fetch_series(&set.source_series).await?;

    // Compute SHA-256 of the raw HTML to detect unchanged pages.
    let new_hash: Vec<u8> = {
        let mut h = Sha256::new();
        h.update(html.as_bytes());
        h.finalize().to_vec()
    };

    // Look up the html_hash stored on the last successful scrape_run_sets row for this
    // source_series. Keying on source_series (not set id) is correct because the hash tracks
    // whether the *page* changed, and a single page can contain cards from many sets.
    let stored_hash: Option<Vec<u8>> = sqlx::query_as(
        r#"
        SELECT srs.html_hash
        FROM scrape_run_sets srs
        JOIN scrape_runs sr ON sr.id = srs.run_id
        WHERE srs.source_series = $1
          AND srs.status = 'ok'
          AND srs.html_hash IS NOT NULL
        ORDER BY sr.id DESC
        LIMIT 1
        "#,
    )
    .bind(&set.source_series)
    .fetch_optional(pool)
    .await?
    .and_then(|(h,): (Option<Vec<u8>>,)| h);

    if stored_hash.as_deref() == Some(new_hash.as_slice()) {
        // Page is unchanged — skip all card upserts. Count the cards from the last run so the
        // parent report still reflects reality.
        let last_seen: i32 = sqlx::query_as(
            r#"
            SELECT COALESCE(srs.cards_seen, 0)
            FROM scrape_run_sets srs
            JOIN scrape_runs sr ON sr.id = srs.run_id
            WHERE srs.source_series = $1
              AND srs.status = 'ok'
            ORDER BY sr.id DESC
            LIMIT 1
            "#,
        )
        .bind(&set.source_series)
        .fetch_optional(pool)
        .await?
        .map(|(n,): (i32,)| n)
        .unwrap_or(0);

        report.cards_seen += last_seen;
        report.cards_unchanged += last_seen;

        // Derive primary set id for telemetry from the sets table.
        let primary_set_id: Option<String> = sqlx::query_as(
            "SELECT id FROM sets WHERE source_series = $1 LIMIT 1",
        )
        .bind(&set.source_series)
        .fetch_optional(pool)
        .await?
        .map(|(id,): (String,)| id);

        tracing::debug!(
            series = %set.source_series,
            cards_skipped = last_seen,
            "set HTML unchanged, skipping card upserts"
        );
        // Propagate the hash so close_run_set can persist it and future runs can skip again.
        return Ok((primary_set_id, Some(new_hash)));
    }

    let cards = parse::parse_cards(&html)?;
    if cards.is_empty() {
        tracing::warn!(series = %set.source_series, "no cards parsed for set");
        return Ok((None, None));
    }
    let primary_set_id = parse::set_id_from_code(&cards[0].code)
        .ok_or_else(|| {
            crate::Error::Parse(format!(
                "could not derive set id from first card code '{}'",
                cards[0].code
            ))
        })?
        .to_string();
    tracing::info!(set = %primary_set_id, count = cards.len(), "parsed set");
    report.cards_seen += cards.len() as i32;

    // Upsert the primary set (writes source_series on INSERT, never overwrites it).
    upsert_set(pool, &primary_set_id, set).await?;

    // For every other set id referenced by cards on this page (cross-set pages like Promotion
    // cards list cards from many different sets), just ensure the FK row exists without touching
    // the canonical source_series of those sets.
    let mut seen_set_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for c in &cards {
        if let Some(sid) = parse::set_id_from_code(&c.code) {
            seen_set_ids.insert(sid.to_string());
        }
    }
    seen_set_ids.remove(&primary_set_id);
    for sid in &seen_set_ids {
        ensure_set_fk(pool, sid, &set.source_series).await?;
    }

    for card in &cards {
        let card_set_id = parse::set_id_from_code(&card.code).unwrap_or(&primary_set_id);
        let hash = payload_hash(card);
        let outcome = upsert_card(pool, card_set_id, card, &hash).await?;
        match outcome {
            UpsertOutcome::Inserted => report.cards_inserted += 1,
            UpsertOutcome::Updated => report.cards_updated += 1,
            UpsertOutcome::Unchanged => report.cards_unchanged += 1,
        }

        let local_image = image_path(image_dir, card);
        if !local_image.exists() {
            let raw_url = format!(
                "../images/cardlist/card/{}{}",
                card.image_filename,
                if card.image_version.is_empty() {
                    String::new()
                } else {
                    format!("?{}", card.image_version)
                }
            );
            match http.fetch_image(&raw_url).await {
                Ok(bytes) => {
                    if let Err(e) = write_atomic(&local_image, &bytes).await {
                        tracing::warn!(card = %card.code, error = %e, "image write failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(card = %card.code, error = %e, "image download failed");
                }
            }
        }
    }

    Ok((Some(primary_set_id), Some(new_hash)))
}

fn image_path(image_dir: &Path, card: &ParsedCard) -> PathBuf {
    let stem = card.image_filename.trim_end_matches(".png");
    let suffix = if card.image_version.is_empty() {
        format!("{stem}.png")
    } else {
        format!("{}.{}.png", stem, card.image_version)
    };
    image_dir.join(suffix)
}

async fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut tmp = target.to_path_buf();
    tmp.set_extension("png.tmp");
    {
        let mut f = fs::File::create(&tmp).await?;
        f.write_all(bytes).await?;
        f.flush().await?;
    }
    fs::rename(&tmp, target).await?;
    Ok(())
}

fn payload_hash(card: &ParsedCard) -> Vec<u8> {
    let mut hasher = Sha256::new();
    let canonical = serde_json::json!({
        "code": card.code,
        "name": card.name,
        "rarity": card.rarity,
        "category": card.category,
        "color": card.color,
        "colors": card.colors,
        "cost": card.cost,
        "life": card.life,
        "power": card.power,
        "counter": card.counter,
        "attribute": card.attribute,
        "block": card.block,
        "card_type": card.card_type,
        "features": card.features,
        "effect_text": card.effect_text,
        "trigger_text": card.trigger_text,
        "notes": card.notes,
        "image_filename": card.image_filename,
        "image_version": card.image_version,
    });
    hasher.update(canonical.to_string().as_bytes());
    hasher.finalize().to_vec()
}

enum UpsertOutcome {
    Inserted,
    Updated,
    Unchanged,
}

async fn upsert_card(
    pool: &PgPool,
    set_id: &str,
    card: &ParsedCard,
    payload_hash: &[u8],
) -> Result<UpsertOutcome> {
    let returned: Option<(bool,)> = sqlx::query_as(
        r#"
        INSERT INTO cards (
            code, set_id, name, rarity, category, color, colors,
            cost, life, power, counter, attribute, block, card_type, features,
            effect_text, trigger_text, notes, image_path, image_version, payload_hash
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)
        ON CONFLICT (code) DO UPDATE SET
            set_id        = EXCLUDED.set_id,
            name          = EXCLUDED.name,
            rarity        = EXCLUDED.rarity,
            category      = EXCLUDED.category,
            color         = EXCLUDED.color,
            colors        = EXCLUDED.colors,
            cost          = EXCLUDED.cost,
            life          = EXCLUDED.life,
            power         = EXCLUDED.power,
            counter       = EXCLUDED.counter,
            attribute     = EXCLUDED.attribute,
            block         = EXCLUDED.block,
            card_type     = EXCLUDED.card_type,
            features      = EXCLUDED.features,
            effect_text   = EXCLUDED.effect_text,
            trigger_text  = EXCLUDED.trigger_text,
            notes         = EXCLUDED.notes,
            image_path    = EXCLUDED.image_path,
            image_version = EXCLUDED.image_version,
            payload_hash  = EXCLUDED.payload_hash,
            updated_at    = now()
        WHERE cards.payload_hash IS DISTINCT FROM EXCLUDED.payload_hash
           OR cards.image_version IS DISTINCT FROM EXCLUDED.image_version
        RETURNING (xmax = 0) AS inserted
        "#,
    )
    .bind(&card.code)
    .bind(set_id)
    .bind(&card.name)
    .bind(&card.rarity)
    .bind(&card.category)
    .bind(&card.color)
    .bind(&card.colors)
    .bind(card.cost)
    .bind(card.life)
    .bind(card.power)
    .bind(card.counter)
    .bind(card.attribute.as_deref())
    .bind(card.block)
    .bind(card.card_type.as_deref())
    .bind(&card.features)
    .bind(card.effect_text.as_deref())
    .bind(card.trigger_text.as_deref())
    .bind(card.notes.as_deref())
    .bind(&card.image_filename)
    .bind(&card.image_version)
    .bind(payload_hash)
    .fetch_optional(pool)
    .await?;

    Ok(match returned {
        Some((true,)) => UpsertOutcome::Inserted,
        Some((false,)) => UpsertOutcome::Updated,
        None => UpsertOutcome::Unchanged,
    })
}

/// Upsert the **primary** set for a source_series page.
/// `source_series` is written only on INSERT and never overwritten — this prevents cross-set
/// pages (e.g. Promotion cards) from clobbering the canonical source_series of existing sets.
async fn upsert_set(pool: &PgPool, id: &str, set: &ParsedSet) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sets (id, source_series, name, display_label)
        VALUES ($1,$2,$3,$4)
        ON CONFLICT (id) DO UPDATE SET
            name          = EXCLUDED.name,
            display_label = EXCLUDED.display_label,
            updated_at    = now()
        "#,
    )
    .bind(id)
    .bind(&set.source_series)
    .bind(&set.name)
    .bind(&set.display_label)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ensure a set row exists for a secondary set referenced by cards on a cross-set page.
/// Uses DO NOTHING so that it never overwrites the canonical source_series of an existing set.
async fn ensure_set_fk(pool: &PgPool, id: &str, placeholder_source_series: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sets (id, source_series, name, display_label)
        VALUES ($1,$2,$3,$3)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(placeholder_source_series)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn open_run(pool: &PgPool) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO scrape_runs (status, started_at)
        VALUES ('running', $1)
        RETURNING id
        "#,
    )
    .bind(Utc::now())
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

async fn close_run(pool: &PgPool, report: &ScrapeReport) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE scrape_runs
        SET finished_at     = now(),
            status          = $2,
            sets_total      = $3,
            sets_ok         = $4,
            cards_seen      = $5,
            cards_inserted  = $6,
            cards_updated   = $7,
            cards_unchanged = $8,
            error           = $9
        WHERE id = $1
        "#,
    )
    .bind(report.run_id)
    .bind(&report.status)
    .bind(report.sets_total)
    .bind(report.sets_ok)
    .bind(report.cards_seen)
    .bind(report.cards_inserted)
    .bind(report.cards_updated)
    .bind(report.cards_unchanged)
    .bind(report.error.as_deref())
    .execute(pool)
    .await?;
    Ok(())
}

async fn open_run_set(pool: &PgPool, run_id: i64, source_series: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO scrape_run_sets (run_id, source_series, status)
        VALUES ($1, $2, 'running')
        ON CONFLICT (run_id, source_series) DO UPDATE SET
            status          = EXCLUDED.status,
            started_at      = now(),
            finished_at     = NULL,
            error           = NULL,
            html_hash       = NULL,
            cards_seen      = 0,
            cards_unchanged = 0,
            set_id          = NULL
        "#,
    )
    .bind(run_id)
    .bind(source_series)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn close_run_set(
    pool: &PgPool,
    run_id: i64,
    source_series: &str,
    set_id: Option<&str>,
    cards_seen: i32,
    cards_unchanged: i32,
    html_hash: Option<&[u8]>,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE scrape_run_sets
        SET finished_at     = now(),
            set_id          = $3,
            cards_seen      = $4,
            cards_unchanged = $5,
            html_hash       = $6,
            status          = $7,
            error           = $8
        WHERE run_id = $1 AND source_series = $2
        "#,
    )
    .bind(run_id)
    .bind(source_series)
    .bind(set_id)
    .bind(cards_seen)
    .bind(cards_unchanged)
    .bind(html_hash)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
