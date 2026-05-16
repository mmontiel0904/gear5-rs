use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use gear5_core::auth::{create_key, list_keys, revoke_key, NewKeyInput};
use gear5_core::config::Config;
use gear5_core::db;

#[derive(Subcommand, Debug)]
pub enum KeyCmd {
    /// Create a new API key. Prints the plaintext key ONCE.
    Create {
        #[arg(long)]
        name: String,
        /// Comma-separated scopes (defaults to "read"). Valid: read, admin.
        #[arg(long, default_value = "read")]
        scopes: String,
        /// Requests per minute allowed for this key.
        #[arg(long, default_value_t = 120)]
        rate: i32,
        /// RFC3339 expiration timestamp, e.g. 2026-12-31T00:00:00Z.
        #[arg(long)]
        expires: Option<String>,
    },
    /// List existing keys.
    List,
    /// Revoke a key by UUID or 16-hex prefix.
    Revoke { id_or_prefix: String },
    /// Revoke a key and issue a replacement with the same name/scopes/rate.
    Rotate { id_or_prefix: String },
}

pub async fn run(cmd: KeyCmd) -> Result<()> {
    let cfg = Config::load().context("load config")?;
    let pool = db::connect(&cfg.database).await?;
    db::migrate(&pool).await?;

    match cmd {
        KeyCmd::Create {
            name,
            scopes,
            rate,
            expires,
        } => {
            let scopes: Vec<String> = scopes
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let expires_at: Option<DateTime<Utc>> = expires
                .as_deref()
                .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
                .transpose()
                .context("parse --expires")?;
            let generated = create_key(
                &pool,
                NewKeyInput {
                    name,
                    scopes,
                    rate_limit_rpm: rate.max(1),
                    expires_at,
                },
            )
            .await?;
            println!("# Save this key now. It will not be shown again.");
            println!("id      = {}", generated.record.id);
            println!("name    = {}", generated.record.name);
            println!("prefix  = {}", generated.record.prefix);
            println!("scopes  = {}", generated.record.scopes.join(","));
            println!("rate    = {} rpm", generated.record.rate_limit_rpm);
            if let Some(t) = generated.record.expires_at {
                println!("expires = {}", t.to_rfc3339());
            }
            println!("key     = {}", generated.plaintext);
        }
        KeyCmd::List => {
            let keys = list_keys(&pool).await?;
            println!(
                "{:<36}  {:<20}  {:<16}  {:<10}  {:>4}  {:<25}  {:<25}",
                "id", "name", "prefix", "scopes", "rpm", "last_used_at", "expires/revoked"
            );
            for k in keys {
                let suffix = if let Some(r) = k.revoked_at {
                    format!("revoked {}", r.format("%Y-%m-%dT%H:%M:%SZ"))
                } else if let Some(e) = k.expires_at {
                    format!("expires {}", e.format("%Y-%m-%dT%H:%M:%SZ"))
                } else {
                    String::new()
                };
                let last_used = k
                    .last_used_at
                    .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                println!(
                    "{:<36}  {:<20}  {:<16}  {:<10}  {:>4}  {:<25}  {:<25}",
                    k.id,
                    truncate(&k.name, 20),
                    k.prefix,
                    k.scopes.join(","),
                    k.rate_limit_rpm,
                    last_used,
                    suffix,
                );
            }
        }
        KeyCmd::Revoke { id_or_prefix } => {
            let row = revoke_key(&pool, &id_or_prefix).await?;
            println!(
                "revoked id={} prefix={} at={}",
                row.id,
                row.prefix,
                row.revoked_at.unwrap_or_else(Utc::now).to_rfc3339()
            );
        }
        KeyCmd::Rotate { id_or_prefix } => {
            let keys = list_keys(&pool).await?;
            let old = keys
                .into_iter()
                .find(|k| k.id.to_string() == id_or_prefix || k.prefix == id_or_prefix)
                .context("no key matched id-or-prefix")?;
            if old.revoked_at.is_none() {
                revoke_key(&pool, &id_or_prefix).await?;
            }
            let generated = create_key(
                &pool,
                NewKeyInput {
                    name: old.name.clone(),
                    scopes: old.scopes.clone(),
                    rate_limit_rpm: old.rate_limit_rpm,
                    expires_at: old.expires_at,
                },
            )
            .await?;
            println!("# Replacement key. Save it now.");
            println!("old_id  = {}", old.id);
            println!("new_id  = {}", generated.record.id);
            println!("prefix  = {}", generated.record.prefix);
            println!("key     = {}", generated.plaintext);
        }
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(n.saturating_sub(1)).collect();
        o.push('…');
        o
    }
}
