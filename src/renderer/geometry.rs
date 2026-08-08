//! Geometry helpers — a faithful Rust port of the original Archify
//! `renderers/shared/geometry.mjs` routing and measurement primitives.
//!
//! Floating-point behavior intentionally mirrors JavaScript: both run IEEE-754
//! doubles, and number formatting below produces the same shortest round-trip
//! strings the JS template literals emitted.

use std::collections::HashMap;

use serde_json::Value;

/// A measured component/node box.
#[derive(Debug, Clone)]
pub struct Rect {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub cx: f64,
    pub cy: f64,
}

impl Rect {
    pub fn new(id: &str, x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect {
            id: id.to_string(),
            x,
            y,
            width,
            height,
            cx: x + width / 2.0,
            cy: y + height / 2.0,
        }
    }
}

pub type Pt = (f64, f64);

/// Format a float exactly like `String(number)` in JavaScript: shortest
/// round-trip representation, integers without a decimal point, and `0`
/// instead of `-0`.
pub fn num(n: f64) -> String {
    if n == 0.0 {
        return "0".to_string();
    }
    let s = format!("{n}");
    if s == "-0" {
        return "0".to_string();
    }
    s
}

/// JS `Math.round` — rounds half toward +Infinity (Rust rounds half away).
pub fn js_round(n: f64) -> f64 {
    (n + 0.5).floor()
}

/// JS `Array.prototype.at(-1)` equivalent.
pub fn last<T>(items: &[T]) -> Option<&T> {
    items.last()
}

/// JS `asArray(value)` — non-arrays become an empty list.
pub fn arr<'a>(value: Option<&'a Value>) -> &'a Vec<Value> {
    value.and_then(Value::as_array).unwrap_or(&EMPTY_ARRAY)
}

pub static EMPTY_ARRAY: Vec<Value> = Vec::new();

pub fn arr_str<'a>(value: Option<&'a Value>) -> Vec<&'a str> {
    arr(value).iter().filter_map(|v| v.as_str()).collect()
}

/// `esc()` moved to [`crate::template::esc`]; this module re-exports for the
/// renderers to keep the import surface small.
pub use crate::template::esc;

/// Fullwidth/wide glyph ranges from the original `FULLWIDTH_RE`.
fn is_fullwidth(ch: char) -> bool {
    let c = ch as u32;
    (0x1100..=0x115F).contains(&c)
        || (0x2329..=0x232A).contains(&c)
        || (0x2E80..=0xA4CF).contains(&c)
        || (0xAC00..=0xD7A3).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFE10..=0xFE19).contains(&c)
        || (0xFE30..=0xFE6F).contains(&c)
        || (0xFF01..=0xFF60).contains(&c)
        || (0xFFE0..=0xFFE6).contains(&c)
        || (0x16FE0..=0x18DFF).contains(&c)
        || (0x1AFF0..=0x1AFFF).contains(&c)
        || (0x1B000..=0x1B2FF).contains(&c)
        || (0x1F000..=0x1FAFF).contains(&c)
        || (0x20000..=0x3FFFD).contains(&c)
}

/// Monospace text width in "units": CJK and other wide glyphs count double.
pub fn text_units(text: &str) -> usize {
    text.chars().map(|ch| if is_fullwidth(ch) { 2 } else { 1 }).sum()
}

/// Shared node-text fitting constants (port of `text-fit.mjs`).
pub mod text_fit {
    pub const WIDTH_FACTOR: f64 = 0.6;
    pub const HORIZONTAL_PADDING: f64 = 8.0;

    /// `fittedNodeFontSize` — largest size at or below `preferred` that fits.
    pub fn fitted_font_size(text: &str, width: f64, preferred: f64, minimum: f64) -> f64 {
        let units = (super::text_units(text) as f64).max(1.0);
        let available = (width - HORIZONTAL_PADDING).max(1.0);
        let fitted = preferred.min(available / (units * WIDTH_FACTOR));
        ((fitted * 10.0).floor() / 10.0).max(minimum)
    }

    pub fn minimum_text_width(text: &str, minimum: f64) -> f64 {
        super::text_units(text) as f64 * minimum * WIDTH_FACTOR
    }

    pub fn available_text_width(width: f64) -> f64 {
        width - HORIZONTAL_PADDING
    }
}

/// `anchor(rect, side)` — the connection point on a box side.
pub fn anchor(rect: &Rect, side: &str) -> Pt {
    match side {
        "left" => (rect.x, rect.cy),
        "right" => (rect.x + rect.width, rect.cy),
        "top" => (rect.cx, rect.y),
        "bottom" => (rect.cx, rect.y + rect.height),
        _ => (rect.x + rect.width, rect.cy),
    }
}

pub fn default_from_side(from: &Rect, to: &Rect) -> &'static str {
    if to.cx < from.cx {
        "left"
    } else if to.cx > from.cx {
        "right"
    } else if to.cy > from.cy {
        "bottom"
    } else {
        "top"
    }
}

pub fn default_to_side(from: &Rect, to: &Rect) -> &'static str {
    if to.cx < from.cx {
        "right"
    } else if to.cx > from.cx {
        "left"
    } else if to.cy > from.cy {
        "top"
    } else {
        "bottom"
    }
}

pub fn chosen_side<'a>(side: Option<&'a str>, fallback: &'a str) -> &'a str {
    match side {
        Some(s) if s != "auto" => s,
        _ => fallback,
    }
}

fn cross_product(a: Pt, b: Pt, c: Pt) -> f64 {
    (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0)
}

fn collinear_forward(a: Pt, b: Pt, c: Pt) -> bool {
    if cross_product(a, b, c).abs() > 0.0001 {
        return false;
    }
    (b.0 - a.0) * (c.0 - b.0) + (b.1 - a.1) * (c.1 - b.1) >= -0.0001
}

/// `normalizeRoutePoints` — drop duplicates and collinear midpoints.
pub fn normalize_route_points(points: &[Pt]) -> Vec<Pt> {
    let finite: Vec<Pt> = points.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    let mut deduped: Vec<Pt> = Vec::new();
    for point in finite {
        match deduped.last() {
            Some(prev) if (point.0 - prev.0).abs() <= 0.0001 && (point.1 - prev.1).abs() <= 0.0001 => {}
            _ => deduped.push(point),
        }
    }
    let mut normalized: Vec<Pt> = Vec::new();
    for point in deduped {
        while normalized.len() >= 2 && collinear_forward(normalized[normalized.len() - 2], normalized[normalized.len() - 1], point) {
            normalized.pop();
        }
        normalized.push(point);
    }
    normalized
}

const ENDPOINT_SIDE_RULES: [(&str, &str, f64, f64, &str, &str); 4] = [
    ("left", "horizontal", -1.0, 1.0, "leftward", "rightward from the left"),
    ("right", "horizontal", 1.0, -1.0, "rightward", "leftward from the right"),
    ("top", "vertical", -1.0, 1.0, "upward", "downward from above"),
    ("bottom", "vertical", 1.0, -1.0, "downward", "upward from below"),
];

fn endpoint_side_issue(points: &[Pt], endpoint: &str, side: &str) -> Option<(Pt, Pt)> {
    let rule = ENDPOINT_SIDE_RULES.iter().find(|(s, ..)| *s == side)?;
    let (_, axis, source_sign, target_sign, ..) = *rule;
    let normalized = normalize_route_points(points);
    if normalized.len() < 2 {
        return None;
    }
    let segment_index = if endpoint == "source" { 0 } else { normalized.len() - 2 };
    let start = normalized[segment_index];
    let end = normalized[segment_index + 1];
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let along = if axis == "horizontal" { dx } else { dy };
    let across = if axis == "horizontal" { dy } else { dx };
    let expected_sign = if endpoint == "source" { source_sign } else { target_sign };
    if across.abs() <= 0.0001 && along * expected_sign > 0.0001 {
        return None;
    }
    Some((start, end))
}

/// `routeHonorsEndpointSides` — first/final segments must leave the authored
/// sides perpendicularly.
pub fn route_honors_endpoint_sides(points: &[Pt], from_side: &str, to_side: &str) -> bool {
    endpoint_side_issue(points, "source", from_side).is_none() && endpoint_side_issue(points, "target", to_side).is_none()
}

fn segment_position(index: usize, segment_count: usize) -> &'static str {
    if index == 0 {
        "source-stub"
    } else if index == segment_count - 1 {
        "target-stub"
    } else {
        "interior"
    }
}

/// `collectRouteRhythmIssues` — micro-segments and short interior segments.
pub fn route_rhythm_issue_count(points: &[Pt], interior_segment_px: f64, micro_segment_px: f64) -> usize {
    let points = normalize_route_points(points);
    if points.len() < 2 {
        return 0;
    }
    let mut issues = 0;
    for segment_index in 0..points.len() - 1 {
        let start = points[segment_index];
        let end = points[segment_index + 1];
        let length = (end.0 - start.0).abs() + (end.1 - start.1).abs();
        if length <= 0.0001 {
            continue;
        }
        let position = segment_position(segment_index, points.len() - 1);
        let flagged = length < micro_segment_px - 0.0001
            || (position == "interior" && length < interior_segment_px - 0.0001);
        if flagged {
            issues += 1;
        }
    }
    issues
}

const PORT_OUTWARD_VECTOR: [(&str, f64, f64); 4] = [("left", -1.0, 0.0), ("right", 1.0, 0.0), ("top", 0.0, -1.0), ("bottom", 0.0, 1.0)];

fn outward_stub(point: Pt, side: &str, distance: f64) -> Pt {
    let (_, dx, dy) = PORT_OUTWARD_VECTOR.iter().find(|(s, ..)| *s == side).copied().unwrap_or(("", 0.0, 0.0));
    (point.0 + dx * distance, point.1 + dy * distance)
}

fn collinear_backtrack(a: Pt, b: Pt, c: Pt) -> bool {
    let first = (b.0 - a.0, b.1 - a.1);
    let second = (c.0 - b.0, c.1 - b.1);
    let cross = first.0 * second.1 - first.1 * second.0;
    let dot = first.0 * second.0 + first.1 * second.1;
    cross.abs() <= 0.0001 && dot < -0.0001
}

/// `sideAwareBridgeCandidates` — bounded outside channels for near-parallel
/// anchors on the same axis.
pub fn side_aware_bridge_candidates(start: Pt, end: Pt, from_side: &str, to_side: &str) -> Vec<Vec<Pt>> {
    let start_stub = outward_stub(start, from_side, 24.0);
    let end_stub = outward_stub(end, to_side, 24.0);
    let mut raw_candidates: Vec<Vec<Pt>> = Vec::new();
    let minimum_bridge = 16.0;
    let vertical = ["top", "bottom"];
    let horizontal = ["left", "right"];

    if vertical.contains(&from_side) && vertical.contains(&to_side) && (start.0 - end.0).abs() < minimum_bridge {
        for channel_x in [start.0.max(end.0) + minimum_bridge, start.0.min(end.0) - minimum_bridge] {
            raw_candidates.push(vec![start_stub, (channel_x, start_stub.1), (channel_x, end_stub.1), end_stub]);
        }
    }
    if horizontal.contains(&from_side) && horizontal.contains(&to_side) && (start.1 - end.1).abs() < minimum_bridge {
        for channel_y in [start.1.max(end.1) + minimum_bridge, start.1.min(end.1) - minimum_bridge] {
            raw_candidates.push(vec![start_stub, (start_stub.0, channel_y), (end_stub.0, channel_y), end_stub]);
        }
    }

    raw_candidates.push(vec![start_stub, (end_stub.0, start_stub.1), end_stub]);
    raw_candidates.push(vec![start_stub, (start_stub.0, end_stub.1), end_stub]);

    let mut full: Vec<Vec<Pt>> = Vec::new();
    for candidate in raw_candidates {
        let mut points = vec![start];
        points.extend(candidate);
        points.push(end);
        let normalized = normalize_route_points(&points);
        if normalized.len() < 2 {
            continue;
        }
        let first = *normalized.first().unwrap();
        let second = *normalized.get(1).unwrap_or(&first);
        let last = *normalized.last().unwrap();
        let second_last = *normalized.get(normalized.len().saturating_sub(2)).unwrap_or(&last);
        if collinear_backtrack(first, second, *normalized.get(2).unwrap_or(&second)) {
            continue;
        }
        if collinear_backtrack(second_last, last, *normalized.get(normalized.len().saturating_sub(3)).unwrap_or(&second_last)) {
            continue;
        }
        if !route_honors_endpoint_sides(&normalized, from_side, to_side) {
            continue;
        }
        full.push(normalized[1..normalized.len() - 1].to_vec());
    }
    full
}

/// `automaticPortRhythmBridge` — full outside-channel route when a conventional
/// midpoint dogleg would violate the 8/16px rhythm floors.
pub fn automatic_port_rhythm_bridge(
    start: Pt,
    end: Pt,
    from_side: &str,
    to_side: &str,
    accept: &dyn Fn(&[Pt]) -> bool,
) -> Option<Vec<Pt>> {
    if !start.0.is_finite() || !start.1.is_finite() || !end.0.is_finite() || !end.1.is_finite() {
        return None;
    }
    let from_vector = PORT_OUTWARD_VECTOR.iter().find(|(s, ..)| *s == from_side);
    let to_vector = PORT_OUTWARD_VECTOR.iter().find(|(s, ..)| *s == to_side);
    if from_vector.is_none() || to_vector.is_none() {
        return None;
    }
    let endpoint_stub_px = 24.0;
    let interior_segment_px = 16.0;
    let start_stub = (start.0 + from_vector.unwrap().1 * endpoint_stub_px, start.1 + from_vector.unwrap().2 * endpoint_stub_px);
    let end_stub = (end.0 + to_vector.unwrap().1 * endpoint_stub_px, end.1 + to_vector.unwrap().2 * endpoint_stub_px);
    let vertical = ["top", "bottom"];
    let horizontal = ["left", "right"];
    let mut candidates: Vec<Vec<Pt>> = Vec::new();

    if vertical.contains(&from_side) && vertical.contains(&to_side) && (start.0 - end.0).abs() < interior_segment_px {
        for channel_x in [start.0.max(end.0) + interior_segment_px, start.0.min(end.0) - interior_segment_px] {
            candidates.push(vec![start, start_stub, (channel_x, start_stub.1), (channel_x, end_stub.1), end_stub, end]);
        }
    }
    if horizontal.contains(&from_side) && horizontal.contains(&to_side) && (start.1 - end.1).abs() < interior_segment_px {
        for channel_y in [start.1.max(end.1) + interior_segment_px, start.1.min(end.1) - interior_segment_px] {
            candidates.push(vec![start, start_stub, (start_stub.0, channel_y), (end_stub.0, channel_y), end_stub, end]);
        }
    }

    for candidate in candidates {
        let normalized = normalize_route_points(&candidate);
        if route_honors_endpoint_sides(&normalized, from_side, to_side)
            && route_rhythm_issue_count(&normalized, interior_segment_px, 8.0) == 0
            && accept(&normalized)
        {
            return Some(normalized);
        }
    }
    None
}

fn relation_key(relation: &Value) -> String {
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}",
        relation.get("id").and_then(Value::as_str).unwrap_or(""),
        relation.get("from").and_then(Value::as_str).unwrap_or(""),
        relation.get("to").and_then(Value::as_str).unwrap_or(""),
        relation.get("label").and_then(Value::as_str).unwrap_or(""),
    )
}

struct GroupItem {
    relation_index: usize,
    endpoint: &'static str,
    side: String,
    counterpart_cx: f64,
    counterpart_cy: f64,
    key: String,
}

/// `automaticPortSpread` — spread multi-relation fan-out anchors along the
/// shared side so parallel connections stay readable.
pub fn automatic_port_spread(
    relations: &[&Value],
    boxes: &HashMap<String, Rect>,
) -> HashMap<usize, (Option<Pt>, Option<Pt>)> {
    let mut groups: HashMap<(String, String), (Rect, Vec<GroupItem>)> = HashMap::new();
    let mut spread: HashMap<usize, (Option<Pt>, Option<Pt>)> = HashMap::new();

    for (index, relation) in relations.iter().enumerate() {
        if relation.get("route").and_then(Value::as_str).map(|r| r != "auto").unwrap_or(false) {
            continue;
        }
        if relation.get("via").is_some()
            || relation.get("channelX").is_some()
            || relation.get("channelY").is_some()
            || relation.get("labelAt").is_some()
        {
            continue;
        }
        let Some(from_id) = relation.get("from").and_then(Value::as_str) else {
            continue;
        };
        let Some(to_id) = relation.get("to").and_then(Value::as_str) else {
            continue;
        };
        let (Some(from), Some(to)) = (boxes.get(from_id), boxes.get(to_id)) else {
            continue;
        };
        let from_side = chosen_side(relation.get("fromSide").and_then(Value::as_str), default_from_side(from, to));
        let to_side = chosen_side(relation.get("toSide").and_then(Value::as_str), default_to_side(from, to));
        let key = relation_key(relation);
        let entry = groups.entry((from.id.clone(), from_side.to_string())).or_insert_with(|| (from.clone(), Vec::new()));
        entry.1.push(GroupItem {
            relation_index: index,
            endpoint: "from",
            side: from_side.to_string(),
            counterpart_cx: to.cx,
            counterpart_cy: to.cy,
            key: key.clone(),
        });
        let entry = groups.entry((to.id.clone(), to_side.to_string())).or_insert_with(|| (to.clone(), Vec::new()));
        entry.1.push(GroupItem {
            relation_index: index,
            endpoint: "to",
            side: to_side.to_string(),
            counterpart_cx: from.cx,
            counterpart_cy: from.cy,
            key,
        });
    }

    for (_, (rect, items)) in groups.iter_mut() {
        if items.len() < 2 {
            continue;
        }
        let vertical_side = items[0].side == "left" || items[0].side == "right";
        items.sort_by(|a, b| {
            let a_coord = if vertical_side { a.counterpart_cy } else { a.counterpart_cx };
            let b_coord = if vertical_side { b.counterpart_cy } else { b.counterpart_cx };
            a_coord.total_cmp(&b_coord).then_with(|| a.key.cmp(&b.key))
        });
        let extent = if vertical_side { rect.height } else { rect.width };
        let usable = (extent - 32.0).max(0.0);
        let spacing = 14.0_f64.min(usable / (items.len() - 1) as f64);
        if !(spacing > 0.0) {
            continue;
        }
        for (index, item) in items.iter().enumerate() {
            let offset = (index as f64 - (items.len() as f64 - 1.0) / 2.0) * spacing;
            let mut point = anchor(rect, &item.side);
            if vertical_side {
                point.1 += offset;
            } else {
                point.0 += offset;
            }
            let entry = spread.entry(item.relation_index).or_insert((None, None));
            if item.endpoint == "from" {
                entry.0 = Some(point);
            } else {
                entry.1 = Some(point);
            }
        }
    }
    spread
}

/// `polylinePath` — plain `M ... L ...` path without corner rounding.
pub fn polyline_path(points: &[Pt]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(index, (x, y))| format!("{} {} {}", if index == 0 { "M" } else { "L" }, num(*x), num(*y)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `routePointsValue` — the `data-composition-points` attribute.
pub fn route_points_value(points: &[Pt]) -> String {
    points
        .iter()
        .filter(|p| p.0.is_finite() && p.1.is_finite())
        .map(|(x, y)| format!("{},{}", num(*x), num(*y)))
        .collect::<Vec<_>>()
        .join(";")
}

/// `roundedPath` — polyline with Q-rounded corners at `radius`.
pub fn rounded_path(points: &[Pt], radius: f64) -> String {
    if points.len() < 3 || radius <= 0.0 {
        return polyline_path(points);
    }
    let mut commands = vec![format!("M {} {}", num(points[0].0), num(points[0].1))];
    for i in 1..points.len() - 1 {
        let (px, py) = points[i - 1];
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];
        let prev_len = (cx - px).hypot(cy - py);
        let next_len = (nx - cx).hypot(ny - cy);
        let r = radius.min(prev_len / 2.0).min(next_len / 2.0);
        if r < 1.0 {
            commands.push(format!("L {} {}", num(cx), num(cy)));
            continue;
        }
        let before = (cx - ((cx - px) / prev_len) * r, cy - ((cy - py) / prev_len) * r);
        let after = (cx + ((nx - cx) / next_len) * r, cy + ((ny - cy) / next_len) * r);
        commands.push(format!("L {} {}", num(before.0), num(before.1)));
        commands.push(format!("Q {} {} {} {}", num(cx), num(cy), num(after.0), num(after.1)));
    }
    let (end_x, end_y) = *points.last().unwrap();
    commands.push(format!("L {} {}", num(end_x), num(end_y)));
    commands.join(" ")
}

/// `labelPoint` — shared label placement for edges/flows/transitions.
pub fn label_point(label_at: Option<&Vec<f64>>, label_dx: Option<f64>, label_dy: Option<f64>, label_segment: Option<usize>, points: &[Pt]) -> Pt {
    if let Some(at) = label_at {
        if at.len() == 2 {
            return (at[0], at[1]);
        }
    }
    let dx = label_dx.unwrap_or(0.0);
    let dy = label_dy.unwrap_or(0.0);
    if points.len() == 2 {
        return ((points[0].0 + points[1].0) / 2.0 + dx, points[0].1 - 10.0 + dy);
    }
    let segment_index = (points.len() - 2).min(label_segment.unwrap_or(1).max(0));
    let a = points[segment_index];
    let b = points[segment_index + 1];
    ((a.0 + b.0) / 2.0 + dx, (a.1 + b.1) / 2.0 - 10.0 + dy)
}

/// `rectsOverlap` — two boxes separated by at least `gap` do not overlap.
pub fn rects_overlap(a: &Rect, b: &Rect, gap: f64) -> bool {
    !(a.x + a.width + gap <= b.x
        || b.x + b.width + gap <= a.x
        || a.y + a.height + gap <= b.y
        || b.y + b.height + gap <= a.y)
}

fn point_in_box(point: Pt, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    point.0 >= x1 && point.0 <= x2 && point.1 >= y1 && point.1 <= y2
}

fn orientation(a: Pt, b: Pt, c: Pt) -> i8 {
    let value = (b.1 - a.1) * (c.0 - b.0) - (b.0 - a.0) * (c.1 - b.1);
    if value.abs() < 0.0001 {
        0
    } else if value > 0.0 {
        1
    } else {
        2
    }
}

fn on_segment(a: Pt, b: Pt, c: Pt) -> bool {
    b.0 <= a.0.max(c.0) && b.0 >= a.0.min(c.0) && b.1 <= a.1.max(c.1) && b.1 >= a.1.min(c.1)
}

fn segments_intersect(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    if o1 == 0 && on_segment(a, c, b) {
        return true;
    }
    if o2 == 0 && on_segment(a, d, b) {
        return true;
    }
    if o3 == 0 && on_segment(c, a, d) {
        return true;
    }
    if o4 == 0 && on_segment(c, b, d) {
        return true;
    }
    o1 != o2 && o3 != o4
}

/// `segmentIntersectsRect` — with an optional clearance gap.
pub fn segment_intersects_rect(start: Pt, end: Pt, rect: &Rect, gap: f64) -> bool {
    let x1 = rect.x - gap;
    let y1 = rect.y - gap;
    let x2 = rect.x + rect.width + gap;
    let y2 = rect.y + rect.height + gap;
    if point_in_box(start, x1, y1, x2, y2) || point_in_box(end, x1, y1, x2, y2) {
        return true;
    }
    segments_intersect(start, end, (x1, y1), (x2, y1))
        || segments_intersect(start, end, (x2, y1), (x2, y2))
        || segments_intersect(start, end, (x2, y2), (x1, y2))
        || segments_intersect(start, end, (x1, y2), (x1, y1))
}

pub const COMPONENT_FILL: [(&str, &str); 7] = [
    ("frontend", "c-frontend"),
    ("backend", "c-backend"),
    ("database", "c-database"),
    ("cloud", "c-cloud"),
    ("security", "c-security"),
    ("messagebus", "c-messagebus"),
    ("external", "c-external"),
];

pub const COMPONENT_TEXT: [(&str, &str); 7] = [
    ("frontend", "t-frontend"),
    ("backend", "t-backend"),
    ("database", "t-database"),
    ("cloud", "t-cloud"),
    ("security", "t-security"),
    ("messagebus", "t-messagebus"),
    ("external", "t-external"),
];

pub fn component_fill(kind: &str) -> &'static str {
    COMPONENT_FILL.iter().find(|(k, _)| *k == kind).map(|(_, v)| *v).unwrap_or("c-external")
}

pub fn component_text(kind: &str) -> &'static str {
    COMPONENT_TEXT.iter().find(|(k, _)| *k == kind).map(|(_, v)| *v).unwrap_or("t-muted")
}

pub const ARROW_CLASS_MAP: [(&str, &str, &str); 4] = [
    ("default", "a-default", "arrowhead"),
    ("emphasis", "a-emphasis", "arrowhead-emphasis"),
    ("security", "a-security", "arrowhead-security"),
    ("dashed", "a-dashed", "arrowhead-dashed"),
];

pub fn arrow_class(variant: &str) -> (&'static str, &'static str) {
    ARROW_CLASS_MAP
        .iter()
        .find(|(k, ..)| *k == variant)
        .map(|(_, cls, marker)| (*cls, *marker))
        .unwrap_or(("a-default", "arrowhead"))
}

/// `variantAccent` — label text class per variant.
pub fn variant_accent(variant: Option<&str>, dashed: &str) -> &'static str {
    match variant {
        Some("security") => "t-security",
        Some("emphasis") => "t-backend",
        Some("dashed") => {
            if dashed == "t-messagebus" {
                "t-messagebus"
            } else {
                "t-database"
            }
        }
        _ => "t-muted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_units_count_fullwidth_double() {
        assert_eq!(text_units("Hello"), 5);
        assert_eq!(text_units("服务"), 4);
        assert_eq!(text_units("a服务b"), 6);
    }

    #[test]
    fn num_formats_like_js() {
        assert_eq!(num(36.0), "36");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(53.199999999999996), "53.199999999999996");
        assert_eq!(num(1.8), "1.8");
    }

    #[test]
    fn rounded_path_rounds_corners() {
        let points = vec![(558.0, 330.0), (594.0, 330.0), (594.0, 226.0), (630.0, 226.0)];
        let d = rounded_path(&points, 8.0);
        assert_eq!(
            d,
            "M 558 330 L 586 330 Q 594 330 594 322 L 594 234 Q 594 226 602 226 L 630 226"
        );
    }

    #[test]
    fn normalize_collinear_points() {
        let points = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 5.0)];
        let normalized = normalize_route_points(&points);
        assert_eq!(normalized, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]);
    }

    #[test]
    fn fitted_font_size_shrinks_to_fit() {
        assert_eq!(text_fit::fitted_font_size("web + mobile", 122.0, 9.0, 6.0), 9.0);
        // 40 units of text in a 60px box at preferred 11 -> shrinks
        assert_eq!(text_fit::fitted_font_size("a very long label that spills", 60.0, 11.0, 9.0), 9.0);
    }
}
