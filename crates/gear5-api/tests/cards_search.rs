//! Integration tests for the `/cards/search` SQL path against a real Postgres.
//!
//! Each `#[sqlx::test]` runs the embedded migration suite (including
//! `20260517000001_card_search.sql`) on a fresh database, so the indexes and
//! generated `name_norm` column are exercised exactly as in production.
//!
//! Requires `DATABASE_URL` to point at a Postgres server with `CREATE DATABASE`
//! permissions. The binary crate does not expose its modules to integration
//! tests, so the relevant modules are re-included via `#[path = ...]`.

#![allow(dead_code, unused_imports)]

#[path = "../src/auth_cache.rs"]
mod auth_cache;
#[path = "../src/middleware/mod.rs"]
mod middleware;
#[path = "../src/openapi/mod.rs"]
mod openapi;
#[path = "../src/routes/mod.rs"]
mod routes;
#[path = "../src/search_cache.rs"]
mod search_cache;
#[path = "../src/state.rs"]
mod state;

use routes::cards::{query_suggestions, CardSuggestion};
use sqlx::PgPool;

async fn seed_set(pool: &PgPool, id: &str, source_series: &str) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sets (id, source_series, name, display_label)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(source_series)
    .bind(format!("Set {id}"))
    .bind(format!("[{id}]"))
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_card(
    pool: &PgPool,
    code: &str,
    set_id: &str,
    name: &str,
    rarity: &str,
    color: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO cards (
            code, set_id, name, rarity, category, color, colors,
            image_path, image_version, payload_hash
        )
        VALUES ($1, $2, $3, $4, 'CHARACTER', $5, ARRAY[$5]::text[],
                $6, '1', E'\\x00')
        "#,
    )
    .bind(code)
    .bind(set_id)
    .bind(name)
    .bind(rarity)
    .bind(color)
    .bind(format!("{code}.png"))
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_fixture(pool: &PgPool) -> sqlx::Result<()> {
    seed_set(pool, "OP01", "569101").await?;
    seed_card(pool, "OP01-001", "OP01", "Monkey D. Luffy", "L", "Red").await?;
    seed_card(pool, "OP01-002", "OP01", "Roronoa Zoro", "SR", "Green").await?;
    seed_card(pool, "OP01-013", "OP01", "Lucci", "C", "Black").await?;
    seed_card(pool, "OP01-016", "OP01", "Nami", "R", "Blue").await?;
    seed_card(pool, "OP01-025", "OP01", "Lily", "C", "Red").await?;
    seed_card(pool, "OP01-026", "OP01", "Lola", "C", "Red").await?;
    // Diacritic-bearing name to exercise the unaccent path.
    seed_card(pool, "OP01-099", "OP01", "Pokémon Trainer", "C", "Yellow").await?;
    Ok(())
}

fn names(rows: &[CardSuggestion]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn prefix_match_returns_all_matching_cards(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "lu", 10).await?;

    let got = names(&rows);
    assert!(
        got.contains(&"Lucci") && got.contains(&"Monkey D. Luffy"),
        "expected Lucci and Luffy on prefix `lu`, got {got:?}",
    );
    assert!(
        !got.contains(&"Nami"),
        "Nami should not match prefix `lu`, got {got:?}",
    );
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn prefix_match_is_case_and_accent_insensitive(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "pokemon", 5).await?;

    assert_eq!(names(&rows), vec!["Pokémon Trainer"]);
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn fuzzy_fallback_catches_typo(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    // `lffy` has no prefix match anywhere in the corpus but is trigram-similar
    // to `luffy`, so the fuzzy CTE should rescue the result.
    let rows = query_suggestions(&pool, "lffy", 10).await?;

    assert!(
        names(&rows).contains(&"Monkey D. Luffy"),
        "expected fuzzy fallback to surface Luffy, got {:?}",
        names(&rows),
    );
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn limit_clamps_result_count(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "l", 2).await?;

    assert_eq!(rows.len(), 2, "limit=2 must cap output, got {rows:?}");
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn no_matches_returns_empty(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "zzzzzz", 10).await?;

    assert!(rows.is_empty(), "expected empty, got {rows:?}");
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn image_url_is_versioned(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "nami", 5).await?;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].image_url, "/images/OP01-016.1.png");
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn payload_is_slim(pool: PgPool) -> sqlx::Result<()> {
    seed_fixture(&pool).await?;

    let rows = query_suggestions(&pool, "lu", 10).await?;
    let row = rows
        .iter()
        .find(|r| r.code == "OP01-001")
        .expect("Luffy in results");

    // Serialised shape: exactly the slim+rarity+color contract, nothing else.
    let json = serde_json::to_value(row).expect("serialises");
    let obj = json.as_object().expect("object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["code", "color", "image_url", "name", "rarity", "set_id"],
    );
    Ok(())
}
