//! Renderer — computes diagram SVG from validated JSON IR and injects it into
//! the embedded front-end template.
//!
//! The SVG generation is a faithful port of the original Archify renderer
//! scripts (`renderers/*/render-*.mjs`): same geometry, same element classes,
//! same sentinel slots, so output is visually pixel-identical to the Node.js
//! pipeline while requiring no runtime beyond the binary itself.

pub mod architecture;
pub mod dataflow;
pub mod geometry;
pub mod legend;
pub mod lifecycle;
pub mod sequence;
pub mod workflow;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::template::{apply_template, render_cards, esc, TemplateData, TEMPLATE_HTML};
use crate::validator;

pub const DIAGRAM_TYPES: [&str; 5] = ["architecture", "workflow", "sequence", "dataflow", "lifecycle"];

/// Anchor right after the template's theme-resolution script; used only when
/// `--theme` is passed, never for the default render path.
const THEME_ANCHOR: &str = "  <!-- Async font load: a blackholed network must not block first paint.";

fn render_svg(diagram_type: &str, ir: &Value) -> Result<String> {
    match diagram_type {
        "architecture" => architecture::render_svg(ir),
        "sequence" => sequence::render_svg(ir),
        "workflow" => workflow::render_svg(ir),
        "lifecycle" => lifecycle::render_svg(ir),
        "dataflow" => dataflow::render_svg(ir),
        other => bail!("unknown diagram type \"{other}\". Expected one of: {}", DIAGRAM_TYPES.join(", ")),
    }
}

/// The `writeDiagram` port: fill the template and write the standalone file.
fn write_diagram(
    out_path: &Path,
    diagram_type: &str,
    meta: &Value,
    footer_label: &str,
    svg: &str,
    cards: &Value,
    theme: Option<&str>,
) -> Result<()> {
    if !DIAGRAM_TYPES.contains(&diagram_type) {
        bail!("writeDiagram: unknown diagram type {diagram_type:?}");
    }
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create output directory {}", parent.display()))?;
        }
    }
    let guided_hint = meta
        .get("views")
        .and_then(Value::as_array)
        .filter(|views| !views.is_empty())
        .map(|_| " &bull; <kbd>[</kbd>/<kbd>]</kbd> views &bull; <kbd>P</kbd> play story")
        .unwrap_or("");
    let start_url = format!(
        "https://tt-a1i.github.io/archify/start.html?type={}&amp;source=artifact",
        esc(diagram_type)
    );
    let footer = format!(
        "{footer_label} &bull; Built with Archify<span class=\"no-print\"> &bull; <a class=\"artifact-start-link\" href=\"{start_url}\" target=\"_blank\" rel=\"noopener noreferrer\">Create yours &nearr;</a> &bull; Hover to trace &bull; <kbd>R</kbd> route &bull; Click to focus &bull; <kbd>+</kbd>/<kbd>&minus;</kbd> zoom &bull; <kbd>M</kbd> radar{guided_hint} &bull; <kbd>T</kbd> theme &bull; <kbd>E</kbd> export</span>"
    );
    let title = meta.get("title").and_then(Value::as_str).unwrap_or("Untitled");
    let subtitle = meta.get("subtitle").and_then(Value::as_str);
    let visual_preset = meta.get("visual_preset").and_then(Value::as_str).unwrap_or("classic");
    let guided_views = meta.get("views").cloned().unwrap_or(Value::Null);

    let mut template = TEMPLATE_HTML.to_string();
    if let Some(theme) = theme {
        // Validate theme to prevent XSS injection
        if theme != "dark" && theme != "light" {
            bail!("invalid theme: {theme:?}. Expected \"dark\" or \"light\"");
        }
        // The template resolves the theme before first paint (URL param,
        // localStorage, then prefers-color-scheme). When the user pins a theme
        // via `--theme`, mirror that choice into the same mechanism the
        // toolbar re-applies later, so no template code changes.
        if let Some(index) = template.find(THEME_ANCHOR) {
            let script = format!(
                "  <script>try{{document.documentElement.setAttribute('data-theme','{theme}');localStorage.setItem('archify-theme','{theme}');}}catch(e){{}}</script>\n"
            );
            template.insert_str(index, &script);
        }
    }

    let html = apply_template(
        &template,
        &TemplateData {
            title,
            subtitle,
            footer: &footer,
            svg,
            cards: &render_cards(cards),
            visual_preset,
            guided_views: &guided_views,
            source_evidence: None,
        },
    )
    .map_err(anyhow::Error::msg)?;
    std::fs::write(out_path, html).with_context(|| format!("cannot write output file {}", out_path.display()))?;
    Ok(())
}

/// Maximum input file size (100 MB) to prevent memory exhaustion attacks.
const MAX_INPUT_SIZE: usize = 100 * 1024 * 1024;

/// Render a JSON IR file into a standalone HTML diagram.
pub fn render_file(input: &Path, output: &Path, diagram_type: &str, theme: Option<&str>) -> Result<PathBuf> {
    let metadata = std::fs::metadata(input)
        .with_context(|| format!("cannot access input file {}", input.display()))?;
    if metadata.len() > MAX_INPUT_SIZE as u64 {
        bail!(
            "input file {} is too large ({} bytes). Maximum allowed: {} bytes",
            input.display(),
            metadata.len(),
            MAX_INPUT_SIZE
        );
    }
    let text = std::fs::read_to_string(input)
        .with_context(|| format!("cannot read input file {}", input.display()))?;
    let ir: Value = serde_json::from_str(&text)
        .with_context(|| format!("input {} is not valid JSON", input.display()))?;
    validator::validate_value(diagram_type, &ir)?;
    let svg = render_svg(diagram_type, &ir)?;
    let meta = ir.get("meta").cloned().unwrap_or_else(|| serde_json::json!({}));
    let cards = ir.get("cards").cloned().unwrap_or(Value::Null);
    let footer_label = match diagram_type {
        "architecture" => "Architecture diagram",
        "workflow" => "Workflow diagram",
        "sequence" => "Sequence diagram",
        "lifecycle" => "Lifecycle diagram",
        "dataflow" => "Data-flow diagram",
        _ => "Diagram",
    };
    write_diagram(output, diagram_type, &meta, footer_label, &svg, &cards, theme)?;
    Ok(output.to_path_buf())
}

/// `svgRootAttrs` — root SVG data attributes from meta.
pub fn svg_root_attrs(meta: &Value, _kind: &str) -> String {
    let animation = if meta.get("animation").and_then(Value::as_str) == Some("trace") { " data-animation=\"trace\"" } else { "" };
    let preset = format!(" data-preset=\"{}\"", esc(meta.get("visual_preset").and_then(Value::as_str).unwrap_or("classic")));
    let engineering_profile = meta
        .get("engineering_profile")
        .and_then(Value::as_str)
        .map(|profile| format!(" data-engineering-profile=\"{}\"", esc(profile)))
        .unwrap_or_default();
    let quality_profile = if meta.get("quality_profile").and_then(Value::as_str) == Some("showcase") { "showcase" } else { "standard" };
    format!(
        "role=\"img\" aria-labelledby=\"archify-diagram-title archify-diagram-description\"{animation}{preset}{engineering_profile} data-quality-profile=\"{quality_profile}\""
    )
}

/// `svgAccessibleText` — accessible title/desc inside the SVG.
pub fn svg_accessible_text(meta: &Value, kind: &str) -> String {
    let description = match meta.get("subtitle").and_then(Value::as_str) {
        Some(subtitle) => subtitle.to_string(),
        None => format!("A {kind} generated by Archify."),
    };
    format!(
        "        <title id=\"archify-diagram-title\">{}</title>\n        <desc id=\"archify-diagram-description\">{}</desc>",
        esc(meta.get("title").and_then(Value::as_str).unwrap_or("Untitled")),
        esc(&description)
    )
}

/// `animateAttr` — trace animation step bookkeeping (capped at 12).
pub fn animate_attr(meta: &Value, kind: &str, step: Option<f64>) -> String {
    if meta.get("animation").and_then(Value::as_str) != Some("trace") {
        return String::new();
    }
    let safe_step = match step {
        Some(step) if step.is_finite() && step >= 0.0 => (step.floor().min(12.0)) as u64,
        _ => 0,
    };
    format!(" data-animate=\"{kind}\" style=\"--step:{safe_step}\"")
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty()).map(|s| s.to_string())
}

/// `focusNodeAttrs` — semantic hooks for the standalone HTML explorer.
pub fn focus_node_attrs(id: &str, label: &str, kind: Option<&str>, sublabel: Option<&str>, tag: Option<&str>, context: Option<&str>) -> String {
    let kind = clean_optional(kind);
    let sublabel = clean_optional(sublabel);
    let tag = clean_optional(tag);
    let context = clean_optional(context);
    let mut optional = String::new();
    for (name, value) in [
        ("data-node-kind", kind.as_deref()),
        ("data-node-sublabel", sublabel.as_deref()),
        ("data-node-tag", tag.as_deref()),
        ("data-node-context", context.as_deref()),
    ] {
        if let Some(value) = value {
            optional.push_str(&format!(" {name}=\"{}\"", esc(value)));
        }
    }
    let detail = [sublabel.as_deref(), context.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    let aria = if detail.is_empty() {
        format!("Focus {label}")
    } else {
        format!("Focus {label}, {detail}")
    };
    format!(
        "id=\"node-{}\" data-node-id=\"{}\" data-node-label=\"{}\" tabindex=\"0\" role=\"button\" aria-label=\"{}\" aria-pressed=\"false\"{optional}",
        esc(id),
        esc(id),
        esc(label),
        esc(&aria)
    )
}

/// `focusNodeTitle` — compact native SVG title fallback.
pub fn focus_node_title(label: &str, sublabel: Option<&str>, context: Option<&str>, tag: Option<&str>) -> String {
    let parts: Vec<String> = [label.to_string(), sublabel.unwrap_or_default().to_string(), context.unwrap_or_default().to_string(), tag.unwrap_or_default().to_string()]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    format!("<title>{}</title>", esc(&parts.join(" · ")))
}

/// `focusEdgeAttrs` — semantic hooks for connection paths/labels.
pub fn focus_edge_attrs(from: &str, to: &str, label: Option<&str>, key: Option<usize>, id: Option<&str>) -> String {
    let mut out = format!("data-edge-from=\"{}\" data-edge-to=\"{}\"", esc(from), esc(to));
    if let Some(label) = label {
        out.push_str(&format!(" data-edge-label=\"{}\"", esc(label)));
    }
    if let Some(key) = key {
        out.push_str(&format!(" data-edge-key=\"{key}\""));
    }
    if let Some(id) = id.filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!(" data-edge-id=\"{}\"", esc(id)));
    }
    out
}

/// `renderDefinitions` — shared SVG defs (arrowheads + grid pattern).
pub fn render_definitions() -> &'static str {
    "        <!-- Definitions -->
        <defs>
          <marker id=\"arrowhead\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">
            <polygon points=\"0 0, 10 3.5, 0 7\" class=\"m-default\" />
          </marker>
          <marker id=\"arrowhead-emphasis\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">
            <polygon points=\"0 0, 10 3.5, 0 7\" class=\"m-emphasis\" />
          </marker>
          <marker id=\"arrowhead-security\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">
            <polygon points=\"0 0, 10 3.5, 0 7\" class=\"m-security\" />
          </marker>
          <marker id=\"arrowhead-dashed\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">
            <polygon points=\"0 0, 10 3.5, 0 7\" class=\"m-dashed\" />
          </marker>
          <pattern id=\"grid\" width=\"40\" height=\"40\" patternUnits=\"userSpaceOnUse\">
            <path d=\"M 40 0 L 0 0 0 40\" class=\"c-grid\" stroke-width=\"0.5\"/>
          </pattern>
        </defs>"
}

/// `renderSemanticSigil` — renderer-owned role stamp glyphs.
pub fn render_semantic_sigil(kind: &str, x: f64, y: f64, size: f64) -> String {
    let normalized = if SIGIL_SHAPES.contains_key(kind) { kind } else { "neutral" };
    let tone = match normalized {
        "frontend" | "start" => "frontend",
        "backend" | "active" => "backend",
        "database" | "success" => "database",
        "cloud" | "waiting" => "cloud",
        "security" | "failure" => "security",
        "messagebus" => "messagebus",
        "external" | "neutral" => "external",
        _ => "external",
    };
    let scale = size / 16.0;
    let shape = SIGIL_SHAPES.get(normalized).copied().unwrap_or(SIGIL_SHAPES["neutral"]);
    format!(
        "<g aria-hidden=\"true\" data-semantic-sigil=\"{}\" class=\"semantic-sigil s-{}\" transform=\"translate({} {}) scale({})\">\n            {}\n          </g>",
        esc(normalized),
        tone,
        geometry::num(x),
        geometry::num(y),
        geometry::num(scale),
        shape
    )
}

use std::collections::HashMap;
use once_cell::sync::Lazy;

static SIGIL_SHAPES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "frontend",
        "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"2\"/>\n            <path d=\"M2 6.5h12\"/>\n            <circle cx=\"4.1\" cy=\"4.8\" r=\".7\" class=\"sigil-fill\"/>\n            <circle cx=\"6.3\" cy=\"4.8\" r=\".7\" class=\"sigil-fill\"/>",
    );
    map.insert("backend", "<path d=\"M6 3 3 8l3 5M10 3l3 5-3 5\"/>");
    map.insert(
        "database",
        "<ellipse cx=\"8\" cy=\"4\" rx=\"5\" ry=\"2\"/>\n            <path d=\"M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4M3 8c0 1.1 2.2 2 5 2s5-.9 5-2\"/>",
    );
    map.insert("cloud", "<path d=\"M4.3 12.5h7.3a2.4 2.4 0 0 0 .2-4.8 4 4 0 0 0-7.5-1.3A3.1 3.1 0 0 0 4.3 12.5Z\"/>");
    map.insert(
        "security",
        "<path d=\"M8 2.2 13 4v3.5c0 3.1-1.8 5.4-5 6.5-3.2-1.1-5-3.4-5-6.5V4Z\"/>\n            <path d=\"m5.8 8 1.5 1.5 3-3\"/>",
    );
    map.insert(
        "messagebus",
        "<path d=\"M2.5 4.5h11M2.5 8h11M2.5 11.5h11\"/>\n            <circle cx=\"5\" cy=\"4.5\" r=\"1\" class=\"sigil-fill\"/>\n            <circle cx=\"10.5\" cy=\"8\" r=\"1\" class=\"sigil-fill\"/>\n            <circle cx=\"7\" cy=\"11.5\" r=\"1\" class=\"sigil-fill\"/>",
    );
    map.insert(
        "external",
        "<rect x=\"2.5\" y=\"5\" width=\"8.5\" height=\"8\" rx=\"1.5\"/>\n            <path d=\"M8 2.5h5.5V8M13.5 2.5 7.5 8.5\"/>",
    );
    map.insert("start", "<circle cx=\"8\" cy=\"8\" r=\"5\"/>\n            <path d=\"m7 5.4 3.6 2.6L7 10.6Z\" class=\"sigil-fill\"/>");
    map.insert(
        "active",
        "<path d=\"M2 8h3l1.5-3.5L9 12l1.6-4H14\"/>",
    );
    map.insert(
        "waiting",
        "<path d=\"M4 2.5h8M4 13.5h8M5 3c0 2.8 2 3.2 3 5-1 1.8-3 2.2-3 5M11 3c0 2.8-2 3.2-3 5 1 1.8 3 2.2 3 5\"/>",
    );
    map.insert(
        "success",
        "<circle cx=\"8\" cy=\"8\" r=\"5.3\"/>\n            <path d=\"m5.2 8 1.8 1.8 3.8-4\"/>",
    );
    map.insert(
        "failure",
        "<circle cx=\"8\" cy=\"8\" r=\"5.3\"/>\n            <path d=\"m5.7 5.7 4.6 4.6m0-4.6-4.6 4.6\"/>",
    );
    map.insert(
        "neutral",
        "<rect x=\"3\" y=\"3\" width=\"10\" height=\"10\" rx=\"2\"/>\n            <circle cx=\"8\" cy=\"8\" r=\"1.2\" class=\"sigil-fill\"/>",
    );
    map
});
