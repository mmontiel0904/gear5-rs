// Error::Config wraps figment::Error which is ~200 bytes; the result-large-err
// clippy lint suggests boxing. The juice is not worth the squeeze for an internal
// error type used in non-hot paths (scrape orchestration, key issuance, startup).
#![allow(clippy::result_large_err)]

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod model;
pub mod scraper;

pub use error::{Error, Result};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
