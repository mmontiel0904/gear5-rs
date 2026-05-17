use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use gear5_core::model::Card;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CardFilter {
    /// Filter to a specific set, e.g. `OP01`, `EB04`.
    pub set: Option<String>,
    /// Match any card whose `colors` array contains this value (`Red`, `Green`, …).
    pub color: Option<String>,
    /// LEADER / CHARACTER / EVENT / STAGE / DON.
    pub category: Option<String>,
    /// L / C / UC / R / SR / SEC.
    pub rarity: Option<String>,
    /// Match any card whose `features` array contains this value, e.g. `Straw Hat Crew`.
    pub feature: Option<String>,
    pub cost: Option<i32>,
    pub power_min: Option<i32>,
    pub power_max: Option<i32>,
    /// Case-insensitive substring search across `name` and `effect_text`.
    pub q: Option<String>,
    /// Opaque pagination cursor (the `next_cursor` from the previous page).
    pub cursor: Option<String>,
    /// Page size. Clamped to `[1, 200]`.
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CardView {
    #[serde(flatten)]
    pub card: Card,
    /// Server-relative URL for the cached card art.
    pub image_url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CardListResponse {
    pub items: Vec<CardView>,
    /// Pass this back as `cursor=` to fetch the next page. `null` when the page is the last.
    pub next_cursor: Option<String>,
    pub limit: i64,
}

fn image_url(image_path: &str, image_version: &str) -> String {
    let stem = image_path.trim_end_matches(".png");
    if image_version.is_empty() {
        format!("/images/{}.png", stem)
    } else {
        format!("/images/{}.{}.png", stem, image_version)
    }
}

#[utoipa::path(
    get,
    path = "/cards/{code}",
    tag = "catalog",
    security(("BearerAuth" = [])),
    params(
        ("code" = String, Path, description = "Card code, e.g. `OP01-001` or `OP01-003_p1` for alt arts"),
    ),
    responses(
        (status = 200, body = CardView),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn get_card(
    State(s): State<AppState>,
    _: ReadAuth,
    Path(code): Path<String>,
) -> Result<Json<CardView>, ApiError> {
    let card: Option<Card> = sqlx::query_as(
        r#"
        SELECT code, set_id, name, rarity, category, color, colors,
               cost, life, power, counter, attribute, block, card_type, features,
               effect_text, trigger_text, notes, image_path, image_version,
               payload_hash, first_seen_at, updated_at
        FROM cards WHERE code = $1
        "#,
    )
    .bind(&code)
    .fetch_optional(&s.pool)
    .await?;
    let card = card.ok_or_else(ApiError::not_found)?;
    let url = image_url(&card.image_path, &card.image_version);
    Ok(Json(CardView {
        card,
        image_url: url,
    }))
}

#[utoipa::path(
    get,
    path = "/cards",
    tag = "catalog",
    security(("BearerAuth" = [])),
    params(CardFilter),
    responses(
        (status = 200, body = CardListResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn list_cards(
    State(s): State<AppState>,
    _: ReadAuth,
    Query(f): Query<CardFilter>,
) -> Result<Json<CardListResponse>, ApiError> {
    let limit = f.limit.unwrap_or(50).clamp(1, 200);

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        SELECT code, set_id, name, rarity, category, color, colors,
               cost, life, power, counter, attribute, block, card_type, features,
               effect_text, trigger_text, notes, image_path, image_version,
               payload_hash, first_seen_at, updated_at
        FROM cards
        WHERE 1=1
        "#,
    );

    if let Some(set) = &f.set {
        qb.push(" AND set_id = ").push_bind(set.clone());
    }
    if let Some(color) = &f.color {
        qb.push(" AND ")
            .push_bind(color.clone())
            .push(" = ANY(colors)");
    }
    if let Some(category) = &f.category {
        qb.push(" AND category = ").push_bind(category.clone());
    }
    if let Some(rarity) = &f.rarity {
        qb.push(" AND rarity = ").push_bind(rarity.clone());
    }
    if let Some(feature) = &f.feature {
        qb.push(" AND ")
            .push_bind(feature.clone())
            .push(" = ANY(features)");
    }
    if let Some(cost) = f.cost {
        qb.push(" AND cost = ").push_bind(cost);
    }
    if let Some(pmin) = f.power_min {
        qb.push(" AND power >= ").push_bind(pmin);
    }
    if let Some(pmax) = f.power_max {
        qb.push(" AND power <= ").push_bind(pmax);
    }
    if let Some(q) = f.q.as_ref().filter(|s| !s.trim().is_empty()) {
        let needle = format!("%{}%", q.trim().to_lowercase());
        qb.push(" AND (lower(name) LIKE ").push_bind(needle.clone());
        qb.push(" OR lower(coalesce(effect_text,'')) LIKE ")
            .push_bind(needle)
            .push(")");
    }
    if let Some(cursor) = &f.cursor {
        qb.push(" AND code > ").push_bind(cursor.clone());
    }

    qb.push(" ORDER BY code ASC LIMIT ").push_bind(limit + 1);

    let mut rows: Vec<Card> = qb.build_query_as().fetch_all(&s.pool).await?;
    let next_cursor = if rows.len() as i64 > limit {
        rows.pop().map(|c| c.code)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|c| {
            let url = image_url(&c.image_path, &c.image_version);
            CardView {
                card: c,
                image_url: url,
            }
        })
        .collect();

    Ok(Json(CardListResponse {
        items,
        next_cursor,
        limit,
    }))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParams {
    /// Free-text query, matched against card name (prefix first, trigram fuzzy fallback).
    /// Queries shorter than 2 characters return an empty list.
    pub q: String,
    /// Result cap. Clamped to `[1, 25]`. Defaults to 10.
    #[param(minimum = 1, maximum = 25)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CardSuggestion {
    pub code: String,
    pub set_id: String,
    pub name: String,
    pub rarity: String,
    pub color: String,
    /// Server-relative URL for the cached card art.
    pub image_url: String,
}

#[derive(FromRow)]
struct CardSuggestionRow {
    code: String,
    set_id: String,
    name: String,
    rarity: String,
    color: String,
    image_path: String,
    image_version: String,
}

impl From<CardSuggestionRow> for CardSuggestion {
    fn from(r: CardSuggestionRow) -> Self {
        let image_url = image_url(&r.image_path, &r.image_version);
        Self {
            code: r.code,
            set_id: r.set_id,
            name: r.name,
            rarity: r.rarity,
            color: r.color,
            image_url,
        }
    }
}

const SEARCH_DEFAULT_LIMIT: i64 = 10;
const SEARCH_MAX_LIMIT: i64 = 25;
const SEARCH_MIN_CHARS: usize = 2;

#[utoipa::path(
    get,
    path = "/cards/search",
    tag = "catalog",
    security(("BearerAuth" = [])),
    params(SearchParams),
    responses(
        (status = 200, body = Vec<CardSuggestion>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn search_cards(
    State(s): State<AppState>,
    _: ReadAuth,
    Query(p): Query<SearchParams>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = p.limit.unwrap_or(SEARCH_DEFAULT_LIMIT).clamp(1, SEARCH_MAX_LIMIT);

    let needle = p.q.trim().to_lowercase();
    if needle.chars().count() < SEARCH_MIN_CHARS {
        return Ok(suggestion_response(Arc::new(Vec::new())));
    }

    if let Some(cached) = s.search_cache.get(&needle, limit) {
        return Ok(suggestion_response(cached));
    }

    let suggestions = Arc::new(query_suggestions(&s.pool, &needle, limit).await?);
    s.search_cache.insert(&needle, limit, suggestions.clone());

    Ok(suggestion_response(suggestions))
}

/// SQL-only path for `/cards/search`: prefix match first, trigram-similarity
/// fallback for typos. Caller is responsible for normalizing `needle`
/// (lowercase; the `cards.name_norm` generated column already strips accents).
pub(crate) async fn query_suggestions(
    pool: &sqlx::PgPool,
    needle: &str,
    limit: i64,
) -> Result<Vec<CardSuggestion>, sqlx::Error> {
    let rows: Vec<CardSuggestionRow> = sqlx::query_as(
        r#"
        WITH prefix AS (
            SELECT code, set_id, name, rarity, color, image_path, image_version, 0::int AS rk
            FROM cards
            WHERE name_norm LIKE $1 || '%'
            ORDER BY name_norm
            LIMIT $2
        ),
        fuzzy AS (
            SELECT code, set_id, name, rarity, color, image_path, image_version, 1::int AS rk
            FROM cards
            WHERE name_norm % $1
              AND code NOT IN (SELECT code FROM prefix)
            ORDER BY similarity(name_norm, $1) DESC
            LIMIT $2
        )
        SELECT code, set_id, name, rarity, color, image_path, image_version
        FROM (
            SELECT * FROM prefix
            UNION ALL
            SELECT * FROM fuzzy
        ) merged
        ORDER BY rk, name
        LIMIT $2
        "#,
    )
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(CardSuggestion::from).collect())
}

fn suggestion_response(suggestions: Arc<Vec<CardSuggestion>>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json((*suggestions).clone()),
    )
}
