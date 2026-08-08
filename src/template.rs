//! Template application — the only place the Archify front-end document is
//! touched. Every replacement below mirrors the original `applyTemplate`
//! helper byte-for-byte: the template's CSS/JS is never modified, only the
//! authored metadata sentinels and the pre-rendered SVG/cards slots.

use serde_json::Value;

/// The front-end template, embedded verbatim from the official Archify
/// project. Upstream template upgrades are picked up by replacing this file.
pub const TEMPLATE_HTML: &str = include_str!("../assets/template.html");

/// Replace the template's `[VISUAL PRESET]` value.
const PRESET_PLACEHOLDER: &str = "<html lang=\"en\" data-theme=\"dark\" data-preset=\"[VISUAL PRESET]\">";
const TITLE_PLACEHOLDER: &str = "<title>[PROJECT NAME] Architecture Diagram</title>";
const H1_PLACEHOLDER: &str = "<h1>[PROJECT NAME] Architecture</h1>";
const SUBTITLE_PLACEHOLDER: &str = "<p class=\"subtitle\">[Subtitle description]</p>";
const SVG_SLOT_RE: &str = "      <!-- ARCHIFY:SVG_SLOT_START -->";
const CARDS_SLOT_RE: &str = "    <!-- ARCHIFY:CARDS_SLOT_START -->";
const FOOTER_PLACEHOLDER: &str = "[Project Name] &bull; [Additional metadata]";
const GUIDED_VIEWS_PLACEHOLDER: &str = "<!-- ARCHIFY:GUIDED_VIEWS_DATA -->";
const SOURCE_EVIDENCE_PLACEHOLDER: &str = "    <!-- ARCHIFY:SOURCE_EVIDENCE_DATA -->";

const ESCAPE_MAP: [(char, &str); 5] = [
    ('&', "&amp;"),
    ('<', "&lt;"),
    ('>', "&gt;"),
    ('"', "&quot;"),
    ('\'', "&#39;"),
];

/// HTML-escape a value exactly like the original `esc()` helper.
pub fn esc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ESCAPE_MAP.iter().find(|(from, _)| *from == ch) {
            Some((_, to)) => out.push_str(to),
            None => out.push(ch),
        }
    }
    out
}

/// JSON stringify then escape `<`, `>`, `&` as unicode escapes — mirrors the
/// original `applyTemplate` JSON payload handling.
fn payload_json(value: &Value) -> String {
    let json = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
    json.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026")
}

/// Replace the first occurrence of `needle` with `replacement`.
fn replace_first(haystack: &str, needle: &str, replacement: &str) -> String {
    match haystack.find(needle) {
        Some(index) => {
            let mut out = String::with_capacity(haystack.len() + replacement.len());
            out.push_str(&haystack[..index]);
            out.push_str(replacement);
            out.push_str(&haystack[index + needle.len()..]);
            out
        }
        None => haystack.to_string(),
    }
}

/// Replace the region between the SVG slot sentinels (inclusive) with `svg`.
fn replace_slot(haystack: &str, start_sentinel: &str, end_sentinel: &str, replacement: &str) -> Result<String, String> {
    let start = haystack
        .find(start_sentinel)
        .ok_or_else(|| format!("template missing sentinel {start_sentinel:?}"))?;
    let end_rel = haystack[start..]
        .find(end_sentinel)
        .ok_or_else(|| format!("template missing sentinel {end_sentinel:?}"))?;
    let end = start + end_rel + end_sentinel.len();
    let mut out = String::with_capacity(haystack.len() + replacement.len());
    out.push_str(&haystack[..start]);
    out.push_str(replacement);
    out.push_str(&haystack[end..]);
    Ok(out)
}

/// Render the info-cards HTML block, matching `renderCards` exactly.
pub fn render_cards(cards: &Value) -> String {
    let list = cards.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return String::new();
    }
    let mut parts = vec!["    <!-- Info Cards -->".to_string(), "    <div class=\"cards\">".to_string()];
    let mut blocks = Vec::new();
    for card in &list {
        let dot = card.get("dot").and_then(Value::as_str).unwrap_or_default();
        let title = card.get("title").and_then(Value::as_str).unwrap_or_default();
        let items = card.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut block = format!(
            "      <div class=\"card\">\n        <div class=\"card-header\">\n          <div class=\"card-dot {}\"></div>\n          <h3>{}</h3>\n        </div>\n        <ul>",
            esc(dot),
            esc(title)
        );
        for item in &items {
            let text = item.as_str().unwrap_or_default();
            block.push_str(&format!("\n          <li>&bull; {}</li>", esc(text)));
        }
        block.push_str("\n        </ul>\n      </div>");
        blocks.push(block);
    }
    parts.push(blocks.join("\n\n"));
    parts.push("    </div>".to_string());
    parts.join("\n")
}

/// Options consumed by [`apply_template`].
pub struct TemplateData<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub footer: &'a str,
    pub svg: &'a str,
    pub cards: &'a str,
    pub visual_preset: &'a str,
    pub guided_views: &'a Value,
    pub source_evidence: Option<&'a Value>,
}

/// The exact `applyTemplate` port: validates all sentinels, then replaces each
/// placeholder in the original order (function replacers avoid `$&`-style
/// pattern injection).
pub fn apply_template(template: &str, data: &TemplateData) -> Result<String, String> {
    for (label, needle) in [
        ("SVG slot", SVG_SLOT_RE),
        ("cards slot", CARDS_SLOT_RE),
        ("preset", PRESET_PLACEHOLDER),
        ("title", TITLE_PLACEHOLDER),
        ("h1", H1_PLACEHOLDER),
        ("subtitle", SUBTITLE_PLACEHOLDER),
        ("footer", FOOTER_PLACEHOLDER),
        ("guided views", GUIDED_VIEWS_PLACEHOLDER),
    ] {
        if !template.contains(needle) {
            return Err(format!("applyTemplate: template missing placeholder {label:?}"));
        }
    }
    if data.source_evidence.is_some() && !template.contains(SOURCE_EVIDENCE_PLACEHOLDER) {
        return Err("applyTemplate: repository evidence requires the source-evidence slot".into());
    }

    let guided_views_json = payload_json(data.guided_views);
    let source_evidence_json = data.source_evidence.map(payload_json);

    let mut out = template.to_string();
    out = replace_first(&out, PRESET_PLACEHOLDER, &format!("<html lang=\"en\" data-theme=\"dark\" data-preset=\"{}\">", esc(data.visual_preset)));
    out = replace_first(&out, TITLE_PLACEHOLDER, &format!("<title>{} Diagram</title>", esc(data.title)));
    out = replace_first(&out, H1_PLACEHOLDER, &format!("<h1>{}</h1>", esc(data.title)));
    out = replace_first(&out, SUBTITLE_PLACEHOLDER, &format!("<p class=\"subtitle\">{}</p>", esc(data.subtitle.unwrap_or(""))));
    out = replace_slot(&out, SVG_SLOT_RE, "      <!-- ARCHIFY:SVG_SLOT_END -->", data.svg)?;
    out = replace_slot(&out, CARDS_SLOT_RE, "    <!-- ARCHIFY:CARDS_SLOT_END -->", data.cards)?;
    out = replace_first(&out, FOOTER_PLACEHOLDER, data.footer);
    let guided_views_tag =
        format!("<script id=\"archify-guided-views-data\" type=\"application/json\">{guided_views_json}</script>");
    out = replace_first(&out, GUIDED_VIEWS_PLACEHOLDER, &guided_views_tag);
    let source_evidence_tag = match source_evidence_json {
        Some(json) => format!("    <script id=\"archify-source-evidence-data\" type=\"application/json\">{json}</script>"),
        None => String::new(),
    };
    out = replace_first(&out, SOURCE_EVIDENCE_PLACEHOLDER, &source_evidence_tag);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MINI_TEMPLATE: &str = r#"<html lang="en" data-theme="dark" data-preset="[VISUAL PRESET]">
<title>[PROJECT NAME] Architecture Diagram</title>
<h1>[PROJECT NAME] Architecture</h1>
<p class="subtitle">[Subtitle description]</p>
<!-- ARCHIFY:GUIDED_VIEWS_DATA -->
    <!-- ARCHIFY:SOURCE_EVIDENCE_DATA -->
      <!-- ARCHIFY:SVG_SLOT_START --><svg></svg>      <!-- ARCHIFY:SVG_SLOT_END -->
    <!-- ARCHIFY:CARDS_SLOT_START --><div></div>    <!-- ARCHIFY:CARDS_SLOT_END -->
[Project Name] &bull; [Additional metadata]"#;

    #[test]
    fn applies_all_placeholders() {
        let out = apply_template(
            MINI_TEMPLATE,
            &TemplateData {
                title: "My System",
                subtitle: Some("Sub"),
                footer: "Architecture diagram &bull; footer",
                svg: "<svg viewBox=\"0 0 1 1\"></svg>",
                cards: "    <!-- Info Cards -->\n    <div class=\"cards\"></div>",
                visual_preset: "blueprint",
                guided_views: &json!([{ "id": "v1", "label": "V", "focus": ["a"] }]),
                source_evidence: None,
            },
        )
        .unwrap();
        assert!(out.contains("data-preset=\"blueprint\""));
        assert!(out.contains("<title>My System Diagram</title>"));
        assert!(out.contains("<h1>My System</h1>"));
        assert!(out.contains("<p class=\"subtitle\">Sub</p>"));
        assert!(out.contains("<svg viewBox=\"0 0 1 1\"></svg>"));
        assert!(out.contains("archify-guided-views-data"));
        assert!(!out.contains("[PROJECT NAME]"));
        assert!(!out.contains("GUIDED_VIEWS_DATA"));
    }

    #[test]
    fn esc_handles_dollar_sequences() {
        // A literal `$&` in authored text must survive untouched (the original
        // used function replacers precisely to avoid JS `$&` expansion).
        assert_eq!(esc("Plan $&50 tier"), "Plan $&amp;50 tier");
    }

    #[test]
    fn guided_views_json_escapes_markup() {
        let out = apply_template(
            MINI_TEMPLATE,
            &TemplateData {
                title: "T",
                subtitle: None,
                footer: "f",
                svg: "svg",
                cards: "cards",
                visual_preset: "classic",
                guided_views: &json!([{ "id": "v", "label": "<x>&", "focus": ["a"] }]),
                source_evidence: None,
            },
        )
        .unwrap();
        assert!(out.contains("\\u003cx\\u003e\\u0026"));
    }
}
