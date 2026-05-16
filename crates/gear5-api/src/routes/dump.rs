use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use gear5_core::model::Card;
use sqlx::Row;
use std::io;
use std::pin::Pin;

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

    // Stream rows out as NDJSON. Constant memory regardless of catalog size or how many
    // concurrent dump callers are connected. The explicit type pin nails down the error
    // channel so `async_stream::try_stream!` can pick the right `From<io::Error>` impl.
    let pool = s.pool.clone();
    let row_stream: Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> =
        Box::pin(async_stream::try_stream! {
            let mut cards = sqlx::query_as::<_, Card>(
                r#"
                SELECT code, set_id, name, rarity, category, color, colors,
                       cost, life, power, counter, attribute, block, card_type, features,
                       effect_text, trigger_text, notes, image_path, image_version,
                       payload_hash, first_seen_at, updated_at
                FROM cards
                ORDER BY code ASC
                "#,
            )
            .fetch(&pool);

            while let Some(card) = cards
                .try_next()
                .await
                .map_err(|e| io::Error::other(format!("db: {e}")))?
            {
                let mut line = serde_json::to_vec(&card)
                    .map_err(|e| io::Error::other(format!("json: {e}")))?;
                line.push(b'\n');
                yield Bytes::from(line);
            }
        });

    let body = Body::from_stream(row_stream);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .header(header::VARY, "Authorization");
    if let Some(server_etag) = etag {
        builder = builder.header(header::ETAG, format!("\"{server_etag}\""));
    }
    builder
        .body(body)
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
