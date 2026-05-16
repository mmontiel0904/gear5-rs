use crate::model::{ParsedCard, ParsedSet};
use crate::{Error, Result};
use scraper::{ElementRef, Html, Selector};
use std::sync::OnceLock;

fn sel(s: &'static str) -> &'static Selector {
    static MAP: OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, &'static Selector>>,
    > = OnceLock::new();
    let m = MAP.get_or_init(Default::default);
    let mut g = m.lock().unwrap();
    if let Some(found) = g.get(s) {
        return found;
    }
    let boxed: &'static Selector =
        Box::leak(Box::new(Selector::parse(s).expect("static selector")));
    g.insert(s, boxed);
    boxed
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn full_text(el: ElementRef) -> String {
    collapse_ws(&el.text().collect::<String>())
}

/// Inside a label-value div like `<div class="cost"><h3>Cost</h3>5</div>`,
/// return the trimmed value text.
fn value_after_h3(el: ElementRef) -> String {
    let h3 = el.select(sel("h3")).next();
    let h3_text = h3.map(|h| h.text().collect::<String>()).unwrap_or_default();
    let h3_collapsed = collapse_ws(&h3_text);
    let full = collapse_ws(&el.text().collect::<String>());
    full.strip_prefix(&h3_collapsed)
        .unwrap_or(&full)
        .trim()
        .to_string()
}

fn label_of(el: ElementRef) -> Option<String> {
    el.select(sel("h3"))
        .next()
        .map(|h| collapse_ws(&h.text().collect::<String>()))
}

fn parse_int(s: &str) -> Option<i32> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Extract the human-readable set name between the first pair of dashes.
fn extract_set_name(label: &str) -> String {
    if let Some(first) = label.find('-') {
        let rest = &label[first + 1..];
        if let Some(second) = rest.find('-') {
            let candidate = rest[..second].trim();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }
    label.trim().to_string()
}

pub fn parse_sets(html: &str) -> Result<Vec<ParsedSet>> {
    let doc = Html::parse_document(html);
    let select = doc
        .select(sel("select[name=\"series\"]"))
        .next()
        .ok_or_else(|| Error::Parse("series select missing on index page".to_string()))?;
    let mut out = Vec::new();
    for opt in select.select(sel("option")) {
        let value = match opt.value().attr("value") {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => continue,
        };
        // The HTML contains things like <option ...>...&lt;br&gt;... [OP01]</option>;
        // scraper already decodes entities, but inline <br> elements stay as elements.
        let display_label = full_text(opt);
        if display_label.eq_ignore_ascii_case("ALL")
            || display_label.eq_ignore_ascii_case("Recording")
        {
            continue;
        }
        let name = extract_set_name(&display_label);
        out.push(ParsedSet {
            source_series: value,
            name,
            display_label,
        });
    }
    Ok(out)
}

/// Derive a set id from a card code, e.g. `OP01-001` -> `OP01`, `EB04-001` -> `EB04`.
pub fn set_id_from_code(code: &str) -> Option<&str> {
    let cut = code.find('-')?;
    let prefix = &code[..cut];
    if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    }
}

pub fn parse_cards(html: &str) -> Result<Vec<ParsedCard>> {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for dl in doc.select(sel("dl.modalCol")) {
        match parse_card(dl) {
            Ok(card) => out.push(card),
            Err(e) => {
                tracing::warn!(error = %e, "skipping card row");
                continue;
            }
        }
    }
    Ok(out)
}

fn parse_card(dl: ElementRef) -> Result<ParsedCard> {
    let code = dl
        .value()
        .attr("id")
        .ok_or_else(|| Error::Parse("card dl missing id".to_string()))?
        .to_string();

    let info_spans: Vec<String> = dl
        .select(sel("div.infoCol span"))
        .map(|s| collapse_ws(&s.text().collect::<String>()))
        .collect();
    let rarity = info_spans.get(1).cloned().unwrap_or_default();
    let category = info_spans.get(2).cloned().unwrap_or_default();

    let name = dl
        .select(sel("div.cardName"))
        .next()
        .map(full_text)
        .unwrap_or_default();

    let (image_filename, image_version) = dl
        .select(sel(".frontCol img"))
        .next()
        .and_then(|img| img.value().attr("data-src"))
        .and_then(split_image_src)
        .unwrap_or_else(|| (format!("{code}.png"), String::new()));

    // Walk every div under <dd> that holds an <h3> label.
    let mut cost = None;
    let mut life = None;
    let mut power = None;
    let mut counter = None;
    let mut attribute: Option<String> = None;
    let mut block = None;
    let mut color = String::new();
    let mut card_type: Option<String> = None;
    let mut effect_text: Option<String> = None;
    let mut trigger_text: Option<String> = None;
    let mut notes: Option<String> = None;

    for inner in dl.select(sel("dd div")) {
        let Some(label) = label_of(inner) else {
            continue;
        };
        let lower = label.to_ascii_lowercase();
        let value = value_after_h3(inner);

        if lower.starts_with("cost") {
            cost = parse_int(&value);
        } else if lower.starts_with("life") {
            life = parse_int(&value);
        } else if lower.starts_with("power") {
            power = parse_int(&value);
        } else if lower.starts_with("counter") {
            counter = parse_int(&value);
        } else if lower.starts_with("attribute") {
            // Value may include an icon's alt text; rely on <i> child.
            attribute = inner
                .select(sel("i"))
                .next()
                .map(|i| collapse_ws(&i.text().collect::<String>()))
                .filter(|s| !s.is_empty())
                .or_else(|| (!value.is_empty()).then_some(value));
        } else if lower.starts_with("block") {
            block = parse_int(&value);
        } else if lower.starts_with("color") {
            color = value;
        } else if lower.starts_with("type") {
            card_type = (!value.is_empty()).then_some(value);
        } else if lower.starts_with("effect") {
            effect_text = (!value.is_empty() && value != "-").then_some(value);
        } else if lower.starts_with("trigger") {
            trigger_text = (!value.is_empty() && value != "-").then_some(value);
        } else if lower.starts_with("notes") {
            notes = (!value.is_empty()).then_some(value);
        }
    }

    let colors: Vec<String> = if color.is_empty() {
        Vec::new()
    } else {
        color
            .split('/')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let features: Vec<String> = card_type
        .as_deref()
        .map(|t| {
            t.split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(ParsedCard {
        code,
        name,
        rarity,
        category,
        color,
        colors,
        cost,
        life,
        power,
        counter,
        attribute,
        block,
        card_type,
        features,
        effect_text,
        trigger_text,
        notes,
        image_filename,
        image_version,
    })
}

/// `data-src="../images/cardlist/card/OP01-001.png?260508"` -> ("OP01-001.png", "260508")
fn split_image_src(src: &str) -> Option<(String, String)> {
    let (path, version) = match src.split_once('?') {
        Some((p, v)) => (p, v.to_string()),
        None => (src, String::new()),
    };
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    if filename.is_empty() {
        return None;
    }
    Some((filename, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_set_id_from_code() {
        assert_eq!(set_id_from_code("OP01-001"), Some("OP01"));
        assert_eq!(set_id_from_code("EB04-010"), Some("EB04"));
        assert_eq!(set_id_from_code("OP01-003_p1"), Some("OP01"));
        assert_eq!(set_id_from_code("garbled"), None);
    }

    #[test]
    fn parses_int_with_dash() {
        assert_eq!(parse_int("5000"), Some(5000));
        assert_eq!(parse_int("-"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("5,000"), Some(5000));
    }

    #[test]
    fn splits_image_src() {
        let (f, v) = split_image_src("../images/cardlist/card/OP01-001.png?260508").unwrap();
        assert_eq!(f, "OP01-001.png");
        assert_eq!(v, "260508");
    }
}
