use gear5_core::scraper::parse::{parse_cards, parse_sets};

const OP01_HTML: &str = include_str!("../../../tests/scrape_fixtures/op01.html");

#[test]
fn parses_op01_card_count() {
    let cards = parse_cards(OP01_HTML).expect("parse cards");
    assert_eq!(
        cards.len(),
        154,
        "OP01 fixture is expected to contain 154 cards"
    );
}

#[test]
fn parses_op01_leader_fields() {
    let cards = parse_cards(OP01_HTML).expect("parse cards");
    let zoro = cards
        .iter()
        .find(|c| c.code == "OP01-001")
        .expect("OP01-001 present");
    assert_eq!(zoro.name, "Roronoa Zoro");
    assert_eq!(zoro.rarity, "L");
    assert_eq!(zoro.category, "LEADER");
    assert_eq!(zoro.colors, vec!["Red".to_string()]);
    assert_eq!(zoro.life, Some(5));
    assert_eq!(zoro.power, Some(5000));
    assert_eq!(zoro.attribute.as_deref(), Some("Slash"));
    assert_eq!(zoro.image_filename, "OP01-001.png");
    assert!(
        !zoro.image_version.is_empty(),
        "image version should be parsed"
    );
    assert!(zoro
        .features
        .iter()
        .any(|f| f == "Supernovas" || f == "Straw Hat Crew"));
}

#[test]
fn parses_op01_alt_art_dual_color() {
    let cards = parse_cards(OP01_HTML).expect("parse cards");
    let luffy_alt = cards
        .iter()
        .find(|c| c.code == "OP01-003_p1")
        .expect("OP01-003_p1 alt art present");
    assert_eq!(
        luffy_alt.colors,
        vec!["Red".to_string(), "Green".to_string()]
    );
    assert_eq!(luffy_alt.category, "LEADER");
}

#[test]
fn parses_op01_sets_dropdown() {
    let sets = parse_sets(OP01_HTML).expect("parse sets");
    // Anything above ~20 entries means we got the real dropdown, not a fallback.
    assert!(
        sets.len() >= 20,
        "expected at least 20 set options, got {}",
        sets.len()
    );
    assert!(
        sets.iter().any(|s| s.source_series == "569101"),
        "OP-01 source_series 569101 must be in dropdown",
    );
}

#[test]
fn derives_set_id_from_first_card_code() {
    use gear5_core::scraper::parse::set_id_from_code;
    let cards = parse_cards(OP01_HTML).expect("parse cards");
    let first = &cards[0];
    let derived = set_id_from_code(&first.code).expect("derive set id");
    assert_eq!(derived, "OP01");
}
