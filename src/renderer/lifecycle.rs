//! Lifecycle diagram renderer — faithful port of the original
//! `renderers/lifecycle/render-lifecycle.mjs`.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use super::geometry::{
    anchor, arrow_class, arr, automatic_port_spread, chosen_side, default_from_side, default_to_side, esc, label_point, num,
    route_points_value, rounded_path, text_fit, text_units, variant_accent, Pt, Rect,
};
use super::legend::{legend_layout, render_legend, resolve_legend};
use super::{
    animate_attr, focus_edge_attrs, focus_node_attrs, focus_node_title, render_definitions, render_semantic_sigil, svg_accessible_text,
    svg_root_attrs,
};

const LAYOUT: LifecycleLayout = LifecycleLayout {
    phase_y: 126.0,
    event_y: 278.0,
    outcome_y: 450.0,
    phase_w: 118.0,
    phase_h: 62.0,
    event_w: 126.0,
    event_h: 58.0,
    outcome_w: 118.0,
    outcome_h: 58.0,
    phase_xs: [94.0, 248.0, 402.0, 556.0, 710.0],
    event_xs: [402.0, 556.0, 710.0],
    outcome_xs: [402.0, 556.0, 710.0],
};

struct LifecycleLayout {
    phase_y: f64,
    event_y: f64,
    outcome_y: f64,
    phase_w: f64,
    phase_h: f64,
    event_w: f64,
    event_h: f64,
    outcome_w: f64,
    outcome_h: f64,
    phase_xs: [f64; 5],
    event_xs: [f64; 3],
    outcome_xs: [f64; 3],
}

const TYPE_CLASS: [(&str, &str); 8] = [
    ("start", "c-frontend"),
    ("active", "c-backend"),
    ("waiting", "c-cloud"),
    ("decision", "c-security"),
    ("success", "c-database"),
    ("failure", "c-security"),
    ("neutral", "c-external"),
    ("external", "c-external"),
];

const TEXT_CLASS: [(&str, &str); 8] = [
    ("start", "t-frontend"),
    ("active", "t-backend"),
    ("waiting", "t-cloud"),
    ("decision", "t-security"),
    ("success", "t-database"),
    ("failure", "t-security"),
    ("neutral", "t-muted"),
    ("external", "t-muted"),
];

const STATE_TEXT_FIT: (f64, f64, f64, f64) = (7.0, 6.0, 7.0, 6.0); // sublabelPreferred, sublabelMinimum, tagPreferred, tagMinimum

const LEGEND_CATALOG: [(&str, &str); 8] = [
    ("start", "start"),
    ("active", "active state"),
    ("waiting", "waiting"),
    ("decision", "decision"),
    ("success", "terminal success"),
    ("failure", "failure / exit"),
    ("neutral", "neutral"),
    ("external", "external"),
];

type MeasuredState = (Rect, String, String, Option<String>, Option<String>, String, usize, Option<String>); // rect, type, label, sublabel, tag, lane, col, step

pub fn render_svg(lifecycle: &Value) -> Result<String> {
    let view_box = lifecycle
        .get("meta")
        .and_then(|m| m.get("viewBox"))
        .and_then(Value::as_array)
        .map(|p| (p[0].as_f64().unwrap_or(980.0), p[1].as_f64().unwrap_or(660.0)))
        .unwrap_or((980.0, 660.0));

    let states = measure_states(lifecycle);
    let lanes: Vec<&Value> = arr(lifecycle.get("lanes")).iter().collect();
    let lane_labels: HashMap<String, String> = lanes.iter().filter_map(|lane| Some((lane.get("id")?.as_str()?.to_string(), lane.get("label")?.as_str()?.to_string()))).collect();
    let transitions: Vec<&Value> = arr(lifecycle.get("transitions")).iter().collect();

    let state_steps = state_steps(lifecycle);
    let state_boxes: HashMap<String, Rect> = states.iter().map(|(id, (rect, ..))| (id.clone(), rect.clone())).collect();
    let automatic_ports = automatic_port_spread(&transitions, &state_boxes);
    let routed: HashMap<usize, Vec<Pt>> = transitions
        .iter()
        .enumerate()
        .map(|(index, transition)| (index, path_for(transition, &states, &automatic_ports, index)))
        .collect();

    let present_kinds: Vec<String> = states.values().map(|(_, kind, ..)| kind.clone()).collect();
    let legend_entries = resolve_legend(
        lifecycle.get("meta").and_then(|m| m.get("legend")),
        &LEGEND_CATALOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect::<Vec<_>>(),
        &present_kinds,
    );

    let svg = format!(
        "      <svg viewBox=\"0 0 {} {}\" {}>
{}
{}

        <!-- Background Grid -->
        <rect width=\"100%\" height=\"100%\" fill=\"url(#grid)\" />

        <!-- Lifecycle bands -->
{}

        <!-- Primary lifecycle rail -->
{}

        <!-- Transition paths -->
{}

        <!-- States -->
{}

        <!-- Transition labels -->
{}

        <!-- Legend -->
{}
      </svg>",
        num(view_box.0),
        num(view_box.1),
        svg_root_attrs(lifecycle.get("meta").unwrap_or(&Value::Null), "lifecycle diagram"),
        svg_accessible_text(lifecycle.get("meta").unwrap_or(&Value::Null), "lifecycle diagram"),
        render_definitions(),
        render_bands(lifecycle, &lane_labels, view_box),
        render_lifecycle_rail(&states, view_box),
        transitions
            .iter()
            .enumerate()
            .map(|(index, transition)| render_transition_path(lifecycle, transition, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        states
            .iter()
            .map(|(id, state)| render_state(lifecycle, id, state, &lane_labels, &state_steps))
            .collect::<Vec<_>>()
            .join("\n\n"),
        transitions
            .iter()
            .enumerate()
            .map(|(index, transition)| render_transition_label(transition, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        render_legend_block(lifecycle, &legend_entries, &states, view_box),
    );
    Ok(svg)
}

fn band_for(lane: &str) -> &'static str {
    if lane == "main" {
        "phase"
    } else if lane == "terminal" {
        "outcome"
    } else {
        "event"
    }
}

fn measure_states(lifecycle: &Value) -> HashMap<String, MeasuredState> {
    let mut states = HashMap::new();
    for state in arr(lifecycle.get("states")) {
        let Some(id) = state.get("id").and_then(Value::as_str) else {
            continue;
        };
        let lane = state.get("lane").and_then(Value::as_str).unwrap_or("");
        let band = band_for(lane);
        let is_phase = band == "phase";
        let is_outcome = band == "outcome";
        let width = state.get("width").and_then(Value::as_f64).unwrap_or(if is_phase { LAYOUT.phase_w } else if is_outcome { LAYOUT.outcome_w } else { LAYOUT.event_w });
        let height = state.get("height").and_then(Value::as_f64).unwrap_or(if is_phase { LAYOUT.phase_h } else if is_outcome { LAYOUT.outcome_h } else { LAYOUT.event_h });
        let xs: &[f64] = if is_phase { &LAYOUT.phase_xs } else if is_outcome { &LAYOUT.outcome_xs } else { &LAYOUT.event_xs };
        let col = state.get("col").and_then(Value::as_u64).unwrap_or(0) as usize;
        let cx = xs.get(col).copied().unwrap_or(*xs.last().unwrap());
        let y = (if is_phase { LAYOUT.phase_y } else if is_outcome { LAYOUT.outcome_y } else { LAYOUT.event_y }) + state.get("yOffset").and_then(Value::as_f64).unwrap_or(0.0);
        states.insert(
            id.to_string(),
            (
                Rect::new(id, cx - width / 2.0, y, width, height),
                state.get("type").and_then(Value::as_str).unwrap_or("neutral").to_string(),
                state.get("label").and_then(Value::as_str).unwrap_or(id).to_string(),
                state.get("sublabel").and_then(Value::as_str).map(String::from),
                state.get("tag").and_then(Value::as_str).map(String::from),
                lane.to_string(),
                col,
                state.get("step").and_then(Value::as_str).map(String::from),
            ),
        );
    }
    states
}

fn state_steps(lifecycle: &Value) -> HashMap<String, f64> {
    let mut steps: HashMap<String, f64> = HashMap::new();
    for (index, transition) in arr(lifecycle.get("transitions")).iter().enumerate() {
        if let Some(from) = transition.get("from").and_then(Value::as_str) {
            steps.entry(from.to_string()).or_insert(index as f64);
        }
        if let Some(to) = transition.get("to").and_then(Value::as_str) {
            steps.entry(to.to_string()).or_insert(index as f64 + 1.0);
        }
    }
    for (index, state) in arr(lifecycle.get("states")).iter().enumerate() {
        if let Some(id) = state.get("id").and_then(Value::as_str) {
            steps.entry(id.to_string()).or_insert(index as f64);
        }
    }
    steps
}

fn route_via(transition: &Value, from_lane: &str, to_lane: &str, from: &Rect, to: &Rect, start: Pt, end: Pt) -> Vec<Pt> {
    if let Some(via) = transition.get("via").and_then(Value::as_array) {
        return via.iter().filter_map(|p| p.as_array()).filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect();
    }
    match transition.get("route").and_then(Value::as_str).unwrap_or("auto") {
        "straight" => Vec::new(),
        "drop" => {
            let y = transition.get("channelY").and_then(Value::as_f64).unwrap_or((start.1 + end.1) / 2.0);
            vec![(start.0, y), (end.0, y)]
        }
        "bottom-channel" => {
            let y = transition.get("channelY").and_then(Value::as_f64).unwrap_or((from.y + from.height).max(to.y + to.height) + 34.0);
            vec![(start.0, y), (end.0, y)]
        }
        "top-channel" => {
            let y = transition.get("channelY").and_then(Value::as_f64).unwrap_or(from.y.min(to.y) - 28.0);
            vec![(start.0, y), (end.0, y)]
        }
        "right-channel" => {
            let x = transition.get("channelX").and_then(Value::as_f64).unwrap_or((from.x + from.width).max(to.x + to.width) + 36.0);
            vec![(x, start.1), (x, end.1)]
        }
        "left-channel" => {
            let x = transition.get("channelX").and_then(Value::as_f64).unwrap_or(from.x.min(to.x) - 36.0);
            vec![(x, start.1), (x, end.1)]
        }
        _ => {
            if from_lane == to_lane {
                return Vec::new();
            }
            let y = transition.get("channelY").and_then(Value::as_f64).unwrap_or((start.1 + end.1) / 2.0);
            vec![(start.0, y), (end.0, y)]
        }
    }
}

fn path_for(
    transition: &Value,
    states: &HashMap<String, MeasuredState>,
    automatic_ports: &HashMap<usize, (Option<Pt>, Option<Pt>)>,
    index: usize,
) -> Vec<Pt> {
    let from_id = transition.get("from").and_then(Value::as_str).unwrap_or("");
    let to_id = transition.get("to").and_then(Value::as_str).unwrap_or("");
    let (Some((from_rect, _, _, _, _, from_lane, _, _)), Some((to_rect, _, _, _, _, to_lane, _, _))) = (states.get(from_id), states.get(to_id)) else {
        return Vec::new();
    };
    let from_side = chosen_side(transition.get("fromSide").and_then(Value::as_str), default_from_side(from_rect, to_rect));
    let to_side = chosen_side(transition.get("toSide").and_then(Value::as_str), default_to_side(from_rect, to_rect));
    let ports = automatic_ports.get(&index);
    let start = ports.and_then(|p| p.0).unwrap_or_else(|| anchor(from_rect, from_side));
    let end = ports.and_then(|p| p.1).unwrap_or_else(|| anchor(to_rect, to_side));
    let mut via = route_via(transition, from_lane, to_lane, from_rect, to_rect, start, end);
    if ports.is_some() && via.is_empty() && (start.0 - end.0).abs() >= 4.0 && (start.1 - end.1).abs() >= 4.0 {
        let mid_x = (start.0 + end.0) / 2.0;
        via = vec![(mid_x, start.1), (mid_x, end.1)];
    }
    let mut points = vec![start];
    points.extend(via);
    points.push(end);
    points
}

fn render_bands(lifecycle: &Value, lane_labels: &HashMap<String, String>, view_box: Pt) -> String {
    let right = view_box.0 - 72.0;
    let lanes: Vec<&Value> = arr(lifecycle.get("lanes")).iter().collect();
    let main_lane = lanes.iter().find(|lane| lane.get("id").and_then(Value::as_str) == Some("main"));
    let terminal_lane = lanes.iter().find(|lane| lane.get("id").and_then(Value::as_str) == Some("terminal"));
    let event_lanes: Vec<&Value> = lanes.iter().filter(|lane| {
        let id = lane.get("id").and_then(Value::as_str);
        id != Some("main") && id != Some("terminal")
    }).copied().collect();
    let titles = [
        main_lane.and_then(|l| l.get("label")).and_then(Value::as_str).unwrap_or("Lifecycle phases").to_string(),
        if event_lanes.is_empty() {
            "Interruptions + recovery".to_string()
        } else {
            event_lanes.iter().filter_map(|l| l.get("label").and_then(Value::as_str)).collect::<Vec<_>>().join(" + ")
        },
        terminal_lane.and_then(|l| l.get("label")).and_then(Value::as_str).unwrap_or("Outcomes").to_string(),
    ];
    let _ = lane_labels;
    format!(
        "        <path d=\"M 72 112 L {} 112\" class=\"a-default\" stroke-width=\"0.8\" stroke-dasharray=\"3,8\"/>\n        <text x=\"72\" y=\"100\" class=\"t-dim\" font-size=\"10\" font-weight=\"600\">01 / {}</text>\n        <path d=\"M 72 264 L {} 264\" class=\"a-default\" stroke-width=\"0.8\" stroke-dasharray=\"3,8\"/>\n        <text x=\"72\" y=\"252\" class=\"t-dim\" font-size=\"10\" font-weight=\"600\">02 / {}</text>\n        <path d=\"M 72 436 L {} 436\" class=\"a-default\" stroke-width=\"0.8\" stroke-dasharray=\"3,8\"/>\n        <text x=\"72\" y=\"424\" class=\"t-dim\" font-size=\"10\" font-weight=\"600\">03 / {}</text>",
        num(right),
        esc(&titles[0]),
        num(right),
        esc(&titles[1]),
        num(right),
        esc(&titles[2])
    )
}

fn render_lifecycle_rail(states: &HashMap<String, MeasuredState>, view_box: Pt) -> String {
    let main_cols: Vec<usize> = states
        .values()
        .filter(|(_, _, _, _, _, lane, ..)| band_for(lane) == "phase")
        .map(|(_, _, _, _, _, _, col, _)| *col)
        .collect();
    if main_cols.is_empty() {
        return String::new();
    }
    let rail_end = LAYOUT.phase_xs[*main_cols.iter().max().unwrap()] + 38.0;
    let _ = view_box;
    format!(
        "        <path d=\"M 154 {} L {} {}\" class=\"a-emphasis\" stroke-width=\"2.2\" marker-end=\"url(#arrowhead-emphasis)\"/>",
        num(LAYOUT.phase_y + 31.0),
        num(rail_end),
        num(LAYOUT.phase_y + 31.0)
    )
}

fn render_transition_path(lifecycle: &Value, transition: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let (cls, marker) = arrow_class(transition.get("variant").and_then(Value::as_str).unwrap_or("default"));
    let points = routed.get(&index).cloned().unwrap_or_default();
    let stroke_width = transition
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(if transition.get("variant").and_then(Value::as_str) == Some("emphasis") { 2.0 } else { 1.1 });
    let corner_radius = transition.get("cornerRadius").and_then(Value::as_f64).unwrap_or(10.0);
    let label = transition.get("label").and_then(Value::as_str);
    let id = transition.get("id").and_then(Value::as_str);
    let from = transition.get("from").and_then(Value::as_str).unwrap_or("");
    let to = transition.get("to").and_then(Value::as_str).unwrap_or("");
    format!(
        "        <path {} data-composition-points=\"{}\" d=\"{}\" class=\"{}\"{} stroke-width=\"{}\" marker-end=\"url(#{})\"/>",
        focus_edge_attrs(from, to, label, Some(index), id),
        route_points_value(&points),
        rounded_path(&points, corner_radius),
        cls,
        animate_attr(lifecycle.get("meta").unwrap_or(&Value::Null), "edge", Some(index as f64)),
        num(stroke_width),
        marker
    )
}

fn render_transition_label(transition: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let Some(label) = transition.get("label").and_then(Value::as_str) else {
        return String::new();
    };
    let points = routed.get(&index).cloned().unwrap_or_default();
    let label_at = transition.get("labelAt").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_f64).collect());
    let (lx, ly) = label_point(
        label_at.as_ref(),
        transition.get("labelDx").and_then(Value::as_f64),
        transition.get("labelDy").and_then(Value::as_f64),
        transition.get("labelSegment").and_then(Value::as_u64).map(|v| v as usize),
        &points,
    );
    let note = transition.get("note").and_then(Value::as_str);
    let longest_line = text_units(label).max(text_units(note.unwrap_or("")));
    let label_w = (longest_line as f64 * 4.9 + 12.0).max(32.0);
    let label_h = if note.is_some() { 27.0 } else { 16.0 };
    let note_html = note
        .filter(|n| !n.is_empty())
        .map(|note| {
            format!(
                "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"t-dim\" font-size=\"7\" text-anchor=\"middle\">{}</text>",
                num(lx),
                num(ly + 11.0),
                esc(note)
            )
        })
        .unwrap_or_default();
    let from = transition.get("from").and_then(Value::as_str).unwrap_or("");
    let to = transition.get("to").and_then(Value::as_str).unwrap_or("");
    let id = transition.get("id").and_then(Value::as_str);
    format!(
        "        <g data-detail=\"context\" {}>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"8\" text-anchor=\"middle\">{}</text>{}\n        </g>",
        focus_edge_attrs(from, to, Some(label), Some(index), id),
        num(lx - label_w / 2.0),
        num(ly - 11.0),
        num(label_w),
        num(label_h),
        num(lx),
        num(ly),
        variant_accent(transition.get("variant").and_then(Value::as_str), "t-messagebus"),
        esc(label),
        note_html
    )
}

fn render_state(
    lifecycle: &Value,
    id: &str,
    state: &MeasuredState,
    lane_labels: &HashMap<String, String>,
    state_steps: &HashMap<String, f64>,
) -> String {
    let (rect, kind, label, sublabel, tag, lane, _, step) = state;
    let fill = TYPE_CLASS.iter().find(|(k, _)| k == kind).map(|(_, v)| *v).unwrap_or("c-external");
    let accent = TEXT_CLASS.iter().find(|(k, _)| k == kind).map(|(_, v)| *v).unwrap_or("t-muted");
    let has_sub = sublabel.as_ref().is_some_and(|s| !s.is_empty());
    let sub = if has_sub {
        let font = text_fit::fitted_font_size(sublabel.as_deref().unwrap(), rect.width, STATE_TEXT_FIT.0, STATE_TEXT_FIT.1);
        format!(
            "\n          <text data-detail=\"context\" x=\"{}\" y=\"{}\" class=\"t-muted\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(rect.cx),
            num(rect.y + 37.0),
            num(font),
            esc(sublabel.as_deref().unwrap())
        )
    } else {
        String::new()
    };
    let tag_html = if let Some(tag) = tag.as_deref().filter(|t| !t.is_empty()) {
        let font = text_fit::fitted_font_size(tag, rect.width, STATE_TEXT_FIT.2, STATE_TEXT_FIT.3);
        format!(
            "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(rect.cx),
            num(rect.y + rect.height - 11.0),
            accent,
            num(font),
            esc(tag)
        )
    } else {
        String::new()
    };
    let step_html = if let Some(step) = step.as_deref().filter(|s| !s.is_empty()) {
        format!(
            "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"7\" font-weight=\"700\">{}</text>",
            num(rect.x + 10.0),
            num(rect.y + 14.0),
            accent,
            esc(step)
        )
    } else {
        String::new()
    };
    let context = lane_labels.get(lane).cloned().unwrap_or_else(|| "Lifecycle state".to_string());
    let node_step = state_steps.get(id).copied();
    let step_attr = animate_attr(lifecycle.get("meta").unwrap_or(&Value::Null), "node", node_step);
    let passport_sublabel = sublabel.as_deref();
    let passport_tag = tag.as_deref();
    format!(
        "        <g {}>\n          {}\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"7\" class=\"c-mask\"/>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"7\" class=\"{}\"{} stroke-width=\"1.5\"/>\n          {}\n          <text{} x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"10\" font-weight=\"600\" text-anchor=\"middle\">{}</text>{}{}{}\n        </g>",
        focus_node_attrs(id, label, Some(kind), passport_sublabel, passport_tag, Some(&context)),
        focus_node_title(label, passport_sublabel, Some(&context), passport_tag),
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
        num(rect.cx),
        num(rect.y + 21.0),
        esc(label),
        sub,
        tag_html,
        step_html
    )
}

fn legend_y(view_box: Pt) -> f64 {
    view_box.1 - 36.0
}

fn lifecycle_area_bottom(view_box: Pt) -> f64 {
    view_box.1 - 122.0
}

fn render_legend_block(
    lifecycle: &Value,
    entries: &[super::legend::LegendEntry],
    _states: &HashMap<String, MeasuredState>,
    view_box: Pt,
) -> String {
    let mut layout = legend_layout(
        40.0,
        legend_y(view_box),
        view_box.0 - 80.0,
        lifecycle_area_bottom(view_box) + 8.0,
        if lifecycle.get("meta").and_then(|m| m.get("legend")).is_none() { "hide" } else { "error" },
        "lifecycle",
    );
    let swatch = |entry: &super::legend::LegendEntry| {
        let class = TYPE_CLASS.iter().find(|(k, _)| k == &entry.kind.as_str()).map(|(_, v)| *v).unwrap_or("c-external");
        format!(
            "<rect x=\"{}\" y=\"{}\" width=\"14\" height=\"9\" rx=\"2\" class=\"{}\" stroke-width=\"1\"/>",
            num(entry.x),
            num(entry.baseline - 8.0),
            class
        )
    };
    render_legend(entries, &layout, &swatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_official_example() {
        let text = include_str!("../../examples/agent-run.lifecycle.json");
        let lifecycle: Value = serde_json::from_str(text).unwrap();
        let svg = render_svg(&lifecycle).unwrap();
        assert!(svg.contains("<!-- Lifecycle bands -->"));
        assert!(svg.contains("<!-- Primary lifecycle rail -->"));
        assert!(svg.contains("M 72 112"));
        assert!(svg.contains("01 / "));
    }
}
