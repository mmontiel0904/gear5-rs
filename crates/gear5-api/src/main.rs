mod auth_cache;
mod middleware;
mod openapi;
mod routes;
mod scheduler;
mod state;

use anyhow::Context;
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::Router;
use gear5_core::config::Config;
use gear5_core::db;
use gear5_core::scraper::HttpClient;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth_cache::AuthCache;
use crate::middleware::AdminAuth;
use crate::openapi::ApiDoc;
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

    let cache_capacity =
        NonZeroUsize::new(cfg.auth.cache_capacity.max(1)).expect("cache_capacity is clamped >= 1");
    let auth_cache = Arc::new(AuthCache::new(
        cache_capacity,
        Duration::from_secs(cfg.auth.cache_ttl_secs.max(1)),
    ));

    let state = AppState {
        pool: pool.clone(),
        cfg: cfg.clone(),
        http: http.clone(),
        limiters: Arc::new(RwLock::new(HashMap::new())),
        scrape_lock: Arc::new(tokio::sync::Mutex::new(())),
        auth_cache,
    };

    if cfg.scrape.enabled {
        scheduler::spawn(state.clone());
    } else {
        tracing::info!("scrape scheduler disabled by config");
    }

    let request_timeout = Duration::from_secs(cfg.auth.request_timeout_secs.max(1));

    let openapi_spec = ApiDoc::openapi();
    let docs_router: Router<AppState> = SwaggerUi::new("/docs")
        .url("/api-doc/openapi.json", openapi_spec)
        .into();
    let docs_router = if cfg.auth.public_docs {
        docs_router
    } else {
        docs_router.layer(axum::middleware::from_extractor_with_state::<
            AdminAuth,
            AppState,
        >(state.clone()))
    };

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
        .merge(docs_router)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = cfg.server.bind.parse().context("parse bind addr")?;
    tracing::info!(%addr, "gear5-api listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("gear5-api shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to install ctrl_c handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT, draining"),
        _ = terminate => tracing::info!("received SIGTERM, draining"),
    }
}
