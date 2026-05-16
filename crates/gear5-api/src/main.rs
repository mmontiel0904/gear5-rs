mod middleware;
mod routes;
mod scheduler;
mod state;

use anyhow::Context;
use axum::routing::{delete, get, post};
use axum::Router;
use gear5_core::config::Config;
use gear5_core::db;
use gear5_core::scraper::HttpClient;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    let cfg = Config::load().context("load config")?;
    tokio::fs::create_dir_all(&cfg.images.dir).await?;

    let pool = db::connect(&cfg.database).await.context("db connect")?;
    db::migrate(&pool).await.context("migrate")?;

    let http = HttpClient::new(&cfg.scrape).context("http client")?;
    let state = AppState {
        pool: pool.clone(),
        cfg: cfg.clone(),
        http: http.clone(),
        limiters: Arc::new(RwLock::new(HashMap::new())),
        scrape_lock: Arc::new(tokio::sync::Mutex::new(())),
    };

    if cfg.scrape.enabled {
        scheduler::spawn(state.clone());
    } else {
        tracing::info!("scrape scheduler disabled by config");
    }

    let app = Router::new()
        .route("/health", get(routes::health::liveness))
        .route("/health/scrape", get(routes::health::scrape_health))
        .route("/sets", get(routes::sets::list_sets))
        .route("/cards", get(routes::cards::list_cards))
        .route("/cards/:code", get(routes::cards::get_card))
        .route("/dump", get(routes::dump::dump))
        .route("/images/:file", get(routes::images::serve_image))
        .route(
            "/admin/keys",
            post(routes::admin::create_key).get(routes::admin::list_keys),
        )
        .route("/admin/keys/:id", delete(routes::admin::revoke_key))
        .route("/admin/scrape/run", post(routes::admin::trigger_scrape))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = cfg.server.bind.parse().context("parse bind addr")?;
    tracing::info!(%addr, "gear5-api listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
