//! Architecture diagram renderer — faithful port of the original
//! `renderers/architecture/render-architecture.mjs` (plus `grid.mjs`).
//!
//! Produces the SVG that is injected into the template's SVG slot. The visual
//! contract (element classes, ordering, geometry) matches the Node.js
//! pipeline exactly so drag/zoom/focus/export features keep working.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use super::geometry::{
    anchor, arrow_class, arr, automatic_port_rhythm_bridge, automatic_port_spread, chosen_side, component_fill, component_text,
    default_from_side, default_to_side, esc, label_point, num, rounded_path, route_honors_endpoint_sides, route_points_value,
    segment_intersects_rect, side_aware_bridge_candidates, text_fit, text_units, variant_accent, Pt, Rect,
};
use super::legend::{legend_footprint, legend_layout, relationship_legend_obstacles, render_legend, resolve_legend};
use super::{
    animate_attr, focus_edge_attrs, focus_node_attrs, focus_node_title, render_definitions, render_semantic_sigil, svg_accessible_text,
    svg_root_attrs,
};

struct Layout {
    default_w: f64,
    default_h: f64,
    margin: f64,
    boundary_pad: f64,
    boundary_extra_bottom: f64,
    legend_h: f64,
}

const LAYOUT: Layout = Layout {
    default_w: 120.0,
    default_h: 60.0,
    margin: 40.0,
    boundary_pad: 30.0,
    boundary_extra_bottom: 20.0,
    legend_h: 28.0,
};

const LEGEND_CATALOG: [(&str, &str); 7] = [
    ("frontend", "Frontend"),
    ("backend", "Backend"),
    ("database", "Database"),
    ("cloud", "Cloud"),
    ("security", "Security"),
    ("messagebus", "Message bus"),
    ("external", "External"),
];

const COMPONENT_TEXT_FIT: (f64, f64, f64, f64) = (9.0, 6.0, 7.0, 6.0); // sublabelPreferred, sublabelMinimum, tagPreferred, tagMinimum

/// The full architecture rendering pipeline.
pub fn render_svg(arch: &Value) -> Result<String> {
    let grid = grid_layout(arch);
    let components_map = measure_components(arch, &grid);
    let component_steps = component_steps(arch, &components_map);
    let boundaries = measure_boundaries(arch, &components_map);
    let composition_frames: Vec<(String, String, f64, f64, f64, f64, f64)> = boundaries
        .iter()
        .enumerate()
        .map(|(index, b)| {
            (
                b.0.clone(),
                b.1.clone(),
                b.2,
                b.3,
                b.4,
                b.5,
                if b.1 == "security-group" { 8.0 } else { 12.0 },
            )
        })
        .collect();
    let _ = composition_frames;

    let component_contexts: HashMap<String, String> = components_map
        .iter()
        .map(|(id, rect)| {
            let mut scopes: Vec<(&str, f64)> = boundaries
                .iter()
                .filter(|b| b.6.iter().any(|wrapped| wrapped == id))
                .map(|b| (b.0.as_str(), b.2 * b.4))
                .collect();
            scopes.sort_by(|a, b| b.1.total_cmp(&a.1));
            let context = if scopes.is_empty() {
                "Architecture component".to_string()
            } else {
                scopes.iter().map(|(label, _)| *label).collect::<Vec<_>>().join(" › ")
            };
            (id.clone(), context)
        })
        .collect();

    let present_kinds: Vec<String> = components_map.values().map(|c| c.1.clone()).collect();
    let legend_config = arch.get("meta").and_then(|m| m.get("legend"));
    let legend_entries = resolve_legend(
        legend_config,
        &LEGEND_CATALOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect::<Vec<_>>(),
        &present_kinds,
    );

    let view_box = auto_view_box(arch, &components_map, &boundaries, &legend_entries);
    let legend_y = view_box.1 - 16.0;

    // Connection routing.
    let connections: Vec<&Value> = arr(arch.get("connections")).iter().collect();
    let automatic_ports = automatic_port_spread(&connections, &box_map(&components_map));
    let routed: HashMap<usize, Vec<Pt>> = connections
        .iter()
        .enumerate()
        .map(|(index, conn)| (index, path_for(conn, &components_map, &automatic_ports, index)))
        .collect();

    let svg = format!(
        "      <svg viewBox=\"0 0 {} {}\" {}>
{}
{}

        <!-- Background Grid -->
        <rect width=\"100%\" height=\"100%\" fill=\"url(#grid)\" />

        <!-- Boundaries (behind everything) -->
{}

        <!-- Connection paths (before components for correct z-order) -->
{}

        <!-- Components -->
{}

        <!-- Connection labels -->
{}

        <!-- Legend -->
{}
      </svg>",
        num(view_box.0),
        num(view_box.1),
        svg_root_attrs(arch.get("meta").unwrap_or(&Value::Null), "architecture diagram"),
        svg_accessible_text(arch.get("meta").unwrap_or(&Value::Null), "architecture diagram"),
        render_definitions(),
        boundaries
            .iter()
            .enumerate()
            .map(|(index, b)| render_boundary(b, index))
            .collect::<Vec<_>>()
            .join("\n\n"),
        connections
            .iter()
            .enumerate()
            .map(|(index, conn)| render_connection_path(arch, conn, index, &components_map, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        components_map
            .iter()
            .map(|(id, (rect, kind, label, sublabel, tag))| {
                let context = component_contexts.get(id).cloned().unwrap_or_default();
                render_component(arch, rect, kind, label, sublabel.as_deref(), tag.as_deref(), &context, component_steps.get(id).copied())
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        connections
            .iter()
            .enumerate()
            .map(|(index, conn)| render_connection_label(conn, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        render_legend_block(
            arch,
            &legend_entries,
            &connections,
            &routed,
            &components_map,
            &boundaries,
            legend_y,
            view_box,
        ),
    );
    Ok(svg)
}

/// A measured component: (rect, kind, label, sublabel, tag).
type MeasuredComponent = (Rect, String, String, Option<String>, Option<String>);

fn box_map(components: &HashMap<String, MeasuredComponent>) -> HashMap<String, Rect> {
    components.iter().map(|(id, (rect, ..))| (id.clone(), rect.clone())).collect()
}

/// `gridLayout` — fixed cell math only, never auto-layout.
fn grid_layout(arch: &Value) -> Option<Grid> {
    let raw = arch.get("layout")?;
    if raw.get("mode").and_then(Value::as_str) != Some("grid") {
        return None;
    }
    Some(Grid {
        origin: raw.get("origin").and_then(Value::as_array).map(|p| (p[0].as_f64().unwrap_or(40.0), p[1].as_f64().unwrap_or(80.0))).unwrap_or((40.0, 80.0)),
        cols: raw.get("cols").and_then(Value::as_u64).unwrap_or(4) as usize,
        gap_x: raw.get("gapX").and_then(Value::as_f64).unwrap_or(30.0),
        gap_y: raw.get("gapY").and_then(Value::as_f64).unwrap_or(40.0),
        cell_w: raw.get("cellW").and_then(Value::as_f64).unwrap_or(130.0),
        cell_h: raw.get("cellH").and_then(Value::as_f64).unwrap_or(64.0),
    })
}

struct Grid {
    origin: Pt,
    cols: usize,
    gap_x: f64,
    gap_y: f64,
    cell_w: f64,
    cell_h: f64,
}

/// `resolveComponentPos` — explicit pos wins; grid row/col otherwise.
fn resolve_component_pos(component: &Value, grid: &Option<Grid>) -> Option<Pt> {
    if let Some(pos) = component.get("pos").and_then(Value::as_array) {
        if pos.len() == 2 {
            return Some((pos[0].as_f64()?, pos[1].as_f64()?));
        }
    }
    let grid = grid.as_ref()?;
    let row = component.get("row").and_then(Value::as_u64)? as usize;
    let col = component.get("col").and_then(Value::as_u64)? as usize;
    let step_x = grid.cell_w + grid.gap_x;
    let step_y = grid.cell_h + grid.gap_y;
    Some((grid.origin.0 + col as f64 * step_x, grid.origin.1 + row as f64 * step_y))
}

/// `measureComponent` — resolve pos/size into a measured box.
fn measure_components(arch: &Value, grid: &Option<Grid>) -> HashMap<String, MeasuredComponent> {
    let mut map = HashMap::new();
    for component in arr(arch.get("components")) {
        let Some(id) = component.get("id").and_then(Value::as_str) else {
            continue;
        };
        let (x, y) = resolve_component_pos(component, grid).unwrap_or((f64::NAN, f64::NAN));
        let size = component.get("size").and_then(Value::as_array);
        let (w, h) = match size {
            Some(size) if size.len() == 2 => (size[0].as_f64().unwrap_or(LAYOUT.default_w), size[1].as_f64().unwrap_or(LAYOUT.default_h)),
            _ => (LAYOUT.default_w, LAYOUT.default_h),
        };
        let rect = Rect::new(id, x, y, w, h);
        map.insert(
            id.to_string(),
            (
                rect,
                component.get("type").and_then(Value::as_str).unwrap_or("external").to_string(),
                component.get("label").and_then(Value::as_str).unwrap_or(id).to_string(),
                component.get("sublabel").and_then(Value::as_str).map(String::from),
                component.get("tag").and_then(Value::as_str).map(String::from),
            ),
        );
    }
    map
}

/// `componentSteps` — animation trace step per component.
fn component_steps(arch: &Value, components: &HashMap<String, MeasuredComponent>) -> HashMap<String, f64> {
    let mut steps: HashMap<String, f64> = HashMap::new();
    for (index, conn) in arr(arch.get("connections")).iter().enumerate() {
        if let Some(from) = conn.get("from").and_then(Value::as_str) {
            steps.entry(from.to_string()).or_insert(index as f64);
        }
        if let Some(to) = conn.get("to").and_then(Value::as_str) {
            steps.entry(to.to_string()).or_insert(index as f64 + 1.0);
        }
    }
    for (index, component) in arr(arch.get("components")).iter().enumerate() {
        if let Some(id) = component.get("id").and_then(Value::as_str) {
            steps.entry(id.to_string()).or_insert(index as f64);
        }
    }
    // Drop steps for ids that are not components (defensive).
    steps.retain(|id, _| components.contains_key(id));
    steps
}

/// `boundaryRect` — a boundary computed from its wrapped components.
type Boundary = (String, String, f64, f64, f64, f64, Vec<String>, f64); // label, kind, x, y, width, height, wraps, pad

fn measure_boundaries(arch: &Value, components: &HashMap<String, MeasuredComponent>) -> Vec<Boundary> {
    let mut boundaries = Vec::new();
    for boundary in arr(arch.get("boundaries")) {
        let wraps: Vec<String> = boundary
            .get("wraps")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();
        let members: Vec<&Rect> = wraps.iter().filter_map(|id| components.get(id).map(|(rect, ..)| rect)).collect();
        if members.is_empty() {
            continue;
        }
        let min_x = members.iter().map(|m| m.x).fold(f64::INFINITY, f64::min);
        let min_y = members.iter().map(|m| m.y).fold(f64::INFINITY, f64::min);
        let max_x = members.iter().map(|m| m.x + m.width).fold(f64::NEG_INFINITY, f64::max);
        let max_y = members.iter().map(|m| m.y + m.height).fold(f64::NEG_INFINITY, f64::max);
        let pad = boundary.get("pad").and_then(Value::as_f64).unwrap_or(LAYOUT.boundary_pad);
        boundaries.push((
            boundary.get("label").and_then(Value::as_str).unwrap_or("Boundary").to_string(),
            boundary.get("kind").and_then(Value::as_str).unwrap_or("boundary").to_string(),
            min_x - pad,
            min_y - pad,
            max_x - min_x + pad * 2.0,
            max_y - min_y + pad + LAYOUT.boundary_extra_bottom,
            wraps,
            pad,
        ));
    }
    boundaries
}

/// `autoViewBox` — fit all geometry plus the measured legend.
fn auto_view_box(
    arch: &Value,
    components: &HashMap<String, MeasuredComponent>,
    boundaries: &[Boundary],
    legend_entries: &[super::legend::LegendEntry],
) -> Pt {
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for (rect, ..) in components.values() {
        max_x = max_x.max(rect.x + rect.width);
        max_y = max_y.max(rect.y + rect.height);
    }
    for b in boundaries {
        max_x = max_x.max(b.2 + b.4);
        max_y = max_y.max(b.3 + b.5);
    }
    let mut width = (max_x + LAYOUT.margin).ceil();
    let mut footprint = legend_footprint(legend_entries, (width - LAYOUT.margin * 2.0).max(1.0));
    if footprint.min_width > width - LAYOUT.margin * 2.0 {
        width = (footprint.min_width + LAYOUT.margin * 2.0).ceil();
        footprint = legend_footprint(legend_entries, width - LAYOUT.margin * 2.0);
    }
    let _ = arch;
    (
        width,
        (max_y + LAYOUT.margin + LAYOUT.legend_h + footprint.extra_height).ceil(),
    )
}

/// `renderBoundary` — the structural frame for a region/security group.
fn render_boundary(b: &Boundary, index: usize) -> String {
    let cls = if b.1 == "security-group" { "c-security-group" } else { "c-region" };
    let label_cls = if b.1 == "security-group" { "t-security" } else { "t-cloud" };
    let rx = if b.1 == "security-group" { 8.0 } else { 12.0 };
    format!(
        "        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"{}\" data-composition-frame-id=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" class=\"{}\" stroke-width=\"1\"/>\n        <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"9\" font-weight=\"600\">{}</text>",
        esc(&b.1),
        index,
        num(b.2),
        num(b.3),
        num(b.4),
        num(b.5),
        num(rx),
        cls,
        num(b.2 + 8.0),
        num(b.3 + 18.0),
        label_cls,
        esc(&b.0)
    )
}

/// Routing helpers shared by validation and rendering.
fn route_clears_components(conn: &Value, points: &[Pt], components: &HashMap<String, MeasuredComponent>) -> bool {
    let from_id = conn.get("from").and_then(Value::as_str).unwrap_or("");
    let to_id = conn.get("to").and_then(Value::as_str).unwrap_or("");
    for (id, (rect, ..)) in components {
        if id == from_id || id == to_id {
            continue;
        }
        for pair in points.windows(2) {
            if segment_intersects_rect(pair[0], pair[1], rect, 2.0) {
                return false;
            }
        }
    }
    true
}

fn route_clears_endpoint_components(points: &[Pt], from: &Rect, to: &Rect) -> bool {
    let last_segment = points.len() as i64 - 2;
    for index in 0..points.len() as i64 - 1 {
        let segment = (points[index as usize], points[index as usize + 1]);
        if index > 0 && segment_intersects_rect(segment.0, segment.1, from, 0.0) {
            return false;
        }
        if index < last_segment && segment_intersects_rect(segment.0, segment.1, to, 0.0) {
            return false;
        }
    }
    true
}

/// `routeVia` — per-connection routing for architecture diagrams.
fn route_via(
    conn: &Value,
    from: &Rect,
    to: &Rect,
    start: Pt,
    end: Pt,
    from_side: &str,
    to_side: &str,
    components: &HashMap<String, MeasuredComponent>,
) -> Vec<Pt> {
    if conn.get("via").is_some() {
        return conn
            .get("via")
            .and_then(Value::as_array)
            .map(|via| via.iter().filter_map(|p| p.as_array()).filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect())
            .unwrap_or_default();
    }
    match conn.get("route").and_then(Value::as_str).unwrap_or("auto") {
        "straight" => Vec::new(),
        "orthogonal-h" => {
            let mid_x = (start.0 + end.0) / 2.0;
            vec![(mid_x, start.1), (mid_x, end.1)]
        }
        "orthogonal-v" => {
            let mid_y = (start.1 + end.1) / 2.0;
            vec![(start.0, mid_y), (end.0, mid_y)]
        }
        _ => {
            let delta_x = (start.0 - end.0).abs();
            let delta_y = (start.1 - end.1).abs();
            if (delta_x < 4.0 || delta_y < 4.0) && route_honors_endpoint_sides(&[start, end], from_side, to_side) {
                return Vec::new();
            }

            let accept = |points: &[Pt]| -> bool {
                route_clears_endpoint_components(points, from, to) && route_clears_components(conn, points, components)
            };
            if let Some(bridge) = automatic_port_rhythm_bridge(start, end, from_side, to_side, &accept) {
                return bridge[1..bridge.len() - 1].to_vec();
            }

            let minimum_stub = 8.0;
            let from_vertical_side = start.1 == from.y || start.1 == from.y + from.height;
            let to_vertical_side = end.1 == to.y || end.1 == to.y + to.height;
            if from_vertical_side && to_vertical_side && delta_x < minimum_stub * 2.0 {
                for channel_x in [start.0.max(end.0) + minimum_stub * 2.0, start.0.min(end.0) - minimum_stub * 2.0] {
                    let candidate = vec![(channel_x, start.1), (channel_x, end.1)];
                    let mut points = vec![start];
                    points.extend(candidate.iter().copied());
                    points.push(end);
                    if route_honors_endpoint_sides(&points, from_side, to_side) && route_clears_components(conn, &points, components) {
                        return candidate;
                    }
                }
            }

            let from_horizontal_side = start.0 == from.x || start.0 == from.x + from.width;
            let to_horizontal_side = end.0 == to.x || end.0 == to.x + to.width;
            if from_horizontal_side && to_horizontal_side && delta_y < minimum_stub * 2.0 {
                for channel_y in [start.1.max(end.1) + minimum_stub * 2.0, start.1.min(end.1) - minimum_stub * 2.0] {
                    let candidate = vec![(start.0, channel_y), (end.0, channel_y)];
                    let mut points = vec![start];
                    points.extend(candidate.iter().copied());
                    points.push(end);
                    if route_honors_endpoint_sides(&points, from_side, to_side) && route_clears_components(conn, &points, components) {
                        return candidate;
                    }
                }
            }

            let mid_x = (start.0 + end.0) / 2.0;
            let horizontal_first = vec![(mid_x, start.1), (mid_x, end.1)];
            let mid_y = (start.1 + end.1) / 2.0;
            let vertical_first = vec![(start.0, mid_y), (end.0, mid_y)];
            let candidates = vec![horizontal_first.clone(), vertical_first.clone()];
            let side_safe: Vec<Vec<Pt>> = candidates
                .iter()
                .filter(|candidate| {
                    let mut points = vec![start];
                    points.extend(candidate.iter().copied());
                    points.push(end);
                    route_honors_endpoint_sides(&points, from_side, to_side)
                })
                .cloned()
                .collect();
            let side_aware = side_aware_bridge_candidates(start, end, from_side, to_side);
            let near_parallel_ports = ((from_side == "top" || from_side == "bottom")
                && (to_side == "top" || to_side == "bottom")
                && delta_x < minimum_stub * 2.0)
                || ((from_side == "left" || from_side == "right")
                    && (to_side == "left" || to_side == "right")
                    && delta_y < minimum_stub * 2.0);
            let mut ordered: Vec<Vec<Pt>> = Vec::new();
            if near_parallel_ports {
                ordered.extend(side_aware.iter().cloned());
                ordered.extend(side_safe.iter().cloned());
            } else {
                ordered.extend(side_safe.iter().cloned());
                ordered.extend(side_aware.iter().cloned());
            }
            ordered.extend(candidates.iter().filter(|c| !side_safe.contains(c)).cloned());
            for candidate in ordered {
                let mut points = vec![start];
                points.extend(candidate.iter().copied());
                points.push(end);
                if route_clears_endpoint_components(&points, from, to) && route_clears_components(conn, &points, components) {
                    return candidate;
                }
            }
            side_safe.first().cloned().or_else(|| side_aware.first().cloned()).unwrap_or(horizontal_first)
        }
    }
}

/// `pathFor` — full routed points for a connection.
fn path_for(
    conn: &Value,
    components: &HashMap<String, MeasuredComponent>,
    automatic_ports: &HashMap<usize, (Option<Pt>, Option<Pt>)>,
    index: usize,
) -> Vec<Pt> {
    let from_id = conn.get("from").and_then(Value::as_str).unwrap_or("");
    let to_id = conn.get("to").and_then(Value::as_str).unwrap_or("");
    let (Some((from_rect, ..)), Some((to_rect, ..))) = (components.get(from_id), components.get(to_id)) else {
        return Vec::new();
    };
    let from_side = chosen_side(conn.get("fromSide").and_then(Value::as_str), default_from_side(from_rect, to_rect));
    let to_side = chosen_side(conn.get("toSide").and_then(Value::as_str), default_to_side(from_rect, to_rect));
    let ports = automatic_ports.get(&index);
    let start = ports.and_then(|p| p.0).unwrap_or_else(|| anchor(from_rect, from_side));
    let end = ports.and_then(|p| p.1).unwrap_or_else(|| anchor(to_rect, to_side));
    let mut points = vec![start];
    points.extend(route_via(conn, from_rect, to_rect, start, end, from_side, to_side, components));
    points.push(end);
    points
}

/// `renderConnectionPath` — the `<path>` element for a connection.
fn render_connection_path(
    arch: &Value,
    conn: &Value,
    index: usize,
    _components: &HashMap<String, MeasuredComponent>,
    routed: &HashMap<usize, Vec<Pt>>,
) -> String {
    let (cls, marker) = arrow_class(conn.get("variant").and_then(Value::as_str).unwrap_or("default"));
    let points = routed.get(&index).cloned().unwrap_or_default();
    let stroke_width = conn
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(if conn.get("variant").and_then(Value::as_str) == Some("emphasis") { 1.8 } else { 1.5 });
    let label = conn.get("label").and_then(Value::as_str);
    let id = conn.get("id").and_then(Value::as_str);
    let from = conn.get("from").and_then(Value::as_str).unwrap_or("");
    let to = conn.get("to").and_then(Value::as_str).unwrap_or("");
    format!(
        "        <path {} data-composition-points=\"{}\" d=\"{}\" class=\"{}\"{} stroke-width=\"{}\" marker-end=\"url(#{})\"/>",
        focus_edge_attrs(from, to, label, Some(index), id),
        route_points_value(&points),
        rounded_path(&points, 8.0),
        cls,
        animate_attr(arch.get("meta").unwrap_or(&Value::Null), "edge", Some(index as f64)),
        num(stroke_width),
        marker
    )
}

/// `renderConnectionLabel` — the masked label chip on a connection.
fn render_connection_label(conn: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let Some(label) = conn.get("label").and_then(Value::as_str) else {
        return String::new();
    };
    let points = routed.get(&index).cloned().unwrap_or_default();
    let label_at = conn.get("labelAt").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_f64).collect());
    let (lx, ly) = label_point(
        label_at.as_ref(),
        conn.get("labelDx").and_then(Value::as_f64),
        conn.get("labelDy").and_then(Value::as_f64),
        conn.get("labelSegment").and_then(Value::as_u64).map(|v| v as usize),
        &points,
    );
    let w = (text_units(label) as f64 * 4.8 + 10.0).max(30.0);
    let from = conn.get("from").and_then(Value::as_str).unwrap_or("");
    let to = conn.get("to").and_then(Value::as_str).unwrap_or("");
    let id = conn.get("id").and_then(Value::as_str);
    let variant = conn.get("variant").and_then(Value::as_str);
    format!(
        "        <g data-detail=\"context\" {}>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"14\" rx=\"3\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"8\" text-anchor=\"middle\">{}</text>\n        </g>",
        focus_edge_attrs(from, to, Some(label), Some(index), id),
        num(lx - w / 2.0),
        num(ly - 10.0),
        num(w),
        num(lx),
        num(ly),
        variant_accent(variant, "t-messagebus"),
        esc(label)
    )
}

/// `renderComponent` — the focusable node group.
fn render_component(
    arch: &Value,
    rect: &Rect,
    kind: &str,
    label: &str,
    sublabel: Option<&str>,
    tag: Option<&str>,
    context: &str,
    step: Option<f64>,
) -> String {
    let fill = component_fill(kind);
    let accent = component_text(kind);
    let cx = rect.cx;
    let has_sub = sublabel.is_some_and(|s| !s.is_empty());
    let label_y = if has_sub { rect.y + rect.height / 2.0 - 2.0 } else { rect.y + rect.height / 2.0 + 4.0 };
    let sub = if has_sub {
        let font = text_fit::fitted_font_size(sublabel.unwrap(), rect.width, COMPONENT_TEXT_FIT.0, COMPONENT_TEXT_FIT.1);
        format!(
            "\n        <text data-detail=\"context\" x=\"{}\" y=\"{}\" class=\"t-muted\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(cx),
            num(rect.y + rect.height / 2.0 + 14.0),
            num(font),
            esc(sublabel.unwrap())
        )
    } else {
        String::new()
    };
    let tag_html = if let Some(tag) = tag.filter(|t| !t.is_empty()) {
        let font = text_fit::fitted_font_size(tag, rect.width, COMPONENT_TEXT_FIT.2, COMPONENT_TEXT_FIT.3);
        format!(
            "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(cx),
            num(rect.y + rect.height - 8.0),
            accent,
            num(font),
            esc(tag)
        )
    } else {
        String::new()
    };
    let step_attr = animate_attr(arch.get("meta").unwrap_or(&Value::Null), "node", step);
    format!(
        "        <g {}>\n          {}\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"c-mask\"/>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"{}\"{} stroke-width=\"1.5\"/>\n          {}\n          <text{} x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"11\" font-weight=\"600\" text-anchor=\"middle\">{}</text>{}{}\n        </g>",
        focus_node_attrs(&rect.id, label, Some(kind), sublabel, tag, Some(context)),
        focus_node_title(label, sublabel, Some(context), tag),
        num(rect.x),
        num(rect.y),
        num(rect.width),
        num(rect.height),
        num(rect.x),
        num(rect.y),
        num(rect.width),
        num(rect.height),
        fill,
        step_attr,
        render_semantic_sigil(kind, rect.x + 6.0, rect.y + 6.0, 11.0),
        if has_sub { " data-detail-anchor" } else { "" },
        num(cx),
        num(label_y),
        esc(label),
        sub,
        tag_html
    )
}

/// `renderLegend` — resolved legend for the architecture diagram.
#[allow(clippy::too_many_arguments)]
fn render_legend_block(
    arch: &Value,
    entries: &[super::legend::LegendEntry],
    connections: &[&Value],
    routed: &HashMap<usize, Vec<Pt>>,
    components: &HashMap<String, MeasuredComponent>,
    boundaries: &[Boundary],
    legend_y: f64,
    view_box: Pt,
) -> String {
    let obstacles = relationship_legend_obstacles(
        connections,
        &|conn: &Value, index: usize| routed.get(&index).cloned().unwrap_or_default(),
        &|conn: &Value, index: usize| {
            let Some(label) = conn.get("label").and_then(Value::as_str) else {
                return None;
            };
            let points = routed.get(&index).cloned().unwrap_or_default();
            let label_at = conn.get("labelAt").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_f64).collect());
            let (x, y) = label_point(
                label_at.as_ref(),
                conn.get("labelDx").and_then(Value::as_f64),
                conn.get("labelDy").and_then(Value::as_f64),
                conn.get("labelSegment").and_then(Value::as_u64).map(|v| v as usize),
                &points,
            );
            let width = (text_units(label) as f64 * 4.8 + 10.0).max(30.0);
            Some((x - width / 2.0, y - 10.0, width, 14.0))
        },
    );
    let content_bottom = components
        .values()
        .map(|(rect, ..)| rect.y + rect.height)
        .chain(boundaries.iter().map(|b| b.3 + b.5))
        .fold(0.0_f64, f64::max);
    let mut layout = legend_layout(
        40.0,
        legend_y,
        view_box.0 - 80.0,
        content_bottom + 8.0,
        if arch.get("meta").and_then(|m| m.get("legend")).is_none() { "hide" } else { "error" },
        "architecture",
    );
    layout.obstacles = obstacles;
    let swatch = |entry: &super::legend::LegendEntry| {
        format!(
            "<rect x=\"{}\" y=\"{}\" width=\"14\" height=\"9\" rx=\"2\" class=\"{}\" stroke-width=\"1\"/>",
            num(entry.x),
            num(entry.baseline - 8.0),
            component_fill(&entry.kind)
        )
    };
    render_legend(entries, &layout, &swatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_official_example() {
        let text = include_str!("../../examples/production-deployment.architecture.json");
        let arch: Value = serde_json::from_str(text).unwrap();
        let svg = render_svg(&arch).unwrap();
        assert!(svg.contains("<svg viewBox=\"0 0 1436 760\""));
        assert!(svg.contains("data-node-id=\"clients\""));
        assert!(svg.contains("AWS us-east-1 / production"));
        assert!(svg.contains("data-edge-from=\"clients\""));
        assert!(svg.contains("<!-- Legend -->"));
        assert!(svg.contains("data-composition-points=\"160,330;230,330\""));
    }

    #[test]
    fn grid_layout_places_components() {
        let arch = serde_json::json!({
            "layout": { "mode": "grid", "origin": [40, 80], "cols": 4, "gapX": 30, "gapY": 40, "cellW": 130, "cellH": 64 },
            "components": [
                { "id": "a", "type": "backend", "label": "A", "row": 0, "col": 0 },
                { "id": "b", "type": "backend", "label": "B", "row": 1, "col": 1 }
            ],
            "meta": { "title": "Grid" }
        });
        let grid = grid_layout(&arch);
        assert!(grid.is_some());
        let components = measure_components(&arch, &grid);
        assert_eq!(components["a"].0.x, 40.0);
        assert_eq!(components["a"].0.y, 80.0);
        assert_eq!(components["b"].0.x, 40.0 + 160.0);
        assert_eq!(components["b"].0.y, 80.0 + 104.0);
    }

    #[test]
    fn component_steps_mirror_original() {
        let text = include_str!("../../examples/production-deployment.architecture.json");
        let arch: Value = serde_json::from_str(text).unwrap();
        let grid = grid_layout(&arch);
        let components = measure_components(&arch, &grid);
        let steps = component_steps(&arch, &components);
        // From the golden output: clients=0 ... events=7, worker=9 (edge 8 claimed 8)
        assert_eq!(steps.get("clients"), Some(&0.0));
        assert_eq!(steps.get("events"), Some(&7.0));
        assert_eq!(steps.get("worker"), Some(&9.0));
        assert_eq!(steps.get("observability"), Some(&12.0));
    }
}
