use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use gear5_core::model::Card;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};

#[derive(Debug, Deserialize)]
pub struct CardFilter {
    pub set: Option<String>,
    pub color: Option<String>,
    pub category: Option<String>,
    pub rarity: Option<String>,
    pub feature: Option<String>,
    pub cost: Option<i32>,
    pub power_min: Option<i32>,
    pub power_max: Option<i32>,
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CardView {
    #[serde(flatten)]
    pub card: Card,
    pub image_url: String,
}

#[derive(Debug, Serialize)]
pub struct CardListResponse {
    pub items: Vec<CardView>,
    pub next_cursor: Option<String>,
    pub limit: i64,
}

fn image_url(card: &Card) -> String {
    let stem = card.image_path.trim_end_matches(".png");
    if card.image_version.is_empty() {
        format!("/images/{}.png", stem)
    } else {
        format!("/images/{}.{}.png", stem, card.image_version)
    }
}

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
    let url = image_url(&card);
    Ok(Json(CardView {
        card,
        image_url: url,
    }))
}

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
            let url = image_url(&c);
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
