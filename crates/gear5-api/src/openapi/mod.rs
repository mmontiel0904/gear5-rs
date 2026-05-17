use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

pub mod schemas;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "gear5-rs",
        version = env!("CARGO_PKG_VERSION"),
        description = "Scraped One Piece Card Game catalog. Authenticate with a `Bearer op_live_...` API key issued via the admin CLI.",
        license(name = "MIT"),
    ),
    paths(
        crate::routes::health::liveness,
        crate::routes::health::scrape_health,
        crate::routes::sets::list_sets,
        crate::routes::cards::list_cards,
        crate::routes::cards::get_card,
        crate::routes::dump::dump,
        crate::routes::images::serve_image,
        crate::routes::admin::create_key,
        crate::routes::admin::list_keys,
        crate::routes::admin::revoke_key,
        crate::routes::admin::trigger_scrape,
    ),
    components(schemas(
        gear5_core::model::Card,
        gear5_core::model::CardSet,
        crate::routes::cards::CardView,
        crate::routes::cards::CardListResponse,
        crate::routes::health::ScrapeHealth,
        crate::routes::admin::CreateKeyBody,
        crate::routes::admin::CreatedKeyResponse,
        crate::routes::admin::ApiKeyView,
        crate::routes::admin::ScrapeTriggerResponse,
        schemas::ErrorBody,
    )),
    tags(
        (name = "health",  description = "Liveness and scrape freshness probes."),
        (name = "catalog", description = "Cards and sets."),
        (name = "images",  description = "Card art served from the local image cache."),
        (name = "dump",    description = "Full NDJSON snapshots for client-side caching."),
        (name = "admin",   description = "API-key lifecycle and on-demand scrape triggers."),
    ),
    modifiers(&BearerAddon),
)]
pub struct ApiDoc;

struct BearerAddon;

impl Modify for BearerAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let scheme = SecurityScheme::Http(
            HttpBuilder::new()
                .scheme(HttpAuthScheme::Bearer)
                .bearer_format("op_live_<base32>")
                .description(Some(
                    "Paste an API key issued via `POST /admin/keys` or `gear5 key create`.",
                ))
                .build(),
        );
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme("BearerAuth", scheme);
        } else {
            let mut components = utoipa::openapi::ComponentsBuilder::new().build();
            components.add_security_scheme("BearerAuth", scheme);
            openapi.components = Some(components);
        }
    }
}
