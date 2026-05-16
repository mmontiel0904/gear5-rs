use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CardSet {
    pub id: String,
    pub source_series: String,
    pub name: String,
    pub display_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Card {
    pub code: String,
    pub set_id: String,
    pub name: String,
    pub rarity: String,
    pub category: String,
    pub color: String,
    pub colors: Vec<String>,
    pub cost: Option<i32>,
    pub life: Option<i32>,
    pub power: Option<i32>,
    pub counter: Option<i32>,
    pub attribute: Option<String>,
    pub block: Option<i32>,
    pub card_type: Option<String>,
    pub features: Vec<String>,
    pub effect_text: Option<String>,
    pub trigger_text: Option<String>,
    pub notes: Option<String>,
    pub image_path: String,
    pub image_version: String,
    #[serde(skip)]
    pub payload_hash: Vec<u8>,
    pub first_seen_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ScrapeRun {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub sets_total: Option<i32>,
    pub sets_ok: Option<i32>,
    pub cards_seen: Option<i32>,
    pub cards_inserted: Option<i32>,
    pub cards_updated: Option<i32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    #[serde(skip)]
    pub hash: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// What scraping produces for a single card on a set page.
#[derive(Debug, Clone)]
pub struct ParsedCard {
    pub code: String,
    pub name: String,
    pub rarity: String,
    pub category: String,
    pub color: String,
    pub colors: Vec<String>,
    pub cost: Option<i32>,
    pub life: Option<i32>,
    pub power: Option<i32>,
    pub counter: Option<i32>,
    pub attribute: Option<String>,
    pub block: Option<i32>,
    pub card_type: Option<String>,
    pub features: Vec<String>,
    pub effect_text: Option<String>,
    pub trigger_text: Option<String>,
    pub notes: Option<String>,
    pub image_filename: String,
    pub image_version: String,
}

/// What scraping produces for one option in the sets dropdown.
/// Note: `id` is intentionally absent — the dropdown label format does not match the card-code prefix
/// (e.g. label `[OP-01]` vs. card codes `OP01-001`). The set id is derived later from the cards
/// returned for each page, so a single source-series can map cleanly to its actual code prefix.
#[derive(Debug, Clone)]
pub struct ParsedSet {
    pub source_series: String,
    pub name: String,
    pub display_label: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScrapeReport {
    pub run_id: i64,
    pub sets_total: i32,
    pub sets_ok: i32,
    pub cards_seen: i32,
    pub cards_inserted: i32,
    pub cards_updated: i32,
    pub status: String,
    pub error: Option<String>,
}
