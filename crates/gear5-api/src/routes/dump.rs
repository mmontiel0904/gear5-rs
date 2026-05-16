use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use gear5_core::model::Card;
use sqlx::Row;

pub async fn dump(
    State(s): State<AppState>,
    _: ReadAuth,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let etag = current_etag(&s.pool).await?;

    if let Some(client_etag) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(server_etag) = &etag {
            if client_etag.trim_matches('"') == server_etag {
                let mut resp = Response::new(Body::empty());
                *resp.status_mut() = StatusCode::NOT_MODIFIED;
                resp.headers_mut()
                    .insert(header::ETAG, format!("\"{server_etag}\"").parse().unwrap());
                return Ok(resp);
            }
        }
    }

    let cards: Vec<Card> = sqlx::query_as(
        r#"
        SELECT code, set_id, name, rarity, category, color, colors,
               cost, life, power, counter, attribute, block, card_type, features,
               effect_text, trigger_text, notes, image_path, image_version,
               payload_hash, first_seen_at, updated_at
        FROM cards
        ORDER BY code ASC
        "#,
    )
    .fetch_all(&s.pool)
    .await?;

    let mut buf: Vec<u8> = Vec::with_capacity(cards.len() * 1024);
    for c in &cards {
        let line = serde_json::to_vec(c).map_err(|e| ApiError::internal(e.to_string()))?;
        buf.extend_from_slice(&line);
        buf.push(b'\n');
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson");
    if let Some(server_etag) = etag {
        builder = builder.header(header::ETAG, format!("\"{server_etag}\""));
    }
    builder
        .body(Body::from(buf))
        .map_err(|e| ApiError::internal(e.to_string()))
}

async fn current_etag(pool: &sqlx::PgPool) -> Result<Option<String>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT finished_at
        FROM scrape_runs
        WHERE status = 'success'
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row
        .and_then(|r| {
            r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at")
                .ok()
                .flatten()
        })
        .map(|t| t.timestamp_millis().to_string()))
}
