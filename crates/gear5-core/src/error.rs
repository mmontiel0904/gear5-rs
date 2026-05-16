use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(#[from] figment::Error),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("migrate error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("argon2: {0}")]
    Argon2(String),

    #[error("invalid api key")]
    InvalidApiKey,

    #[error("not found")]
    NotFound,

    #[error("{0}")]
    Other(String),
}

impl From<argon2::password_hash::Error> for Error {
    fn from(e: argon2::password_hash::Error) -> Self {
        Error::Argon2(e.to_string())
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::Parse(e.to_string())
    }
}
