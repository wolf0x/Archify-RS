//! Architecture delta comparison → faithful port of the original
//! `delta/architecture-delta.mjs`.
//!
//! `compare_file` renders both snapshots, computes a change receipt
//! (canonicalization + entity diff), annotates the head SVG with phantom
//! baseline elements and delta markers, and writes a standalone review HTML
//! that embeds the unmodified template's stylesheet.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use crate::renderer;

const COMPARATOR_VERSION: u8 = 1;
const CANONICAL_VERSION: u8 = 1;

/// The standalone delta review page; sentinels are replaced by
/// `render_delta_html`.
const DELTA_TEMPLATE: &str = include_str!("delta_template.html");

// ---------------------------------------------------------------------------
// Canonicalization
// ---------------------------------------------------------------------------

/// JS `canonical(value)` → deterministic serialization used for equality.
fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .iter()
                .map(|key| format!("{}:{}", serde_json::to_string(key).unwrap_or_default(), canonical(&map[*key])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn equal(left: &Value, right: &Value) -> bool {
    canonical(left) == canonical(right)
}

/// `equal` for optional values → `None` compares as `null`.
fn equal_option(left: &Option<Value>, right: &Option<Value>) -> bool {
    equal(&left.clone().unwrap_or(Value::Null), &right.clone().unwrap_or(Value::Null))
}

fn sorted_objects(values: &Value) -> Value {
    let mut items: Vec<Value> = values
        .as_array()
        .map(|a| a.clone())
        .unwrap_or_default();
    items.sort_by(|a, b| canonical(a).cmp(&canonical(b)));
    Value::Array(items)
}

/// `normalizeRepository` → URL trimmed/`.git`-stripped/lowercased.
fn normalize_repository(repository: Option<&Value>) -> Option<Value> {
    let repository = repository?;
    if repository.is_null() {
        return None;
    }
    let url = repository
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let url = url
        .strip_suffix(".git")
        .unwrap_or(&url)
        .trim_end_matches('/')
        .to_string();
    let revision = repository
        .get("revision")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    Some(json!({ "url": url, "revision": revision }))
}

/// `canonicalArchitecture` → normalized snapshot for deterministic geometry.
pub fn canonical_architecture(diagram: &Value) -> Value {
    let mut meta = diagram
        .get("meta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    meta.remove("output");
    let repository = meta.get("repository").map(|r| normalize_repository(Some(r))).flatten();
    if let Some(repository) = repository {
        meta.insert("repository".to_string(), repository);
    }
    let mut components: Vec<Value> = diagram
        .get("components")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let mut c = c;
            if let Some(sources) = c.get("sources").cloned() {
                if sources.is_array() {
                    c.as_object_mut().unwrap().insert("sources".to_string(), sorted_objects(&sources));
                }
            }
            c
        })
        .collect();
    components.sort_by(|a, b| {
        let (a_id, b_id) = (a.get("id").and_then(Value::as_str).unwrap_or(""), b.get("id").and_then(Value::as_str).unwrap_or(""));
        a_id.cmp(b_id)
    });
    let mut boundaries: Vec<Value> = diagram
        .get("boundaries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    boundaries.sort_by(|a, b| boundary_key(a).cmp(&boundary_key(b)));
    let mut connections: Vec<Value> = diagram
        .get("connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    connections.sort_by(|a, b| {
        let (a_id, b_id) = (a.get("id").and_then(Value::as_str).unwrap_or(""), b.get("id").and_then(Value::as_str).unwrap_or(""));
        a_id.cmp(b_id)
    });
    let mut out = Map::new();
    out.insert("schema_version".to_string(), diagram.get("schema_version").cloned().unwrap_or(Value::Null));
    out.insert("diagram_type".to_string(), diagram.get("diagram_type").cloned().unwrap_or(Value::Null));
    out.insert("meta".to_string(), Value::Object(meta));
    if diagram.get("layout").is_some() {
        out.insert("layout".to_string(), diagram.get("layout").cloned().unwrap());
    }
    out.insert("components".to_string(), Value::Array(components));
    out.insert("boundaries".to_string(), Value::Array(boundaries));
    out.insert("connections".to_string(), Value::Array(connections));
    if diagram.get("cards").is_some() {
        out.insert("cards".to_string(), diagram.get("cards").cloned().unwrap());
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

fn require_comparable_shape(diagram: &Value, side: &str) -> Result<()> {
    if diagram.get("schema_version").and_then(Value::as_u64) != Some(1) {
        bail!("delta/schema-version-mismatch: {side} must use schema_version 1.");
    }
    if diagram.get("diagram_type").and_then(Value::as_str) != Some("architecture") {
        bail!("delta/type-mismatch: {side} must use diagram_type architecture.");
    }
    Ok(())
}

fn stable_index(items: Option<&Value>, collection: &str, side: &str, missing_code: &str) -> Result<HashMap<String, Value>> {
    let mut index = HashMap::new();
    let mut missing: Vec<String> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    if let Some(items) = items.and_then(Value::as_array) {
        for (item_index, item) in items.iter().enumerate() {
            match item.get("id").and_then(Value::as_str) {
                None => missing.push(format!("/{collection}/{item_index}/id")),
                Some(id) => {
                    if index.contains_key(id) {
                        duplicates.push(id.to_string());
                    } else {
                        index.insert(id.to_string(), item.clone());
                    }
                }
            }
        }
    }
    if !missing.is_empty() {
        bail!("{missing_code}: {side} {collection} require authored stable ids for comparison.");
    }
    if !duplicates.is_empty() {
        bail!("delta/duplicate-stable-id: {side} {collection} contain duplicate ids.");
    }
    Ok(index)
}

fn boundary_key(boundary: &Value) -> String {
    let kind = boundary.get("kind").and_then(Value::as_str).unwrap_or("");
    let label = boundary.get("label").and_then(Value::as_str).unwrap_or("");
    format!("{kind}\u{1f}{label}")
}

fn boundary_index(boundaries: Option<&Value>, side: &str) -> Result<HashMap<String, Value>> {
    let mut index = HashMap::new();
    let mut ambiguous: Vec<String> = Vec::new();
    if let Some(boundaries) = boundaries.and_then(Value::as_array) {
        for boundary in boundaries {
            let key = boundary_key(boundary);
            if index.contains_key(&key) {
                let kind = boundary.get("kind").and_then(Value::as_str).unwrap_or("");
                let label = boundary.get("label").and_then(Value::as_str).unwrap_or("");
                ambiguous.push(format!("{kind}:{label}"));
            } else {
                index.insert(key, boundary.clone());
            }
        }
    }
    if !ambiguous.is_empty() {
        bail!("delta/boundary-key-ambiguous: {side} boundary kind + label keys must be unique.");
    }
    Ok(index)
}

fn normalized_field(item: &Value, field: &str) -> Option<Value> {
    let value = item.get(field)?;
    if field == "sources" && value.is_array() {
        return Some(sorted_objects(value));
    }
    if field == "wraps" && value.is_array() {
        let mut items: Vec<String> = value.as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
        items.sort();
        return Some(Value::Array(items.into_iter().map(Value::String).collect()));
    }
    Some(value.clone())
}

fn field_changes(before: &Value, after: &Value, groups: &[(&str, &[&str])]) -> (Vec<String>, Vec<String>) {
    let mut classifications = Vec::new();
    let mut changed_fields = Vec::new();
    for (classification, fields) in groups {
        let changed: Vec<&str> = fields
            .iter()
            .filter(|field| !equal_option(&normalized_field(before, field), &normalized_field(after, field)))
            .copied()
            .collect();
        if !changed.is_empty() {
            classifications.push(classification.to_string());
        }
        changed_fields.extend(changed.iter().map(|field| format!("/{field}")));
    }
    classifications.sort();
    changed_fields.sort();
    (classifications, changed_fields)
}

fn status_for(classifications: &[String], kind: &str) -> &'static str {
    if classifications.iter().any(|c| ["topology", "semantic", "scope"].contains(&c.as_str())) {
        return "changed";
    }
    if classifications.iter().any(|c| c == "evidence") {
        return "evidence-changed";
    }
    if classifications.iter().any(|c| c == "geometry") {
        return match kind {
            "connection" => "rerouted",
            "component" => "moved",
            _ => "geometry-changed",
        };
    }
    "same"
}

fn compare_entities(
    base: &HashMap<String, Value>,
    head: &HashMap<String, Value>,
    kind: &str,
    groups: &[(&str, &[&str])],
    describe: &dyn Fn(&str, Option<&Value>, Option<&Value>) -> Value,
) -> Vec<Value> {
    let mut ids: Vec<&String> = base.keys().chain(head.keys()).collect();
    ids.sort();
    ids.dedup();
    let mut changes = Vec::new();
    for id in ids {
        let base_item = base.get(id);
        let head_item = head.get(id);
        let mut pushed = false;
        if base_item.is_none() {
            changes.push(json!({ "status": "added", "classifications": [identity_classification(kind)], "changedFields": [] }));
            pushed = true;
        } else if head_item.is_none() {
            changes.push(json!({ "status": "removed", "classifications": [identity_classification(kind)], "changedFields": [] }));
            pushed = true;
        } else {
            let (classifications, changed_fields) = field_changes(base_item.unwrap(), head_item.unwrap(), groups);
            let status = status_for(&classifications, kind);
            if status != "same" {
                changes.push(json!({ "status": status, "classifications": classifications, "changedFields": changed_fields }));
                pushed = true;
            }
        }
        // Describe the change only when this id actually produced one;
        // unchanged entities must never touch a previous change's record.
        if pushed {
            if let Some(change) = changes.last_mut() {
                if let Value::Object(map) = change {
                    let described = describe(id, base_item, head_item);
                    if let Value::Object(described_map) = described {
                        for (k, v) in described_map {
                            map.insert(k, v);
                        }
                    }
                }
            }
        }
    }
    changes
}

fn identity_classification(kind: &str) -> &'static str {
    match kind {
        "connection" => "topology",
        "boundary" => "scope",
        _ => "semantic",
    }
}

fn summary_for(changes: &[Value], shape: &[&str]) -> Value {
    let mut summary = Map::new();
    for key in shape {
        summary.insert(key.to_string(), Value::from(0));
    }
    for change in changes {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        // Convert hyphenated status to camelCase to match summary shape keys.
        let key = status
            .split('-')
            .enumerate()
            .map(|(i, part)| if i == 0 { part.to_string() } else { part[..1].to_uppercase() + &part[1..] })
            .collect::<String>();
        if let Some(count) = summary.get_mut(&key) {
            if let Value::Number(number) = count {
                if let Some(value) = number.as_u64() {
                    *count = Value::from(value + 1);
                }
            }
        }
    }
    Value::Object(summary)
}

fn presentation_changed(base: &Value, head: &Value) -> bool {
    let pick = |diagram: &Value| -> Value {
        let meta = diagram.get("meta").cloned().unwrap_or(Value::Null);
        let mut presentation = Map::new();
        for field in ["title", "subtitle", "animation", "visual_preset", "quality_profile", "engineering_profile", "legend", "views", "viewBox"] {
            presentation.insert(field.to_string(), meta.get(field).cloned().unwrap_or(Value::Null));
        }
        presentation.insert("layout".to_string(), diagram.get("layout").cloned().unwrap_or(Value::Null));
        presentation.insert("cards".to_string(), diagram.get("cards").cloned().unwrap_or(Value::Null));
        Value::Object(presentation)
    };
    !equal(&pick(base), &pick(head))
}

/// `compareArchitecture` → full receipt computation.
pub fn compare_architecture(base: &Value, head: &Value) -> Result<Value> {
    require_comparable_shape(base, "base")?;
    require_comparable_shape(head, "head")?;

    let base_components = stable_index(base.get("components"), "components", "base", "delta/stable-id-required")?;
    let head_components = stable_index(head.get("components"), "components", "head", "delta/stable-id-required")?;
    let shared = base_components.keys().filter(|id| head_components.contains_key(*id)).count();
    if shared == 0 {
        bail!("delta/no-shared-component-id: The snapshots share no component id, so Archify cannot prove that they describe the same system.");
    }

    let base_connections = stable_index(base.get("connections"), "connections", "base", "delta/relationship-id-required")?;
    let head_connections = stable_index(head.get("connections"), "connections", "head", "delta/relationship-id-required")?;
    let base_boundaries = boundary_index(base.get("boundaries"), "base")?;
    let head_boundaries = boundary_index(head.get("boundaries"), "head")?;

    let base_repository = normalize_repository(base.get("meta").and_then(|m| m.get("repository")));
    let head_repository = normalize_repository(head.get("meta").and_then(|m| m.get("repository")));
    if let (Some(base_repository), Some(head_repository)) = (&base_repository, &head_repository) {
        if base_repository.get("url") != head_repository.get("url") {
            bail!("delta/repository-mismatch: The snapshots name different repositories.");
        }
    }
    let proof_level = if base_repository.is_some()
        && head_repository.is_some()
        && base_repository.as_ref().unwrap().get("revision").and_then(Value::as_str).map(|r| r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false)
        && head_repository.as_ref().unwrap().get("revision").and_then(Value::as_str).map(|r| r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false)
    {
        "revision-pinned"
    } else {
        "authored"
    };

    let component_groups: [(&str, &[&str]); 3] = [
        ("semantic", &["type", "label", "sublabel", "tag"]),
        ("evidence", &["sources"]),
        ("geometry", &["row", "col", "pos", "size"]),
    ];
    let connection_groups: [(&str, &[&str]); 3] = [
        ("topology", &["from", "to"]),
        ("semantic", &["label", "variant"]),
        ("geometry", &["fromSide", "toSide", "route", "via", "labelAt", "labelDx", "labelDy", "labelSegment", "width"]),
    ];
    let boundary_groups: [(&str, &[&str]); 2] = [("scope", &["wraps"]), ("geometry", &["pad"])];

    let components = compare_entities(&base_components, &head_components, "component", &component_groups, &|id, before, after| json!({
        "id": id,
        "baseLabel": before.and_then(|b| b.get("label")).cloned().unwrap_or(Value::Null),
        "headLabel": after.and_then(|a| a.get("label")).cloned().unwrap_or(Value::Null),
    }));
    let connections = compare_entities(&base_connections, &head_connections, "connection", &connection_groups, &|id, before, after| {
        let mut map = Map::new();
        map.insert("id".to_string(), Value::String(id.to_string()));
        if let Some(before) = before {
            map.insert("base".to_string(), json!({ "from": before.get("from").cloned().unwrap_or(Value::Null), "to": before.get("to").cloned().unwrap_or(Value::Null), "label": before.get("label").and_then(Value::as_str).unwrap_or("") }));
        }
        if let Some(after) = after {
            map.insert("head".to_string(), json!({ "from": after.get("from").cloned().unwrap_or(Value::Null), "to": after.get("to").cloned().unwrap_or(Value::Null), "label": after.get("label").and_then(Value::as_str).unwrap_or("") }));
        }
        Value::Object(map)
    });
    let boundaries = compare_entities(&base_boundaries, &head_boundaries, "boundary", &boundary_groups, &|_key, before, after| {
        let current = after.or(before).unwrap();
        let kind = current.get("kind").and_then(Value::as_str).unwrap_or("");
        let label = current.get("label").and_then(Value::as_str).unwrap_or("");
        json!({ "key": format!("{kind}:{label}"), "kind": kind, "label": label })
    });
    let provenance_changed = !equal_option(&base_repository, &head_repository);

    let summary = json!({
        "components": summary_for(&components, &["added", "changed", "evidenceChanged", "removed", "moved"]),
        "connections": summary_for(&connections, &["added", "changed", "removed", "rerouted"]),
        "boundaries": summary_for(&boundaries, &["added", "changed", "removed", "geometryChanged"]),
        "presentationChanged": presentation_changed(base, head),
        "provenanceChanged": provenance_changed,
    });

    Ok(json!({
        "schemaVersion": 1,
        "ok": true,
        "command": "compare",
        "type": "architecture",
        "comparatorVersion": COMPARATOR_VERSION,
        "canonicalVersion": CANONICAL_VERSION,
        "completeness": "complete",
        "proofLevel": proof_level,
        "base": {
            "title": base.get("meta").and_then(|m| m.get("title")).and_then(Value::as_str).unwrap_or(""),
            "revision": base_repository.as_ref().and_then(|r| r.get("revision")).cloned().unwrap_or(Value::Null),
        },
        "head": {
            "title": head.get("meta").and_then(|m| m.get("title")).and_then(Value::as_str).unwrap_or(""),
            "revision": head_repository.as_ref().and_then(|r| r.get("revision")).cloned().unwrap_or(Value::Null),
        },
        "summary": summary,
        "changes": { "components": components, "connections": connections, "boundaries": boundaries },
        "identity": {
            "components": "components[].id",
            "connections": "connections[].id (required)",
            "boundaries": "boundaries[].kind + boundaries[].label (derived)",
        },
        "view": { "visualPreset": head.get("meta").and_then(|m| m.get("visual_preset")).and_then(Value::as_str).unwrap_or("classic") },
        "limitations": [
            "Authored Architecture IR only; no runtime impact, causality, risk, or mergeability is inferred.",
            "Boundary identity is conservatively derived from kind + label.",
        ],
    }))
}

// ---------------------------------------------------------------------------
// SVG extraction / annotation
// ---------------------------------------------------------------------------

/// HTML-escape exactly like the original `esc()` helper.
fn esc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `safeJson` → JSON string with HTML-significant characters escaped.
fn safe_json(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

/// `extractArchitectureSvg` → primary diagram SVG from a rendered artifact.
pub fn extract_architecture_svg(html: &str) -> Result<String> {
    let marker = r#"<svg viewBox="0 0 "#;
    let start = html.find(marker).ok_or_else(|| anyhow::anyhow!("delta/svg-missing: A validated Architecture artifact did not contain its primary SVG."))?;
    let end = html[start..]
        .find("</svg>")
        .map(|i| start + i + "</svg>".len())
        .ok_or_else(|| anyhow::anyhow!("delta/svg-missing: A validated Architecture artifact did not contain its primary SVG."))?;
    Ok(html[start..end].to_string())
}

/// `extractArtifactCss` → the artifact `<style>` block.
pub fn extract_artifact_css(html: &str) -> Result<String> {
    let start = html.find("<style>").ok_or_else(|| anyhow::anyhow!("delta/css-missing: A validated Architecture artifact did not contain its stylesheet."))?;
    let content_start = start + "<style>".len();
    let end = html[content_start..]
        .find("</style>")
        .map(|i| content_start + i)
        .ok_or_else(|| anyhow::anyhow!("delta/css-missing: A validated Architecture artifact did not contain its stylesheet."))?;
    Ok(html[content_start..end].to_string())
}

fn change_map(changes: &Value) -> HashMap<String, Value> {
    changes
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|change| {
                    change
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|id| (id.to_string(), change.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn boundary_change_map(changes: &Value) -> HashMap<String, Value> {
    changes
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|change| {
                    let kind = change.get("kind").and_then(Value::as_str).unwrap_or("");
                    let label = change.get("label").and_then(Value::as_str).unwrap_or("");
                    Some((format!("{kind}:{}", esc(label)), change.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn add_state(tag: &str, change: Option<&Value>, side: &str, forced_state: Option<&str>) -> String {
    // Append attributes before the tag's closing `/>` (self-closing) or before
    // its final `>` (regular open tag), mirroring the JS `/>$/`/`/>$/` anchors.
    let append = |attributes: String| -> String {
        if let Some(stripped) = tag.strip_suffix("/>") {
            format!("{stripped}{attributes}/>")
        } else if let Some(pos) = tag.rfind('>') {
            format!("{}{}{}", &tag[..pos], attributes, &tag[pos..])
        } else {
            tag.to_string()
        }
    };
    let (state, classes) = match (change, forced_state) {
        (None, None) => ("same".to_string(), String::new()),
        (Some(change), _) => {
            let mut state = forced_state.unwrap_or(change.get("status").and_then(Value::as_str).unwrap_or("same")).to_string();
            if change.get("status").and_then(Value::as_str) == Some("added") && side == "base" {
                state = "same".to_string();
            }
            if change.get("status").and_then(Value::as_str) == Some("removed") && side == "head" {
                state = "same".to_string();
            }
            let classes = change
                .get("classifications")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            (state, classes)
        }
        (None, Some(state)) => (state.to_string(), String::new()),
    };
    let mut attributes = format!(" data-delta-state=\"{}\"", esc(&state));
    if !classes.is_empty() {
        attributes.push_str(&format!(" data-delta-classifications=\"{}\"", esc(&classes)));
    }
    append(attributes)
}

fn marker_for(state: &str) -> &'static str {
    match state {
        "added" => "+",
        "removed" => "\u{2212}",
        "changed" => "~",
        "moved" | "moved-from" | "rerouted" | "geometry-changed" => "\u{2194}",
        "evidence-changed" => "E",
        _ => "",
    }
}

fn add_node_marker(group: &str, state: &str) -> String {
    let symbol = marker_for(state);
    if symbol.is_empty() {
        return group.to_string();
    }
    let box_start = group.find(r#"<rect x=""#);
    let Some(box_start) = box_start else { return group.to_string() };
    let x = read_attr_number(group, box_start, r#"x=""#);
    let y = read_attr_number(group, box_start, r#"y=""#);
    let width = read_attr_number(group, box_start, r#"width=""#);
    let (Some(x), Some(y), Some(width)) = (x, y, width) else { return group.to_string() };
    let mx = x + width - 9.0;
    let my = y + 9.0;
    let marker = format!(
        "\n          <g class=\"delta-node-marker\" aria-hidden=\"true\"><circle cx=\"{}\" cy=\"{}\" r=\"8\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>\n        </g>",
        num(mx),
        num(my),
        num(mx),
        num(my + 3.0),
        symbol
    );
    if let Some(tail) = group.strip_suffix("</g>") {
        format!("{tail}{marker}")
    } else {
        group.to_string()
    }
}

fn num(value: f64) -> String {
    let mut text = format!("{value}");
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    text
}

fn read_attr_number(haystack: &str, from: usize, attr: &str) -> Option<f64> {
    let start = haystack[from..].find(attr).map(|i| from + i + attr.len())?;
    let rest = &haystack[start..];
    let end = rest.find('"')?;
    rest[..end].parse().ok()
}

/// `prefixSvgIds` → namespace all ids and url(#...) references.
fn prefix_svg_ids(svg: &str, prefix: &str) -> String {
    let ids: Vec<String> = svg
        .match_indices(" id=\"")
        .filter_map(|(i, _)| {
            let start = i + " id=\"".len();
            let rest = &svg[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect();
    let mut result = String::with_capacity(svg.len());
    let mut last = 0;
    for (i, _) in svg.match_indices(" id=\"") {
        result.push_str(&svg[last..i]);
        result.push_str(" id=\"");
        let start = i + " id=\"".len();
        let end = svg[start..].find('"').map(|e| start + e).unwrap_or(svg.len());
        result.push_str(prefix);
        result.push('-');
        result.push_str(&svg[start..end]);
        last = end;
    }
    result.push_str(&svg[last..]);
    // Rewrite url(#id) and href="#id" references.
    for id in &ids {
        result = result.replace(&format!("url(#{id})"), &format!("url(#{prefix}-{id})"));
        result = result.replace(&format!("href=\"#{id}\""), &format!("href=\"#{prefix}-{id}\""));
    }
    // Rewrite aria-labelledby references.
    let mut cursor = 0;
    while let Some(rel) = result[cursor..].find("aria-labelledby=\"") {
        let start = cursor + rel + "aria-labelledby=\"".len();
        let end = result[start..].find('"').map(|e| start + e).unwrap_or(result.len());
        let rewritten: String = result[start..end]
            .split_whitespace()
            .map(|id| format!("{prefix}-{id}"))
            .collect::<Vec<_>>()
            .join(" ");
        result.replace_range(start..end, &rewritten);
        cursor = start + rewritten.len();
    }
    result
}

/// `staticize` → strip interactive affordances from snapshot SVGs.
fn staticize(svg: &str) -> String {
    svg.replace("tabindex=\"0\" role=\"button\"", "role=\"group\"")
        .replace(" aria-pressed=\"false\"", "")
        .replace("aria-label=\"Focus ", "aria-label=\"")
}

/// Node `<g data-node-id="...">` group ranges.
fn node_group_ranges(svg: &str) -> Vec<(String, usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(open_index) = svg[search_from..].find("data-node-id=\"") {
        let open_start = search_from + open_index;
        // Rewind to the start of the <g ...> tag.
        let tag_start = svg[..open_start].rfind("<g ").or_else(|| svg[..open_start].rfind("<g>")).unwrap_or(open_start);
        let id_start = open_start + "data-node-id=\"".len();
        let id_end = svg[id_start..].find('"').map(|i| id_start + i).unwrap_or(id_start);
        let id = svg[id_start..id_end].to_string();
        // Walk forward balancing <g>/</g> tags.
        let mut depth = 0usize;
        let mut cursor = tag_start;
        let mut end = tag_start;
        while cursor < svg.len() {
            let close = svg[cursor..].find("</g>");
            let open = svg[cursor..].find("<g ");
            match (open, close) {
                (Some(oi), Some(ci)) if oi <= ci => {
                    depth += 1;
                    cursor += oi + 3;
                }
                (Some(oi), Some(ci)) => {
                    depth = depth.saturating_sub(1);
                    cursor += ci + 4;
                    if depth == 0 {
                        end = cursor;
                        break;
                    }
                }
                (Some(oi), None) => {
                    depth += 1;
                    cursor += oi + 3;
                }
                (None, Some(ci)) => {
                    depth = depth.saturating_sub(1);
                    cursor += ci + 4;
                    if depth == 0 {
                        end = cursor;
                        break;
                    }
                }
                (None, None) => break,
            }
        }
        ranges.push((id, tag_start, end));
        search_from = end.max(search_from + 1);
    }
    ranges
}

/// `transformBoundaryPairs` → rect + text pairs for structural frames.
fn transform_boundary_pairs(svg: &str, transform: &dyn Fn(&str, &str) -> String) -> String {
    let mut result = String::with_capacity(svg.len());
    let mut last = 0;
    let mut cursor = 0;
    while let Some(rel) = svg[cursor..].find("data-graph-role=\"structural-frame\"") {
        let rect_start = cursor + rel;
        // Rewind to <rect.
        let tag_start = svg[..rect_start].rfind("<rect ").or_else(|| svg[..rect_start].rfind("<rect/>")).unwrap_or(rect_start);
        let rect_end = svg[rect_start..].find("/>").map(|i| rect_start + i + 2).unwrap_or(rect_start);
        // Next <text ...>...</text>.
        let text_start = svg[rect_end..].find("<text").map(|i| rect_end + i);
        let (pair_end, kind, label) = match text_start {
            Some(text_start) => {
                let text_end = svg[text_start..].find("</text>").map(|i| text_start + i + "</text>".len());
                match text_end {
                    Some(text_end) => {
                        let kind_start = rect_start + "data-composition-frame-kind=\"".len();
                        let kind = svg[rect_start..].find("data-composition-frame-kind=\"").and_then(|i| {
                            let s = rect_start + i + "data-composition-frame-kind=\"".len();
                            let e = svg[s..].find('"')?;
                            Some(svg[s..s + e].to_string())
                        }).unwrap_or_default();
                        let label = svg[text_start..text_end]
                            .find('>')
                            .and_then(|i| {
                                let content_start = text_start + i + 1;
                                svg[content_start..text_end].find("</text>").map(|e| svg[content_start..content_start + e].to_string())
                            })
                            .unwrap_or_default();
                        (text_end, kind, label)
                    }
                    None => (rect_end, String::new(), String::new()),
                }
            }
            None => (rect_end, String::new(), String::new()),
        };
        result.push_str(&svg[last..tag_start]);
        result.push_str(&transform(&svg[tag_start..pair_end], &format!("{kind}:{label}")));
        last = pair_end;
        cursor = pair_end;
    }
    result.push_str(&svg[last..]);
    result
}

/// `annotateArchitectureSideSvg` → per-side change state markup.
fn annotate_architecture_side_svg(svg: &str, receipt: &Value, side: &str) -> String {
    let nodes = change_map(receipt.pointer("/changes/components").unwrap_or(&Value::Null));
    let edges = change_map(receipt.pointer("/changes/connections").unwrap_or(&Value::Null));
    let boundaries = boundary_change_map(receipt.pointer("/changes/boundaries").unwrap_or(&Value::Null));

    let mut result = transform_boundary_pairs(svg, &|pair, key| {
        let change = boundaries.get(key);
        if change.is_none() {
            return pair.to_string();
        }
        let change = change.unwrap();
        let rect_part = &pair[..pair.find("<text").unwrap_or(pair.len())];
        let rect_tag = rect_part.trim_end();
        let new_rect = add_state(rect_tag, Some(change), side, None).replace("/>", &format!(" data-delta-boundary-key=\"{}\"/>", esc(change.get("key").and_then(Value::as_str).unwrap_or(""))));
        let text_part = &pair[pair.find("<text").unwrap_or(pair.len())..];
        let state = change.get("status").and_then(Value::as_str).unwrap_or("same");
        let new_text = text_part.replacen('>', &format!(" data-delta-state=\"{}\" data-delta-boundary-state=\"{}\" data-delta-boundary-key=\"{}\">", esc(state), esc(state), esc(change.get("key").and_then(Value::as_str).unwrap_or(""))), 1);
        format!("{new_rect}{new_text}")
    });

    // Node groups.
    let mut rebuilt = String::with_capacity(result.len());
    let mut last = 0;
    for (id, start, end) in node_group_ranges(&result) {
        let group = &result[start..end];
        let change = nodes.get(&id);
        if (side == "base" && change.map(|c| c.get("status").and_then(Value::as_str) == Some("added")).unwrap_or(false))
            || (side == "head" && change.map(|c| c.get("status").and_then(Value::as_str) == Some("removed")).unwrap_or(false))
        {
            rebuilt.push_str(&result[last..end]);
            last = end;
            continue;
        }
        let tag_end = group.find('>').map(|i| i + 1).unwrap_or(0);
        let tagged = add_state(&group[..tag_end], change, side, None) + &group[tag_end..];
        let marker = add_node_marker(&tagged, change.map(|c| c.get("status").and_then(Value::as_str).unwrap_or("")).unwrap_or(""));
        rebuilt.push_str(&result[last..start]);
        rebuilt.push_str(&marker);
        last = end;
    }
    rebuilt.push_str(&result[last..]);
    result = rebuilt;

    // Edges.
    // Add state to every path/g carrying data-edge-id.
    let mut edge_result = String::with_capacity(result.len());
    let mut last = 0;
    for (i, _) in result.match_indices("data-edge-id=\"") {
        let tag_start = result[..i].rfind('<').unwrap_or(i);
        edge_result.push_str(&result[last..tag_start]);
        let tag_end = result[i..].find('>').map(|e| i + e + 1).unwrap_or(result.len());
        let tag = &result[tag_start..tag_end];
        let id_start = i + "data-edge-id=\"".len();
        let id_end = result[id_start..].find('"').map(|e| id_start + e).unwrap_or(id_start);
        let id = &result[id_start..id_end];
        let change = edges.get(id);
        if tag.starts_with("<path") || tag.starts_with("<g") {
            edge_result.push_str(&add_state(tag, change, side, None));
        } else {
            edge_result.push_str(tag);
        }
        last = tag_end;
    }
    edge_result.push_str(&result[last..]);
    result = edge_result;

    prefix_svg_ids(&staticize(&result), side)
}

/// `elementById` → node group or edge path/label markup for an id.
fn element_by_id(svg: &str, kind: &str, id: &str) -> String {
    if kind == "node" {
        return node_group_ranges(svg)
            .iter()
            .find(|(candidate, _, _)| candidate == id)
            .map(|(_, start, end)| svg[*start..*end].to_string())
            .unwrap_or_default();
    }
    let path_needle = format!("data-edge-id=\"{}\"", id);
    let path_start = svg.find(&path_needle);
    let path = match path_start {
        Some(i) => {
            let tag_start = svg[..i].rfind('<').unwrap_or(i);
            let tag_end = svg[i..].find("/>").map(|e| i + e + 2).unwrap_or(svg.len());
            if svg[tag_start..tag_end].starts_with("<path") {
                svg[tag_start..tag_end].to_string()
            } else {
                String::new()
            }
        }
        None => String::new(),
    };
    let label = if let Some(g_start) = svg.find(&format!("<g ")) {
        // Match `<g ... data-edge-id="{id}">...</g>` anywhere in the svg.
        let mut cursor = 0;
        let mut label = String::new();
        while let Some(rel) = svg[cursor..].find("<g ") {
            let start = cursor + rel;
            let segment = &svg[start..];
            if segment[..segment.find('>').map(|e| e + 1).unwrap_or(0)].contains(&path_needle) {
                if let Some(e) = segment.find("</g>") {
                    label = segment[..e + 4].to_string();
                    break;
                }
            }
            cursor = start + 3;
        }
        label
    } else {
        String::new()
    };
    [path, label].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

fn force_element_state(markup: &str, state: &str, classifications: &[String]) -> String {
    let change = json!({ "classifications": classifications });
    let mut result = if markup.contains("data-node-id=") {
        let tag_end = markup.find('>').map(|i| i + 1).unwrap_or(0);
        add_state(&markup[..tag_end], Some(&change), "delta", Some(state)) + &markup[tag_end..]
    } else {
        let mut out = String::new();
        let mut last = 0;
        for (i, _) in markup.match_indices("data-edge-id=\"") {
            let tag_start = markup[..i].rfind('<').unwrap_or(i);
            out.push_str(&markup[last..tag_start]);
            let tag_end = markup[i..].find('>').map(|e| i + e + 1).unwrap_or(markup.len());
            let tag = &markup[tag_start..tag_end];
            if tag.starts_with("<path") || tag.starts_with("<g") {
                out.push_str(&add_state(tag, Some(&change), "delta", Some(state)));
            } else {
                out.push_str(tag);
            }
            last = tag_end;
        }
        out.push_str(&markup[last..]);
        out
    };
    result = result.replace("id=\"node-", "id=\"base-node-");
    if markup.contains("data-node-id=") {
        result = add_node_marker(&result, state);
    }
    result
}

/// `boundaryPairByKey` → structural-frame pair whose kind:label equals key.
fn boundary_pair_by_key(svg: &str, key: &str) -> String {
    let mut found = String::new();
    let mut cursor = 0;
    while let Some(rel) = svg[cursor..].find("data-graph-role=\"structural-frame\"") {
        let rect_start = cursor + rel;
        let tag_start = svg[..rect_start].rfind("<rect ").or_else(|| svg[..rect_start].rfind("<rect/>")).unwrap_or(rect_start);
        let rect_end = svg[rect_start..].find("/>").map(|i| rect_start + i + 2).unwrap_or(rect_start);
        let text_start = svg[rect_end..].find("<text").map(|i| rect_end + i);
        let Some(text_start) = text_start else { break };
        let text_end = svg[text_start..].find("</text>").map(|i| text_start + i + "</text>".len());
        let Some(text_end) = text_end else { break };
        let kind = svg[rect_start..].find("data-composition-frame-kind=\"").and_then(|i| {
            let s = rect_start + i + "data-composition-frame-kind=\"".len();
            let e = svg[s..].find('"')?;
            Some(svg[s..s + e].to_string())
        }).unwrap_or_default();
        let label = svg[text_start..text_end].find('>').and_then(|i| {
            let content_start = text_start + i + 1;
            svg[content_start..text_end].find("</text>").map(|e| svg[content_start..content_start + e].to_string())
        }).unwrap_or_default();
        if format!("{kind}:{label}") == key {
            found = svg[tag_start..text_end].to_string();
            break;
        }
        cursor = text_end;
    }
    found
}

fn force_boundary_state(markup: &str, state: &str, key: &str, classifications: &[String]) -> String {
    let change = json!({ "classifications": classifications });
    let rect_tag = &markup[..markup.find("<text").unwrap_or(markup.len())].trim_end();
    let new_rect = add_state(rect_tag, Some(&change), "delta", Some(state)).replace("/>", &format!(" data-delta-boundary-key=\"{}\"/>", esc(key)));
    let text_part = &markup[markup.find("<text").unwrap_or(markup.len())..];
    let new_text = text_part.replacen('>', &format!(" data-delta-state=\"{}\" data-delta-boundary-state=\"{}\" data-delta-boundary-key=\"{}\">", esc(state), esc(state), esc(key)), 1);
    format!("{new_rect}{new_text}")
}

fn view_box_size(svg: &str) -> (f64, f64) {
    let marker = "viewBox=\"0 0 ";
    match svg.find(marker) {
        Some(i) => {
            let rest = &svg[i + marker.len()..];
            let parts: Vec<&str> = rest.split_whitespace().take(2).collect();
            if parts.len() == 2 {
                (parts[0].parse().unwrap_or(0.0), parts[1].trim_end_matches('"').parse().unwrap_or(0.0))
            } else {
                (0.0, 0.0)
            }
        }
        None => (0.0, 0.0),
    }
}

fn edge_symbol_markup(markup: &str, state: &str) -> String {
    let symbol = marker_for(state);
    let points = markup.find("data-composition-points=\"");
    let edge_id = markup.find("data-edge-id=\"");
    let (Some(symbol), Some(points)) = (if symbol.is_empty() { None } else { Some(symbol) }, points) else { return String::new() };
    let p_start = points + "data-composition-points=\"".len();
    let rest = &markup[p_start..];
    let coords: Vec<f64> = rest
        .split(|c: char| c == ',' || c == ' ' || c == '"')
        .filter_map(|part| part.parse().ok())
        .take(2)
        .collect();
    if coords.len() < 2 {
        return String::new();
    }
    let id_attr = edge_id.map(|i| {
        let s = i + "data-edge-id=\"".len();
        let e = markup[s..].find('"').map(|e| s + e).unwrap_or(s);
        format!(" data-edge-id=\"{}\"", esc(&markup[s..e]))
    }).unwrap_or_default();
    format!(
        "<text class=\"delta-edge-marker\" data-delta-state=\"{}\"{} x=\"{}\" y=\"{}\" aria-hidden=\"true\">{}</text>",
        esc(state),
        id_attr,
        num(coords[0] + 9.0),
        num(coords[1] - 7.0),
        symbol
    )
}

fn boundary_symbol_markup(markup: &str, state: &str) -> String {
    let symbol = marker_for(state);
    let frame = markup.find("width=\"");
    let (Some(symbol), Some(frame)) = (if symbol.is_empty() { None } else { Some(symbol) }, frame) else { return String::new() };
    let x = read_attr_number(markup, 0, "x=\"").unwrap_or(0.0);
    let y = read_attr_number(markup, 0, "y=\"").unwrap_or(0.0);
    let width = read_attr_number(markup, frame, "width=\"").unwrap_or(0.0);
    format!(
        "<text class=\"delta-boundary-marker\" data-delta-state=\"{}\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" aria-hidden=\"true\">{}</text>",
        esc(state),
        num(x + width - 12.0),
        num(y + 16.0),
        symbol
    )
}

/// `buildDeltaSvg` → annotated head SVG with baseline phantoms and markers.
fn build_delta_svg(base_svg: &str, head_svg: &str, receipt: &Value) -> String {
    let (base_w, base_h) = view_box_size(base_svg);
    let (head_w, head_h) = view_box_size(head_svg);
    let nodes = change_map(receipt.pointer("/changes/components").unwrap_or(&Value::Null));
    let edges = change_map(receipt.pointer("/changes/connections").unwrap_or(&Value::Null));
    let boundaries = boundary_change_map(receipt.pointer("/changes/boundaries").unwrap_or(&Value::Null));

    let mut base_node_phantoms: Vec<String> = Vec::new();
    let mut base_edge_phantoms: Vec<String> = Vec::new();
    let mut base_boundary_phantoms: Vec<String> = Vec::new();
    let mut edge_markers: Vec<String> = Vec::new();
    let mut boundary_markers: Vec<String> = Vec::new();

    for change in nodes.values() {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        let classifications: Vec<String> = change
            .get("classifications")
            .and_then(Value::as_array)
            .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let id = change.get("id").and_then(Value::as_str).unwrap_or("");
        if status == "removed" {
            base_node_phantoms.push(force_element_state(&element_by_id(base_svg, "node", id), "removed", &classifications));
        } else if classifications.iter().any(|c| c == "geometry") {
            base_node_phantoms.push(force_element_state(&element_by_id(base_svg, "node", id), "moved-from", &classifications));
        }
    }
    for change in edges.values() {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        let classifications: Vec<String> = change
            .get("classifications")
            .and_then(Value::as_array)
            .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let id = change.get("id").and_then(Value::as_str).unwrap_or("");
        if status == "removed" || classifications.iter().any(|c| c == "topology") {
            let phantom = force_element_state(&element_by_id(base_svg, "edge", id), "removed", &classifications);
            base_edge_phantoms.push(phantom.clone());
            edge_markers.push(edge_symbol_markup(&phantom, "removed"));
        } else if classifications.iter().any(|c| c == "geometry") {
            let phantom = force_element_state(&element_by_id(base_svg, "edge", id), "moved-from", &classifications);
            base_edge_phantoms.push(phantom.clone());
            edge_markers.push(edge_symbol_markup(&phantom, "moved-from"));
        }
    }
    for change in boundaries.values() {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        let kind = change.get("kind").and_then(Value::as_str).unwrap_or("");
        let label = change.get("label").and_then(Value::as_str).unwrap_or("");
        let key = change.get("key").and_then(Value::as_str).unwrap_or("");
        let classifications: Vec<String> = change
            .get("classifications")
            .and_then(Value::as_array)
            .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let rendered_key = format!("{kind}:{}", esc(label));
        if status == "removed" {
            let phantom = force_boundary_state(&boundary_pair_by_key(base_svg, &rendered_key), "removed", key, &classifications);
            base_boundary_phantoms.push(phantom.clone());
            boundary_markers.push(boundary_symbol_markup(&phantom, "removed"));
        } else if status == "changed" || status == "geometry-changed" {
            base_boundary_phantoms.push(force_boundary_state(&boundary_pair_by_key(base_svg, &rendered_key), "moved-from", key, &classifications));
        }
    }

    let mut delta = annotate_architecture_side_svg(head_svg, receipt, "head");
    let view_box = format!("viewBox=\"0 0 {} {}\"", num(base_w.max(head_w) + 24.0), num(base_h.max(head_h) + 24.0));
    if let Some(i) = delta.find("viewBox=\"") {
        let end = delta[i..].find('"').map(|e| i + e).unwrap_or(i);
        let end = delta[end + 1..].find('"').map(|e| end + 1 + e).unwrap_or(delta.len());
        delta.replace_range(i..end + 1, &view_box);
    }

    let boundary_anchor = "        <!-- Boundaries (behind everything) -->";
    if let Some(i) = delta.find(boundary_anchor) {
        let insertion = format!("        <!-- Baseline boundary phantoms -->\n{}\n\n{}", base_boundary_phantoms.join("\n"), boundary_anchor);
        delta.replace_range(i..i + boundary_anchor.len(), &insertion);
    }
    let edge_anchor = "        <!-- Connection paths (before components for correct z-order) -->";
    if let Some(i) = delta.find(edge_anchor) {
        let insertion = format!("        <!-- Baseline relationship phantoms -->\n{}\n\n{}", base_edge_phantoms.join("\n"), edge_anchor);
        delta.replace_range(i..i + edge_anchor.len(), &insertion);
    }
    let component_anchor = "        <!-- Components -->";
    if let Some(i) = delta.find(component_anchor) {
        let insertion = format!("        <!-- Baseline removed and move-from component phantoms -->\n{}\n\n{}", base_node_phantoms.join("\n"), component_anchor);
        delta.replace_range(i..i + component_anchor.len(), &insertion);
    }

    for change in edges.values() {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        let classifications: Vec<String> = change
            .get("classifications")
            .and_then(Value::as_array)
            .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let id = change.get("id").and_then(Value::as_str).unwrap_or("");
        if ["added", "changed", "rerouted"].contains(&status) {
            let current = element_by_id(&delta, "edge", id);
            let marker_state = if status == "changed" && classifications.iter().any(|c| c == "topology") { "added" } else { status };
            edge_markers.push(edge_symbol_markup(&current, marker_state));
        }
    }
    for change in boundaries.values() {
        let status = change.get("status").and_then(Value::as_str).unwrap_or("");
        if !["added", "changed", "geometry-changed"].contains(&status) {
            continue;
        }
        let kind = change.get("kind").and_then(Value::as_str).unwrap_or("");
        let label = change.get("label").and_then(Value::as_str).unwrap_or("");
        let rendered_key = format!("{kind}:{}", esc(label));
        boundary_markers.push(boundary_symbol_markup(&boundary_pair_by_key(&delta, &rendered_key), status));
    }

    let legend_anchor = "        <!-- Legend -->";
    if let Some(i) = delta.find(legend_anchor) {
        let insertion = format!(
            "        <!-- Delta relationship symbols -->\n{}\n\n        <!-- Delta boundary symbols -->\n{}\n\n{}",
            edge_markers.iter().filter(|m| !m.is_empty()).cloned().collect::<Vec<_>>().join("\n"),
            boundary_markers.iter().filter(|m| !m.is_empty()).cloned().collect::<Vec<_>>().join("\n"),
            legend_anchor
        );
        delta.replace_range(i..i + legend_anchor.len(), &insertion);
    }

    prefix_svg_ids(&staticize(&delta), "delta")
}

// ---------------------------------------------------------------------------
// Change rows + HTML rendering
// ---------------------------------------------------------------------------

fn architecture_delta_change_rows(receipt: &Value) -> Vec<Value> {
    let mut combined: Vec<Value> = Vec::new();
    
    // Collect component changes with metadata.
    if let Some(changes) = receipt.pointer("/changes/components").and_then(Value::as_array) {
        for change in changes {
            let id = change.get("id").and_then(Value::as_str).unwrap_or("");
            let mut row = change.clone();
            if let Value::Object(map) = &mut row {
                map.insert("kind".to_string(), Value::String("Component".to_string()));
                map.insert("kindKey".to_string(), Value::String("component".to_string()));
                map.insert("key".to_string(), Value::String(format!("component:{id}")));
            }
            combined.push(row);
        }
    }
    
    // Collect connection changes with metadata.
    if let Some(changes) = receipt.pointer("/changes/connections").and_then(Value::as_array) {
        for change in changes {
            let id = change.get("id").and_then(Value::as_str).unwrap_or("");
            let mut row = change.clone();
            if let Value::Object(map) = &mut row {
                map.insert("kind".to_string(), Value::String("Relationship".to_string()));
                map.insert("kindKey".to_string(), Value::String("relationship".to_string()));
                map.insert("key".to_string(), Value::String(format!("relationship:{id}")));
            }
            combined.push(row);
        }
    }
    
    // Collect boundary changes with metadata.
    if let Some(changes) = receipt.pointer("/changes/boundaries").and_then(Value::as_array) {
        for change in changes {
            let key = change.get("key").and_then(Value::as_str).unwrap_or("");
            let mut row = change.clone();
            if let Value::Object(map) = &mut row {
                map.insert("kind".to_string(), Value::String("Boundary".to_string()));
                map.insert("kindKey".to_string(), Value::String("boundary".to_string()));
                map.insert("key".to_string(), Value::String(format!("boundary:{key}")));
                if !map.contains_key("id") {
                    map.insert("id".to_string(), Value::String(key.to_string()));
                }
            }
            combined.push(row);
        }
    }
    
    // Sort by status, kind, id for stable ordering.
    combined.sort_by(|a, b| {
        let a_key = format!(
            "{}:{}:{}",
            a.get("status").and_then(Value::as_str).unwrap_or(""),
            a.get("kind").and_then(Value::as_str).unwrap_or(""),
            a.get("id").and_then(Value::as_str).unwrap_or("")
        );
        let b_key = format!(
            "{}:{}:{}",
            b.get("status").and_then(Value::as_str).unwrap_or(""),
            b.get("kind").and_then(Value::as_str).unwrap_or(""),
            b.get("id").and_then(Value::as_str).unwrap_or("")
        );
        a_key.cmp(&b_key)
    });
    combined
}

fn review_primary_states(row: &Value) -> Vec<String> {
    let kind_key = row.get("kindKey").and_then(Value::as_str).unwrap_or("");
    let status = row.get("status").and_then(Value::as_str).unwrap_or("");
    let classifications: Vec<String> = row
        .get("classifications")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if kind_key == "component" && classifications.iter().any(|c| c == "geometry") {
        return vec![status.to_string(), "moved-from".to_string()];
    }
    if kind_key == "relationship" && status == "changed" && classifications.iter().any(|c| c == "topology") {
        return vec!["changed".to_string(), "removed".to_string()];
    }
    if kind_key == "relationship" && classifications.iter().any(|c| c == "geometry") {
        return vec!["moved-from".to_string(), status.to_string()];
    }
    if kind_key == "boundary" && ["changed", "geometry-changed"].contains(&status) {
        let mut states = vec![status.to_string(), "moved-from".to_string()];
        states.sort();
        return states;
    }
    vec![status.to_string()]
}

/// `expectedReviewTargetSignature` → server-side signature the client verifies.
fn expected_review_target_signature(row: &Value) -> String {
    let classifications: Vec<String> = row
        .get("classifications")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let classifications_text = classifications.join(",");
    let mut descriptors: Vec<String> = Vec::new();
    let kind_key = row.get("kindKey").and_then(Value::as_str).unwrap_or("");
    let status = row.get("status").and_then(Value::as_str).unwrap_or("");
    if kind_key == "component" {
        for state in review_primary_states(row) {
            descriptors.push(format!("g:{state}:{classifications_text}"));
        }
    } else if kind_key == "boundary" {
        for state in review_primary_states(row) {
            descriptors.push(format!("rect:{state}:{classifications_text}"));
            descriptors.push(format!("text:{state}:"));
        }
    } else {
        let base_label = row.pointer("/base/label").and_then(Value::as_str).unwrap_or("");
        let head_label = row.pointer("/head/label").and_then(Value::as_str).unwrap_or("");
        let forms: Vec<(&str, &str, &str)> = if status == "added" {
            vec![("added", "added", head_label)]
        } else if status == "removed" {
            vec![("removed", "removed", base_label)]
        } else if classifications.iter().any(|c| c == "topology") {
            vec![("removed", "removed", base_label), ("changed", "added", head_label)]
        } else if classifications.iter().any(|c| c == "geometry") {
            vec![("moved-from", "moved-from", base_label), (status, status, head_label)]
        } else {
            vec![("changed", "changed", head_label)]
        };
        for (state, marker, label) in forms {
            descriptors.push(format!("path:{state}:{classifications_text}"));
            descriptors.push(format!("text:{marker}:"));
            if !label.is_empty() {
                descriptors.push(format!("g:{state}:{classifications_text}"));
            }
        }
    }
    descriptors.sort();
    descriptors.join("|")
}

fn total(receipt: &Value, key: &str) -> u64 {
    ["components", "connections", "boundaries"]
        .iter()
        .map(|collection| receipt.pointer(&format!("/summary/{collection}/{key}")).and_then(Value::as_u64).unwrap_or(0))
        .sum()
}

fn render_change_row(row: &Value, index: usize) -> String {
    let status = row.get("status").and_then(Value::as_str).unwrap_or("");
    let key = row.get("key").and_then(Value::as_str).unwrap_or("");
    let kind_key = row.get("kindKey").and_then(Value::as_str).unwrap_or("");
    let id = row.get("id").and_then(Value::as_str).unwrap_or("");
    let label = row
        .get("headLabel")
        .or_else(|| row.get("baseLabel"))
        .or_else(|| row.pointer("/head/label"))
        .or_else(|| row.pointer("/base/label"))
        .or_else(|| row.get("label"))
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let marker = marker_for(&status);
    let marker = if marker.is_empty() { "~" } else { marker };
    let kind = row.get("kind").and_then(Value::as_str).unwrap_or("");
    let classifications = row
        .get("classifications")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let changed_fields = row
        .get("changedFields")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let changed_fields = if changed_fields.is_empty() { "identity".to_string() } else { changed_fields };
    let signature = expected_review_target_signature(row);
    format!(
        "<li data-change-status=\"{}\"><button class=\"change-row\" type=\"button\" data-change-index=\"{}\" data-change-key=\"{}\" data-change-kind=\"{}\" data-change-id=\"{}\" data-change-label=\"{}\" data-change-status=\"{}\" data-change-classifications=\"{}\" data-change-target-signature=\"{}\"><span class=\"token\">{}</span><span>{}</span><strong>{}</strong><code>{}</code><span>{}</span><span>{}</span></button></li>",
        esc(&status),
        index,
        esc(&key),
        esc(&kind_key),
        esc(&id),
        esc(&label),
        esc(&status),
        esc(&classifications),
        esc(&signature),
        esc(marker),
        esc(&kind),
        esc(&label),
        esc(&id),
        esc(&classifications),
        esc(&changed_fields)
    )
}

/// `renderArchitectureDeltaHtml` → standalone review page.
fn render_delta_html(receipt: &Value, base_svg: &str, delta_svg: &str, head_svg: &str, artifact_css: &str) -> String {
    let rows = architecture_delta_change_rows(receipt);
    let changed = total(receipt, "changed");
    let proof = if receipt.get("proofLevel").and_then(Value::as_str) == Some("revision-pinned") { "REVISION-PINNED INPUTS" } else { "AUTHORED SNAPSHOTS" };
    let row_html = if rows.is_empty() {
        "<li class=\"empty\">No authored architecture changes.</li>".to_string()
    } else {
        rows.iter().enumerate().map(|(index, row)| render_change_row(row, index)).collect::<Vec<_>>().join("\n")
    };
    let base_title = receipt.pointer("/base/title").and_then(Value::as_str).unwrap_or("");
    let head_title = receipt.pointer("/head/title").and_then(Value::as_str).unwrap_or("");
    let preset = receipt.pointer("/view/visualPreset").and_then(Value::as_str).unwrap_or("classic");
    let rows_count = rows.len();
    let play_disabled = if rows.is_empty() { " disabled" } else { "" };
    let details_open = if rows_count <= 10 { " open" } else { "" };
    let mut html = DELTA_TEMPLATE
        .replace("__ARCHIFY_DELTA_PROOF__", &esc(proof))
        .replace("__ARCHIFY_DELTA_SUBTITLE__", &esc(&format!("{base_title} → {head_title}")))
        .replace("__ARCHIFY_DELTA_ADDED__", &total(receipt, "added").to_string())
        .replace("__ARCHIFY_DELTA_REMOVED__", &total(receipt, "removed").to_string())
        .replace("__ARCHIFY_DELTA_CHANGED__", &changed.to_string())
        .replace("__ARCHIFY_DELTA_PLAY_DISABLED__", play_disabled)
        .replace("__ARCHIFY_DELTA_DETAILS_OPEN__", details_open)
        .replace("__ARCHIFY_DELTA_BASE_VIEW__", base_svg)
        .replace("__ARCHIFY_DELTA_DELTA_SVG__", delta_svg)
        .replace("__ARCHIFY_DELTA_HEAD_VIEW__", head_svg)
        .replace("__ARCHIFY_DELTA_ROWS__", &row_html)
        .replace("__ARCHIFY_DELTA_ROWS_COUNT__", &rows_count.to_string())
        .replace("__ARCHIFY_DELTA_RECEIPT_JSON__", &safe_json(receipt))
        .replace("__ARCHIFY_DELTA_EXPORT_CSS__", &safe_json(&Value::String(artifact_css.to_string())))
        .replace("__ARCHIFY_DELTA_PRESET__", &esc(preset))
        .replace("__ARCHIFY_DELTA_HEAD_TITLE__", &esc(head_title))
        .replace("__ARCHIFY_DELTA_ARTIFACT_CSS__", artifact_css);
    // Strip trailing whitespace per line, like the original `html.replace(/[ \t]+$/gm, '')`.
    html = html
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n");
    html
}

/// Full compare pipeline: render both snapshots, build the delta page.
pub fn compare_files(base: &Path, head: &Path, output: &Path) -> Result<Value> {
    let read_ir = |path: &Path| -> Result<Value> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read input file {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("input {} is not valid JSON", path.display()))
    };
    let base_ir = read_ir(base)?;
    let head_ir = read_ir(head)?;

    // Render canonical snapshots for deterministic geometry.
    let canonical_base = canonical_architecture(&base_ir);
    let canonical_head = canonical_architecture(&head_ir);
    let base_text = serde_json::to_string_pretty(&canonical_base)?;
    let head_text = serde_json::to_string_pretty(&canonical_head)?;
    let base_value: Value = serde_json::from_str(&base_text)?;
    let head_value: Value = serde_json::from_str(&head_text)?;

    let base_svg = renderer::architecture::render_svg(&base_value)?;
    let head_svg = renderer::architecture::render_svg(&head_value)?;

    // The delta page embeds the artifact stylesheet; the template's `<style>`
    // block is identical in every rendered artifact.
    let artifact_css = extract_artifact_css(crate::template::TEMPLATE_HTML)?;

    let receipt = compare_architecture(&base_ir, &head_ir)?;
    let delta_svg = build_delta_svg(&base_svg, &head_svg, &receipt);
    let annotated_base = annotate_architecture_side_svg(&base_svg, &receipt, "base");
    let annotated_head = annotate_architecture_side_svg(&head_svg, &receipt, "head");
    let html = render_delta_html(&receipt, &annotated_base, &delta_svg, &annotated_head, &artifact_css);

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(output, html).with_context(|| format!("cannot write output file {}", output.display()))?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_matches_expected_shape() {
        let text = include_str!("../examples/checkout-platform.base.architecture.json");
        let diagram: Value = serde_json::from_str(text).unwrap();
        let canonical = canonical_architecture(&diagram);
        assert_eq!(canonical["schema_version"], 1);
        assert_eq!(canonical["diagram_type"], "architecture");
        assert!(canonical["components"].is_array());
    }

    #[test]
    fn compares_base_head_examples() {
        let base: Value = serde_json::from_str(include_str!("../examples/checkout-platform.base.architecture.json")).unwrap();
        let head: Value = serde_json::from_str(include_str!("../examples/checkout-platform.head.architecture.json")).unwrap();
        let receipt = compare_architecture(&base, &head).unwrap();
        assert_eq!(receipt["command"], "compare");
        assert_eq!(receipt["proofLevel"], "authored");
        assert!(receipt.pointer("/changes/components").and_then(Value::as_array).unwrap().len() >= 1);
    }

    #[test]
    fn rejects_mismatched_types() {
        let base: Value = serde_json::from_str(include_str!("../examples/checkout-platform.base.architecture.json")).unwrap();
        let mut head = base.clone();
        head["diagram_type"] = Value::String("workflow".to_string());
        assert!(compare_architecture(&base, &head).is_err());
    }

    #[test]
    fn extracts_svg_and_css() {
        let svg = "<svg viewBox=\"0 0 100 100\" role=\"img\"><g data-node-id=\"a\"><rect x=\"1\" y=\"2\" width=\"3\" height=\"4\"/></g></svg>";
        let css = extract_artifact_css("<html><style>body{}</style></html>").unwrap();
        assert_eq!(css, "body{}");
        let extracted = extract_architecture_svg(&format!("<html>{svg}</html>")).unwrap();
        assert!(extracted.starts_with("<svg"));
    }

    #[test]
    fn annotations_are_namespaced() {
        let svg = "<svg viewBox=\"0 0 100 100\" role=\"img\"><defs><marker id=\"arrowhead\"/></defs><g data-node-id=\"a\"><rect x=\"1\" y=\"2\" width=\"3\" height=\"4\"/></g><path data-edge-id=\"e1\" data-composition-points=\"1,2 3,4\"/></svg>";
        let receipt = json!({
            "changes": {
                "components": [{"id": "a", "status": "added", "classifications": ["semantic"], "changedFields": ["/label"]}],
                "connections": [],
                "boundaries": []
            }
        });
        let annotated = annotate_architecture_side_svg(&svg, &receipt, "head");
        assert!(annotated.contains("data-delta-state=\"added\""));
        assert!(annotated.contains("id=\"head-arrowhead\""));
    }
}
