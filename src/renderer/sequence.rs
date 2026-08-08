//! Sequence diagram renderer — faithful port of the original
//! `renderers/sequence/render-sequence.mjs`.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use super::geometry::{
    arrow_class, arr, chosen_side, component_fill, default_from_side, default_to_side, esc, num, route_points_value, text_fit,
    text_units, variant_accent, Pt,
};
use super::legend::{legend_layout, render_legend, resolve_legend};
use super::{
    animate_attr, focus_edge_attrs, focus_node_attrs, focus_node_title, render_definitions, render_semantic_sigil, svg_accessible_text,
    svg_root_attrs,
};

const PARTICIPANT_TEXT_FIT: (f64, f64) = (9.0, 6.0); // sublabelPreferred, sublabelMinimum

const LEGEND_CATALOG: [(&str, &str); 5] = [
    ("emphasis", "request"),
    ("return", "return"),
    ("security", "security"),
    ("dashed", "async trace"),
    ("default", "default message"),
];

pub fn render_svg(sequence: &Value) -> Result<String> {
    let view_box = sequence
        .get("meta")
        .and_then(|m| m.get("viewBox"))
        .and_then(Value::as_array)
        .map(|p| (p[0].as_f64().unwrap_or(920.0), p[1].as_f64().unwrap_or(760.0)))
        .unwrap_or((920.0, 760.0));

    let layout = SequenceLayout {
        top_y: 72.0,
        participant_w: 86.0,
        participant_h: 54.0,
        lifeline_top: 142.0,
        lifeline_bottom: view_box.1 - 65.0,
        legend_y: view_box.1 - 54.0,
        left_x: 62.0,
        col_gap: 108.0,
        label_h: 16.0,
    };

    let participants = measure_participants(sequence, &layout);
    let messages: Vec<&Value> = arr(sequence.get("messages")).iter().collect();
    let segments: Vec<&Value> = arr(sequence.get("segments")).iter().collect();
    let activations: Vec<&Value> = arr(sequence.get("activations")).iter().collect();

    let present_kinds: Vec<String> = messages
        .iter()
        .map(|m| m.get("variant").and_then(Value::as_str).unwrap_or("default").to_string())
        .collect();
    let legend_entries = resolve_legend(
        sequence.get("meta").and_then(|m| m.get("legend")),
        &LEGEND_CATALOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect::<Vec<_>>(),
        &present_kinds,
    );

    let svg = format!(
        "      <svg viewBox=\"0 0 {} {}\" {}>
{}
{}

        <!-- Background Grid -->
        <rect width=\"100%\" height=\"100%\" fill=\"url(#grid)\" />

        <!-- Time Segments -->
{}

        <!-- Lifelines -->
{}

        <!-- Activations -->
{}

        <!-- Messages -->
{}

        <!-- Segment Labels -->
{}

        <!-- Participants -->
{}

        <!-- Legend -->
{}
      </svg>",
        num(view_box.0),
        num(view_box.1),
        svg_root_attrs(sequence.get("meta").unwrap_or(&Value::Null), "sequence diagram"),
        svg_accessible_text(sequence.get("meta").unwrap_or(&Value::Null), "sequence diagram"),
        render_definitions(),
        segments
            .iter()
            .enumerate()
            .map(|(index, segment)| render_segment(segment, index, view_box))
            .collect::<Vec<_>>()
            .join("\n\n"),
        participants
            .iter()
            .map(|(_, p)| render_lifeline(p, &layout))
            .collect::<Vec<_>>()
            .join("\n"),
        activations
            .iter()
            .map(|activation| render_activation(activation, &participants))
            .collect::<Vec<_>>()
            .join("\n"),
        messages
            .iter()
            .enumerate()
            .map(|(index, message)| render_message(sequence, message, index, &participants, &layout))
            .collect::<Vec<_>>()
            .join("\n\n"),
        segments
            .iter()
            .enumerate()
            .map(|(index, segment)| render_segment_label(segment, index, &messages, &participants, &layout, view_box))
            .collect::<Vec<_>>()
            .join("\n"),
        participants
            .iter()
            .map(|(id, p)| render_participant(sequence, id, p, &layout))
            .collect::<Vec<_>>()
            .join("\n\n"),
        render_legend_block(sequence, &legend_entries, &layout, view_box),
    );
    Ok(svg)
}

struct SequenceLayout {
    top_y: f64,
    participant_w: f64,
    participant_h: f64,
    lifeline_top: f64,
    lifeline_bottom: f64,
    legend_y: f64,
    left_x: f64,
    col_gap: f64,
    label_h: f64,
}

type MeasuredParticipant = (Pt, String, String, Option<String>, f64); // (cx, x), type, label, sublabel, index

fn participant_x(index: usize, layout: &SequenceLayout) -> f64 {
    layout.left_x + index as f64 * layout.col_gap
}

fn measure_participants(sequence: &Value, layout: &SequenceLayout) -> HashMap<String, MeasuredParticipant> {
    let mut map = HashMap::new();
    for (index, participant) in arr(sequence.get("participants")).iter().enumerate() {
        let Some(id) = participant.get("id").and_then(Value::as_str) else {
            continue;
        };
        let cx = participant_x(index, layout);
        let x = cx - layout.participant_w / 2.0;
        map.insert(
            id.to_string(),
            (
                (cx, x),
                participant.get("type").and_then(Value::as_str).unwrap_or("external").to_string(),
                participant.get("label").and_then(Value::as_str).unwrap_or(id).to_string(),
                participant.get("sublabel").and_then(Value::as_str).map(String::from),
                index as f64,
            ),
        );
    }
    map
}

fn message_geometry(message: &Value, participants: &HashMap<String, MeasuredParticipant>) -> Option<(f64, f64, f64)> {
    let from = participants.get(message.get("from").and_then(Value::as_str)?)?;
    let to = participants.get(message.get("to").and_then(Value::as_str)?)?;
    let y = message.get("y").and_then(Value::as_f64)?;
    let direction = if to.0 .0 > from.0 .0 { 1.0 } else { -1.0 };
    let start = from.0 .0 + direction * 7.0;
    let end = to.0 .0 - direction * 7.0;
    Some((start, end, (start + end) / 2.0))
}

fn message_label_box(message: &Value, participants: &HashMap<String, MeasuredParticipant>, label_h: f64) -> Option<(f64, f64, f64)> {
    let (_, _, center) = message_geometry(message, participants)?;
    let width = (text_units(message.get("label").and_then(Value::as_str).unwrap_or("")) as f64 * 5.2 + 12.0).max(34.0);
    Some((center - width / 2.0, message.get("y").and_then(Value::as_f64)? - 20.0, width))
}

fn message_route_box(message: &Value, participants: &HashMap<String, MeasuredParticipant>) -> Option<(f64, f64, f64)> {
    let (start, end, _) = message_geometry(message, participants)?;
    Some((start.min(end), message.get("y").and_then(Value::as_f64)? - 2.0, (end - start).abs()))
}

fn render_segment(segment: &Value, index: usize, view_box: Pt) -> String {
    let from = segment.get("from").and_then(Value::as_f64).unwrap_or(0.0);
    let to = segment.get("to").and_then(Value::as_f64).unwrap_or(0.0);
    format!(
        "        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"segment\" data-composition-frame-id=\"{}\" x=\"48\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\" class=\"c-lane\" stroke-width=\"1\"/>",
        index,
        num(from),
        num(view_box.0 - 96.0),
        num(to - from)
    )
}

fn render_segment_label(
    segment: &Value,
    index: usize,
    messages: &[&Value],
    participants: &HashMap<String, MeasuredParticipant>,
    layout: &SequenceLayout,
    view_box: Pt,
) -> String {
    let label = segment.get("label").and_then(Value::as_str).unwrap_or("");
    let label_w = (text_units(label) as f64 * 5.2 + 14.0).max(42.0);
    let occupied: Vec<(f64, f64, f64, f64)> = messages
        .iter()
        .filter_map(|m| message_label_box(m, participants, layout.label_h).map(|(x, y, w)| (x, y, w, layout.label_h)))
        .chain(
            messages
                .iter()
                .filter_map(|m| message_route_box(m, participants).map(|(x, y, w)| (x, y, w, 4.0))),
        )
        .collect();
    let segment_from = segment.get("from").and_then(Value::as_f64).unwrap_or(0.0);
    let mut label_y = segment_from - 22.0;
    let mut label = (56.0, label_y, label_w, 18.0);
    for _attempt in 0..4 {
        let overlaps = occupied.iter().any(|&(ox, oy, ow, oh)| {
            label.0 < ox + ow + 2.0 && ox < label.0 + label.2 + 2.0 && label.1 < oy + oh + 2.0 && oy < label.1 + label.3 + 2.0
        });
        if !overlaps {
            break;
        }
        label_y -= 22.0;
        label.1 = label_y;
    }
    let _ = view_box;
    format!(
        "        <g data-graph-role=\"segment-label\" data-segment-id=\"{}\">\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"t-dim\" font-size=\"9\" font-weight=\"600\">{}</text>\n        </g>",
        index,
        num(label.0),
        num(label.1),
        num(label.2),
        num(label.3),
        num(label.0 + 6.0),
        num(label.1 + 13.0),
        esc(label_text(segment))
    )
}

fn label_text(segment: &Value) -> &str {
    segment.get("label").and_then(Value::as_str).unwrap_or("")
}

fn render_activation(activation: &Value, participants: &HashMap<String, MeasuredParticipant>) -> String {
    let Some(participant) = participants.get(activation.get("participant").and_then(Value::as_str).unwrap_or("")) else {
        return String::new();
    };
    let fill = component_fill(activation.get("type").and_then(Value::as_str).unwrap_or(&participant.1));
    let x = participant.0 .0 - 5.0;
    let from = activation.get("from").and_then(Value::as_f64).unwrap_or(0.0);
    let to = activation.get("to").and_then(Value::as_f64).unwrap_or(0.0);
    let height = to - from;
    format!(
        "        <rect x=\"{}\" y=\"{}\" width=\"10\" height=\"{}\" rx=\"3\" class=\"c-mask\"/>\n        <rect x=\"{}\" y=\"{}\" width=\"10\" height=\"{}\" rx=\"3\" class=\"{}\" stroke-width=\"1\"/>",
        num(x),
        num(from),
        num(height),
        num(x),
        num(from),
        num(height),
        fill
    )
}

fn message_label(message: &Value, x1: f64, x2: f64, participants: &HashMap<String, MeasuredParticipant>, layout: &SequenceLayout) -> String {
    let label = message.get("label").and_then(Value::as_str).unwrap_or("");
    let box_data = message_label_box(message, participants, layout.label_h);
    let center = box_data.map(|(x, _, w)| x + w / 2.0).unwrap_or((x1 + x2) / 2.0);
    let y = message.get("y").and_then(Value::as_f64).unwrap_or(0.0) - 10.0;
    let label_w = box_data.map(|(_, _, w)| w).unwrap_or_else(|| (text_units(label) as f64 * 5.2 + 12.0).max(34.0));
    let accent = match message.get("variant").and_then(Value::as_str) {
        Some("security") => "t-security",
        Some("dashed") => "t-messagebus",
        Some("return") => "t-muted",
        _ => "t-backend",
    };
    format!(
        "        <g data-detail=\"context\">\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"9\" text-anchor=\"middle\">{}</text>\n        </g>",
        num(center - label_w / 2.0),
        num(y - 10.0),
        num(label_w),
        num(layout.label_h),
        num(center),
        num(y),
        accent,
        esc(label)
    )
}

fn render_message(
    sequence: &Value,
    message: &Value,
    index: usize,
    participants: &HashMap<String, MeasuredParticipant>,
    layout: &SequenceLayout,
) -> String {
    let Some((start, end, _)) = message_geometry(message, participants) else {
        return String::new();
    };
    let (cls, marker) = arrow_class(message.get("variant").and_then(Value::as_str).unwrap_or("default"));
    let stroke_width = if message.get("variant").and_then(Value::as_str) == Some("emphasis") { 1.8 } else { 1.4 };
    let dash = if message.get("variant").and_then(Value::as_str) == Some("return") { " stroke-dasharray=\"3,5\"" } else { "" };
    let y = message.get("y").and_then(Value::as_f64).unwrap_or(0.0);
    let note = message
        .get("note")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .map(|note| {
            format!(
                "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"t-dim\" font-size=\"7\">{}</text>",
                num(start.min(end) + 12.0),
                num(y + 18.0),
                esc(note)
            )
        })
        .unwrap_or_default();
    let from = message.get("from").and_then(Value::as_str).unwrap_or("");
    let to = message.get("to").and_then(Value::as_str).unwrap_or("");
    let id = message.get("id").and_then(Value::as_str);
    let label = message.get("label").and_then(Value::as_str);
    let id_attr = id
        .map(|id| format!(" data-composition-edge-id=\"{}\"", esc(id)))
        .unwrap_or_default();
    format!(
        "        <g {}>\n          <path data-composition-edge-from=\"{}\" data-composition-edge-to=\"{}\"{} data-composition-points=\"{}\" d=\"M {} {} L {} {}\" class=\"{}\"{} stroke-width=\"{}\"{} marker-end=\"url(#{})\"/>\n{}{}\n        </g>",
        focus_edge_attrs(from, to, label, Some(index), id),
        esc(from),
        esc(to),
        id_attr,
        route_points_value(&[(start, y), (end, y)]),
        num(start),
        num(y),
        num(end),
        num(y),
        cls,
        animate_attr(sequence.get("meta").unwrap_or(&Value::Null), "edge", Some(index as f64)),
        num(stroke_width),
        dash,
        marker,
        message_label(message, start, end, participants, layout),
        note
    )
}

fn render_lifeline(participant: &MeasuredParticipant, layout: &SequenceLayout) -> String {
    format!(
        "        <path d=\"M {} {} L {} {}\" class=\"a-default\" stroke-width=\"0.8\" stroke-dasharray=\"3,7\"/>",
        num(participant.0 .0),
        num(layout.lifeline_top),
        num(participant.0 .0),
        num(layout.lifeline_bottom)
    )
}

fn render_participant(sequence: &Value, id: &str, participant: &MeasuredParticipant, layout: &SequenceLayout) -> String {
    let (_, kind, label, sublabel, index) = participant;
    let fill = component_fill(kind);
    let has_sub = sublabel.as_ref().is_some_and(|s| !s.is_empty());
    let sub = if has_sub {
        let font = text_fit::fitted_font_size(sublabel.as_deref().unwrap(), layout.participant_w, PARTICIPANT_TEXT_FIT.0, PARTICIPANT_TEXT_FIT.1);
        format!(
            "\n          <text data-detail=\"context\" x=\"{}\" y=\"{}\" class=\"t-muted\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(participant.0 .0),
            num(layout.top_y + 39.0),
            num(font),
            esc(sublabel.as_deref().unwrap())
        )
    } else {
        String::new()
    };
    let passport_kind = kind.as_str();
    let passport_sublabel = sublabel.as_deref();
    format!(
        "        <g {}>\n          {}\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"c-mask\"/>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"{}\"{} stroke-width=\"1.5\"/>\n          {}\n          <text{} x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"11\" font-weight=\"600\" text-anchor=\"middle\">{}</text>{}\n        </g>",
        focus_node_attrs(id, label, Some(passport_kind), passport_sublabel, None, Some("Sequence participant")),
        focus_node_title(label, passport_sublabel, Some("Sequence participant"), None),
        num(participant.0 .1),
        num(layout.top_y),
        num(layout.participant_w),
        num(layout.participant_h),
        num(participant.0 .1),
        num(layout.top_y),
        num(layout.participant_w),
        num(layout.participant_h),
        fill,
        animate_attr(sequence.get("meta").unwrap_or(&Value::Null), "node", Some(*index)),
        render_semantic_sigil(kind, participant.0 .1 + 6.0, layout.top_y + 6.0, 11.0),
        if has_sub { " data-detail-anchor" } else { "" },
        num(participant.0 .0),
        num(layout.top_y + 22.0),
        esc(label),
        sub
    )
}

fn render_legend_block(sequence: &Value, entries: &[super::legend::LegendEntry], layout: &SequenceLayout, view_box: Pt) -> String {
    let catalog: Vec<(String, String, &str, &str, f64, Option<&str>)> = LEGEND_CATALOG
        .iter()
        .map(|(kind, label)| {
            let (class_name, marker, stroke_width, dash) = match *kind {
                "emphasis" => ("a-emphasis", "arrowhead-emphasis", 1.8, None),
                "return" => ("a-default", "arrowhead", 1.4, Some("3,5")),
                "security" => ("a-security", "arrowhead-security", 1.4, None),
                "dashed" => ("a-dashed", "arrowhead-dashed", 1.4, None),
                _ => ("a-default", "arrowhead", 1.4, None),
            };
            (kind.to_string(), label.to_string(), class_name, marker, stroke_width, dash)
        })
        .collect();
    let mut layout_legend = legend_layout(
        40.0,
        layout.legend_y,
        view_box.0 - 80.0,
        layout.legend_y - 30.0,
        if sequence.get("meta").and_then(|m| m.get("legend")).is_none() { "hide" } else { "error" },
        "sequence",
    );
    layout_legend.font_size = 8.0;
    let swatch = |entry: &super::legend::LegendEntry| {
        let item = catalog.iter().find(|(kind, ..)| *kind == entry.kind).unwrap();
        let dash_attr = item.5.map(|d| format!(" stroke-dasharray=\"{d}\"")).unwrap_or_default();
        format!(
            "<path d=\"M {} {} L {} {}\" class=\"{}\" stroke-width=\"{}\"{} marker-end=\"url(#{})\"/>",
            num(entry.x),
            num(entry.baseline - 3.0),
            num(entry.x + 34.0),
            num(entry.baseline - 3.0),
            item.2,
            num(item.4),
            dash_attr,
            item.3
        )
    };
    // The sequence legend uses swatchWidth 34 / swatchGap 9 for all entries.
    let mut with_swatches: Vec<super::legend::LegendEntry> = entries.to_vec();
    for entry in &mut with_swatches {
        entry.swatch_width = Some(34.0);
        entry.swatch_gap = Some(9.0);
    }
    render_legend(&with_swatches, &layout_legend, &swatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_official_example() {
        let text = include_str!("../../examples/cache-miss-request.sequence.json");
        let sequence: Value = serde_json::from_str(text).unwrap();
        let svg = render_svg(&sequence).unwrap();
        assert!(svg.contains("<svg viewBox=\"0 0 820 760\""));
        assert!(svg.contains("data-node-id=\"user\""));
        assert!(svg.contains("data-composition-edge-from=\"user\""));
        assert!(svg.contains("<!-- Time Segments -->"));
        assert!(svg.contains("data-graph-role=\"segment-label\""));
    }
}
