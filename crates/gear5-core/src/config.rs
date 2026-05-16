use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub images: ImagesConfig,
    pub scrape: ScrapeConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8080".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_statement_timeout_ms")]
    pub statement_timeout_ms: i64,
    #[serde(default = "default_idle_tx_timeout_ms")]
    pub idle_tx_timeout_ms: i64,
}

fn default_max_connections() -> u32 {
    16
}

fn default_statement_timeout_ms() -> i64 {
    10_000
}

fn default_idle_tx_timeout_ms() -> i64 {
    30_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImagesConfig {
    pub dir: PathBuf,
}

impl Default for ImagesConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("./var/images"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScrapeConfig {
    pub enabled: bool,
    pub run_at_startup: bool,
    pub cron_hour_utc: u32,
    pub concurrency: usize,
    pub jitter_ms_min: u64,
    pub jitter_ms_max: u64,
    pub user_agent: String,
    pub stale_after_hours: i64,
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_cache_capacity")]
    pub cache_capacity: usize,
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            cache_capacity: default_cache_capacity(),
            cache_ttl_secs: default_cache_ttl_secs(),
            request_timeout_secs: default_request_timeout_secs(),
        }
    }
}

fn default_cache_capacity() -> usize {
    10_000
}

fn default_cache_ttl_secs() -> u64 {
    30
}

fn default_request_timeout_secs() -> u64 {
    30
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            run_at_startup: false,
            cron_hour_utc: 4,
            concurrency: 4,
            jitter_ms_min: 250,
            jitter_ms_max: 500,
            user_agent: "gear5-rs/0.1 (+contact-on-request)".to_string(),
            stale_after_hours: 36,
            base_url: "https://en.onepiece-cardgame.com/cardlist/".to_string(),
        }
    }
}

impl Config {
    /// Load layered config: defaults → `deploy/config.example.toml` (if found) → `./config.toml` (if found) → env (`GEAR5_*`, `DATABASE_URL`).
    pub fn load() -> crate::Result<Self> {
        let mut fig = Figment::new()
            .merge(Toml::file("deploy/config.example.toml"))
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("GEAR5_").split("__"));

        if let Ok(db) = std::env::var("DATABASE_URL") {
            fig = fig.merge(("database.url", db));
        }

        let cfg: Config = fig.extract()?;
        Ok(cfg)
    }
}
