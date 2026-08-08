//! Legend measurement and rendering — port of the original
//! `renderers/shared/legend.mjs`.

use serde_json::Value;

use super::geometry::{num, text_units, Pt};

const DEFAULT_FONT_SIZE: f64 = 8.0;
const DEFAULT_ITEM_GAP: f64 = 22.0;
const DEFAULT_LINE_GAP: f64 = 22.0;
const DEFAULT_SWATCH_GAP: f64 = 7.0;
const TEXT_ADVANCE_EM: f64 = 0.62;
const INTERACTIVE_BADGE_ALLOWANCE: f64 = 21.0;

#[derive(Debug, Clone)]
pub struct LegendEntry {
    pub kind: String,
    pub label: String,
    pub present: bool,
    pub interactive: bool,
    pub swatch_width: Option<f64>,
    pub swatch_gap: Option<f64>,
    pub x: f64,
    pub baseline: f64,
    pub width: f64,
}

/// `resolveLegend` — mode/override filtering against the kinds present.
pub fn resolve_legend(config: Option<&Value>, catalog: &[(String, String)], present_kinds: &[String]) -> Vec<LegendEntry> {
    let mode = config
        .and_then(|c| c.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("auto");
    if mode == "hidden" {
        return Vec::new();
    }
    let overrides = config.and_then(|c| c.get("entries")).and_then(Value::as_object);
    let mut entries = Vec::new();
    for (kind, label) in catalog {
        let override_entry = overrides.and_then(|o| o.get(kind.as_str()));
        let selected_by_mode = mode == "all" || present_kinds.contains(kind);
        let override_visible = override_entry.and_then(|o| o.get("visible")).and_then(Value::as_bool);
        let visible = match override_visible {
            Some(true) => true,
            Some(false) => false,
            None => selected_by_mode,
        };
        if !visible {
            continue;
        }
        let override_label = override_entry.and_then(|o| o.get("label")).and_then(Value::as_str);
        entries.push(LegendEntry {
            kind: kind.clone(),
            label: override_label.unwrap_or(label).to_string(),
            present: present_kinds.contains(kind),
            interactive: present_kinds.contains(kind),
            swatch_width: None,
            swatch_gap: None,
            x: 0.0,
            baseline: 0.0,
            width: 0.0,
        });
    }
    entries
}

fn measured_entry_width(entry: &LegendEntry, font_size: f64, swatch_gap: f64) -> f64 {
    let swatch_width = entry.swatch_width.unwrap_or(14.0);
    (swatch_width + swatch_gap + text_units(&entry.label) as f64 * font_size * TEXT_ADVANCE_EM
        + if entry.interactive { INTERACTIVE_BADGE_ALLOWANCE } else { 0.0 })
    .ceil()
}

pub struct LegendFootprint {
    pub measured: Vec<LegendEntry>,
    pub rows: Vec<Vec<usize>>,
    pub row_count: usize,
    pub min_width: f64,
    pub extra_height: f64,
}

/// `legendFootprint` — pure width/row math shared by viewBox sizing and final
/// placement so the two never disagree.
pub fn legend_footprint(entries: &[LegendEntry], width: f64) -> LegendFootprint {
    if entries.is_empty() {
        return LegendFootprint {
            measured: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            min_width: 0.0,
            extra_height: 0.0,
        };
    }
    let measured: Vec<LegendEntry> = entries
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.width = measured_entry_width(&entry, DEFAULT_FONT_SIZE, entry.swatch_gap.unwrap_or(DEFAULT_SWATCH_GAP));
            entry
        })
        .collect();
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut cursor = 0.0;
    for (index, entry) in measured.iter().enumerate() {
        let row = rows.last_mut().unwrap();
        let required = if row.is_empty() { 0.0 } else { DEFAULT_ITEM_GAP } + entry.width;
        if !row.is_empty() && cursor + required > width {
            rows.push(vec![index]);
            cursor = entry.width;
        } else {
            row.push(index);
            cursor += required;
        }
    }
    let row_count = rows.len();
    let min_width = measured.iter().map(|e| e.width).fold(0.0_f64, f64::max);
    let extra_height = (row_count as f64 - 1.0) * DEFAULT_LINE_GAP;
    LegendFootprint {
        measured,
        rows,
        row_count,
        min_width,
        extra_height,
    }
}

pub struct LegendLayout {
    pub x: f64,
    pub baseline_y: f64,
    pub width: f64,
    pub min_title_y: f64,
    pub obstacles: Vec<Obstacle>,
    pub unfit: &'static str,
    pub diagram_type: &'static str,
    pub font_size: f64,
    pub item_gap: f64,
}

#[derive(Clone)]
pub enum Obstacle {
    Segment { start: Pt, end: Pt },
    Rect { x: f64, y: f64, width: f64, height: f64 },
}

fn rects_overlap(a: &Obstacle, x: f64, y: f64, width: f64, height: f64) -> bool {
    match a {
        Obstacle::Segment { start, end } => segment_intersects_rect(*start, *end, x, y, width, height),
        Obstacle::Rect { x: rx, y: ry, width: rw, height: rh } => {
            x < rx + rw && *rx < x + width && y < ry + rh && *ry < y + height
        }
    }
}

fn segment_intersects_rect(start: Pt, end: Pt, x: f64, y: f64, width: f64, height: f64) -> bool {
    // Liang-Barsky style slab test used by the original segmentIntersectsRect.
    let (x0, y0) = start;
    let (x1, y1) = end;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let mut t_min = 0.0;
    let mut t_max = 1.0;
    for (p, q) in [(-dx, x0 - x), (dx, x + width - x0), (-dy, y0 - y), (dy, y + height - y0)] {
        if p.abs() <= 1e-9 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t_max {
                    return false;
                }
                if r > t_min {
                    t_min = r;
                }
            } else {
                if r < t_min {
                    return false;
                }
                if r < t_max {
                    t_max = r;
                }
            }
        }
    }
    true
}

/// `relationshipLegendObstacles` — collect connection segments/labels that the
/// legend band must not overlap.
pub fn relationship_legend_obstacles(
    relations: &[&Value],
    points_for: &dyn Fn(&Value, usize) -> Vec<Pt>,
    label_rect_for: &dyn Fn(&Value, usize) -> Option<(f64, f64, f64, f64)>,
) -> Vec<Obstacle> {
    let mut obstacles = Vec::new();
    for (index, relation) in relations.iter().enumerate() {
        let points = points_for(relation, index);
        let finite: Vec<Pt> = points
            .iter()
            .copied()
            .filter(|p| p.0.is_finite() && p.1.is_finite())
            .collect();
        for pair in finite.windows(2) {
            obstacles.push(Obstacle::Segment { start: pair[0], end: pair[1] });
        }
        if let Some((x, y, width, height)) = label_rect_for(relation, index) {
            if x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite() {
                obstacles.push(Obstacle::Rect { x, y, width, height });
            }
        }
    }
    obstacles
}

/// `measureLegend` — position entries and enforce the legend band contract.
fn measure_legend(entries: &[LegendEntry], layout: &LegendLayout) -> Option<(Vec<LegendEntry>, f64)> {
    if entries.is_empty() {
        return None;
    }
    let footprint = legend_footprint(entries, layout.width);
    if let Some(too_wide) = footprint.measured.iter().find(|e| e.width > layout.width) {
        if layout.unfit == "hide" {
            return None;
        }
        panic!(
            "[legend/label-too-wide] {} legend label for \"{}\" needs {}px but only {}px is available.",
            layout.diagram_type, too_wide.kind, num(too_wide.width), num(layout.width)
        );
    }
    let title_y = layout.baseline_y - footprint.extra_height - 20.0;
    let legend_top_y = title_y - 10.0;
    if legend_top_y < layout.min_title_y {
        if layout.unfit == "hide" {
            return None;
        }
        panic!(
            "[legend/vertical-overflow] {} legend needs {} rows, which would start at y={} above the available legend band at y={}.",
            layout.diagram_type,
            footprint.row_count,
            num(legend_top_y),
            num(layout.min_title_y)
        );
    }
    let mut positioned: Vec<LegendEntry> = Vec::new();
    for (row_index, row) in footprint.rows.iter().enumerate() {
        let mut entry_x = layout.x;
        let baseline = layout.baseline_y - (footprint.row_count as f64 - row_index as f64 - 1.0) * DEFAULT_LINE_GAP;
        for &entry_index in row {
            let mut entry = footprint.measured[entry_index].clone();
            entry.x = entry_x;
            entry.baseline = baseline;
            positioned.push(entry);
            entry_x += footprint.measured[entry_index].width + layout.item_gap;
        }
    }

    // Collision check against authored geometry.
    let mut legend_rects: Vec<(String, f64, f64, f64, f64)> = Vec::new();
    legend_rects.push(("title".to_string(), layout.x, legend_top_y, 42.0, 13.0));
    for entry in &positioned {
        legend_rects.push((entry.kind.clone(), entry.x, entry.baseline - 9.0, entry.width, 12.0));
    }
    for (kind, x, y, width, height) in &legend_rects {
        if layout.obstacles.iter().any(|obstacle| rects_overlap(obstacle, *x, *y, *width, *height)) {
            if layout.unfit == "hide" {
                return None;
            }
            panic!(
                "[legend/content-overlap] {} legend entry \"{}\" overlaps authored relationship geometry.",
                layout.diagram_type, kind
            );
        }
    }
    Some((positioned, title_y))
}

/// `renderLegend` — the resolved legend SVG block.
pub fn render_legend(entries: &[LegendEntry], layout: &LegendLayout, render_swatch: &dyn Fn(&LegendEntry) -> String) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let Some((measured, title_y)) = measure_legend(entries, layout) else {
        return String::new();
    };
    let has_interactive = measured.iter().any(|e| e.interactive);
    let root_attributes = if has_interactive { " data-legend data-legend-bridge" } else { " data-legend" };
    let mut parts = vec![
        format!("        <g{root_attributes}>"),
        format!(
            "          <text x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"10\" font-weight=\"600\">Legend</text>",
            num(layout.x),
            num(title_y)
        ),
    ];
    for entry in &measured {
        let interactive = if entry.interactive {
            format!(
                " data-legend-kind=\"{}\" data-legend-label=\"{}\"",
                crate::template::esc(&entry.kind),
                crate::template::esc(&entry.label)
            )
        } else {
            String::new()
        };
        parts.push(format!(
            "          <g data-legend-semantic-kind=\"{}\"{interactive} data-legend-x=\"{}\" data-legend-baseline=\"{}\" data-legend-width=\"{}\">",
            crate::template::esc(&entry.kind),
            num(entry.x),
            num(entry.baseline),
            num(entry.width)
        ));
        parts.push(format!("            {}", render_swatch(entry)));
        let swatch_width = entry.swatch_width.unwrap_or(14.0);
        let swatch_gap = entry.swatch_gap.unwrap_or(DEFAULT_SWATCH_GAP);
        parts.push(format!(
            "            <text x=\"{}\" y=\"{}\" class=\"t-muted\" font-size=\"{}\">{}</text>",
            num(entry.x + swatch_width + swatch_gap),
            num(entry.baseline),
            num(layout.font_size),
            crate::template::esc(&entry.label)
        ));
        parts.push("          </g>".to_string());
    }
    parts.push("        </g>".to_string());
    parts.join("\n")
}

/// Shortcut: build a legend layout struct with defaults.
pub fn legend_layout(x: f64, baseline_y: f64, width: f64, min_title_y: f64, unfit: &'static str, diagram_type: &'static str) -> LegendLayout {
    LegendLayout {
        x,
        baseline_y,
        width,
        min_title_y,
        obstacles: Vec::new(),
        unfit,
        diagram_type,
        font_size: DEFAULT_FONT_SIZE,
        item_gap: DEFAULT_ITEM_GAP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_legend_auto_picks_present_kinds() {
        let catalog = vec![
            ("backend".to_string(), "Backend".to_string()),
            ("database".to_string(), "Database".to_string()),
        ];
        let entries = resolve_legend(None, &catalog, &["backend".to_string()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, "backend");
        assert!(entries[0].interactive);
    }

    #[test]
    fn resolve_legend_hidden_is_empty() {
        let catalog = vec![("backend".to_string(), "Backend".to_string())];
        let config = serde_json::json!({ "mode": "hidden" });
        let entries = resolve_legend(Some(&config), &catalog, &["backend".to_string()]);
        assert!(entries.is_empty());
    }

    #[test]
    fn footprint_wraps_rows() {
        let mut entries = Vec::new();
        for kind in ["backend", "database", "cloud"] {
            entries.push(LegendEntry {
                kind: kind.to_string(),
                label: kind.to_string(),
                present: true,
                interactive: false,
                swatch_width: None,
                swatch_gap: None,
                x: 0.0,
                baseline: 0.0,
                width: 0.0,
            });
        }
        let footprint = legend_footprint(&entries, 80.0);
        assert_eq!(footprint.row_count, 3);
        assert!(footprint.extra_height > 0.0);
    }
}
