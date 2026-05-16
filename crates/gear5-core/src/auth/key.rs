use crate::model::ApiKey;
use crate::{Error, Result};
use argon2::password_hash::{rand_core::OsRng as PhOsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base32::Alphabet;
use chrono::{DateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

pub const KEY_VISIBLE_PREFIX: &str = "op_live_";
const KEY_RANDOM_BYTES: usize = 32;
const PREFIX_HEX_LEN: usize = 16;

pub fn key_visible_prefix() -> &'static str {
    KEY_VISIBLE_PREFIX
}

pub struct NewKeyInput {
    pub name: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct GeneratedKey {
    pub record: ApiKey,
    /// Plaintext key. Shown to the operator ONCE and not stored.
    pub plaintext: String,
}

fn generate_random_bytes() -> [u8; KEY_RANDOM_BYTES] {
    let mut buf = [0u8; KEY_RANDOM_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

fn encode_visible(bytes: &[u8]) -> String {
    let body = base32::encode(Alphabet::Rfc4648 { padding: false }, bytes);
    format!("{KEY_VISIBLE_PREFIX}{body}")
}

pub fn lookup_prefix_for(plaintext: &str) -> String {
    let digest = Sha256::digest(plaintext.as_bytes());
    let mut hex = hex::encode(digest);
    hex.truncate(PREFIX_HEX_LEN);
    hex
}

fn hash_secret(plaintext: &str) -> Result<String> {
    let salt = SaltString::generate(&mut PhOsRng);
    let phc = Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)?
        .to_string();
    Ok(phc)
}

pub fn verify_secret(plaintext: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(plaintext.as_bytes(), &parsed)
        .is_ok()
}

pub async fn create_key(pool: &PgPool, input: NewKeyInput) -> Result<GeneratedKey> {
    let raw = generate_random_bytes();
    let plaintext = encode_visible(&raw);
    let prefix = lookup_prefix_for(&plaintext);
    let phc = hash_secret(&plaintext)?;

    let record: ApiKey = sqlx::query_as(
        r#"
        INSERT INTO api_keys (name, prefix, hash, scopes, rate_limit_rpm, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, name, prefix, hash, scopes, rate_limit_rpm,
                  created_at, last_used_at, expires_at, revoked_at
        "#,
    )
    .bind(&input.name)
    .bind(&prefix)
    .bind(&phc)
    .bind(&input.scopes)
    .bind(input.rate_limit_rpm)
    .bind(input.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(GeneratedKey { record, plaintext })
}

pub async fn list_keys(pool: &PgPool) -> Result<Vec<ApiKey>> {
    let rows: Vec<ApiKey> = sqlx::query_as(
        r#"
        SELECT id, name, prefix, hash, scopes, rate_limit_rpm,
               created_at, last_used_at, expires_at, revoked_at
        FROM api_keys
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn revoke_key(pool: &PgPool, id_or_prefix: &str) -> Result<ApiKey> {
    let parsed_uuid = Uuid::parse_str(id_or_prefix).ok();
    let row: Option<ApiKey> = sqlx::query_as(
        r#"
        UPDATE api_keys
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE (id = $1) OR (prefix = $2)
        RETURNING id, name, prefix, hash, scopes, rate_limit_rpm,
                  created_at, last_used_at, expires_at, revoked_at
        "#,
    )
    .bind(parsed_uuid)
    .bind(id_or_prefix)
    .fetch_optional(pool)
    .await?;
    row.ok_or(Error::NotFound)
}

/// DB-only step of bearer-token verification.
///
/// Returns the active `api_keys` row whose lookup prefix matches `plaintext`. Does NOT run
/// argon2 — callers (typically the API auth middleware) are expected to do that on a
/// blocking thread, since argon2id with default params burns ~50–100 ms of CPU per check.
///
/// Returns `Error::InvalidApiKey` if no matching active row exists.
pub async fn lookup_active_row_by_prefix(pool: &PgPool, plaintext: &str) -> Result<ApiKey> {
    if !plaintext.starts_with(KEY_VISIBLE_PREFIX) {
        return Err(Error::InvalidApiKey);
    }
    let prefix = lookup_prefix_for(plaintext);
    let row: Option<ApiKey> = sqlx::query_as(
        r#"
        SELECT id, name, prefix, hash, scopes, rate_limit_rpm,
               created_at, last_used_at, expires_at, revoked_at
        FROM api_keys
        WHERE prefix = $1
          AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
        "#,
    )
    .bind(&prefix)
    .fetch_optional(pool)
    .await?;
    row.ok_or(Error::InvalidApiKey)
}

/// Convenience: prefix lookup + sync argon2 verify in one call. Used by the CLI and tests.
/// Production API code prefers `lookup_active_row_by_prefix` + a `spawn_blocking` verify.
pub async fn lookup_for_verify(pool: &PgPool, plaintext: &str) -> Result<ApiKey> {
    let row = lookup_active_row_by_prefix(pool, plaintext).await?;
    if !verify_secret(plaintext, &row.hash) {
        return Err(Error::InvalidApiKey);
    }
    Ok(row)
}

pub async fn mark_used(pool: &PgPool, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
