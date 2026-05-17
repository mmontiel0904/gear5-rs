// Validates the generated OpenAPI document without booting the server.
//
// The binary crate does not export its modules to integration tests, so we re-include
// the openapi module and its transitive dependencies from source.

#![allow(dead_code, unused_imports)]

use serde_json::Value;

#[path = "../src/auth_cache.rs"]
mod auth_cache;
#[path = "../src/middleware/mod.rs"]
mod middleware;
#[path = "../src/openapi/mod.rs"]
mod openapi;
#[path = "../src/routes/mod.rs"]
mod routes;
#[path = "../src/search_cache.rs"]
mod search_cache;
#[path = "../src/state.rs"]
mod state;

use openapi::ApiDoc;
use utoipa::OpenApi;

#[test]
fn spec_has_expected_paths_and_security() {
    let json: Value = serde_json::to_value(ApiDoc::openapi()).expect("ApiDoc serializes to JSON");

    assert!(
        json.get("openapi")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .starts_with("3."),
        "expected OpenAPI 3.x, got {:?}",
        json.get("openapi"),
    );

    assert_eq!(
        json.pointer("/info/title").and_then(Value::as_str),
        Some("gear5-rs"),
    );

    let paths = json
        .pointer("/paths")
        .and_then(Value::as_object)
        .expect("paths object");
    let expected = [
        "/health",
        "/health/scrape",
        "/sets",
        "/cards",
        "/cards/search",
        "/cards/{code}",
        "/dump",
        "/images/{file}",
        "/admin/keys",
        "/admin/keys/{id}",
        "/admin/scrape/run",
    ];
    for p in expected {
        assert!(paths.contains_key(p), "expected path missing: {p}");
    }

    let scheme = json
        .pointer("/components/securitySchemes/BearerAuth")
        .expect("BearerAuth scheme present");
    assert_eq!(scheme.get("type").and_then(Value::as_str), Some("http"),);
    assert_eq!(scheme.get("scheme").and_then(Value::as_str), Some("bearer"),);

    // A representative authed route should declare the security requirement.
    let get_card_security = json
        .pointer("/paths/~1cards~1{code}/get/security")
        .and_then(Value::as_array)
        .expect("security array on GET /cards/{code}");
    assert!(
        get_card_security
            .iter()
            .any(|item| item.get("BearerAuth").is_some()),
        "GET /cards/{{code}} must require BearerAuth",
    );
}

#[test]
fn spec_components_include_core_schemas() {
    let json: Value = serde_json::to_value(ApiDoc::openapi()).unwrap();
    let schemas = json
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .expect("components.schemas present");

    for name in [
        "Card",
        "CardSet",
        "CardView",
        "CardListResponse",
        "CardSuggestion",
        "ScrapeHealth",
        "CreateKeyBody",
        "CreatedKeyResponse",
        "ApiKeyView",
        "ScrapeTriggerResponse",
        "ErrorBody",
    ] {
        assert!(schemas.contains_key(name), "missing schema: {name}");
    }
}
