//! Data-flow diagram renderer — faithful port of the original
//! `renderers/dataflow/render-dataflow.mjs`.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use super::geometry::{
    anchor, arrow_class, arr, automatic_port_spread, chosen_side, component_fill, component_text, default_from_side, default_to_side,
    esc, label_point, num, polyline_path, route_points_value, text_fit, text_units, variant_accent, Pt, Rect,
};
use super::legend::{legend_layout, render_legend, resolve_legend};
use super::{
    animate_attr, focus_edge_attrs, focus_node_attrs, focus_node_title, render_definitions, render_semantic_sigil, svg_accessible_text,
    svg_root_attrs,
};

const LAYOUT: DataflowLayout = DataflowLayout {
    stage_y: 46.0,
    stage_h: 36.0,
    stage_bottom_pad: 74.0,
    left_x: 100.0,
    col_gap: 215.0,
    stage_w: 168.0,
    node_w: 112.0,
    node_h: 58.0,
    row_ys: [128.0, 242.0, 356.0, 470.0, 584.0],
    label_h: 16.0,
};

struct DataflowLayout {
    stage_y: f64,
    stage_h: f64,
    stage_bottom_pad: f64,
    left_x: f64,
    col_gap: f64,
    stage_w: f64,
    node_w: f64,
    node_h: f64,
    row_ys: [f64; 5],
    label_h: f64,
}

const NODE_TEXT_FIT: (f64, f64, f64, f64) = (7.0, 6.0, 7.0, 6.0); // sublabelPreferred, sublabelMinimum, tagPreferred, tagMinimum

const LEGEND_CATALOG: [(&str, &str); 5] = [
    ("emphasis", "primary data"),
    ("security", "policy / PII"),
    ("dashed", "async batch"),
    ("database", "data store"),
    ("default", "data flow"),
];

type MeasuredNode = (Rect, String, String, Option<String>, Option<String>, usize, usize); // rect, type, label, sublabel, tag, stage, row

pub fn render_svg(dataflow: &Value) -> Result<String> {
    let view_box = dataflow
        .get("meta")
        .and_then(|m| m.get("viewBox"))
        .and_then(Value::as_array)
        .map(|p| (p[0].as_f64().unwrap_or(940.0), p[1].as_f64().unwrap_or(720.0)))
        .unwrap_or((940.0, 720.0));

    let stages: Vec<&Value> = arr(dataflow.get("stages")).iter().collect();
    let nodes_raw: Vec<&Value> = arr(dataflow.get("nodes")).iter().collect();
    let flows_raw: Vec<&Value> = arr(dataflow.get("flows")).iter().collect();

    let nodes = measure_nodes(&nodes_raw);
    let node_steps = node_steps(dataflow);
    let node_boxes: HashMap<String, Rect> = nodes.iter().map(|(id, (rect, ..))| (id.clone(), rect.clone())).collect();
    let automatic_ports = automatic_port_spread(&flows_raw, &node_boxes);
    let routed: HashMap<usize, Vec<Pt>> = flows_raw
        .iter()
        .enumerate()
        .map(|(index, flow)| (index, path_for(flow, &nodes, &automatic_ports, index)))
        .collect();

    let present_kinds: Vec<String> = flows_raw
        .iter()
        .map(|flow| flow.get("variant").and_then(Value::as_str).unwrap_or("default").to_string())
        .chain(nodes.values().filter(|(_, kind, ..)| kind == "database").map(|(_, kind, ..)| kind.clone()))
        .collect();
    let legend_entries = resolve_legend(
        dataflow.get("meta").and_then(|m| m.get("legend")),
        &LEGEND_CATALOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect::<Vec<_>>(),
        &present_kinds,
    );

    let svg = format!(
        "      <svg viewBox=\"0 0 {} {}\" {}>
{}
{}

        <!-- Background Grid -->
        <rect width=\"100%\" height=\"100%\" fill=\"url(#grid)\" />

        <!-- Data Stages -->
{}

        <!-- Flow paths -->
{}

        <!-- Nodes -->
{}

        <!-- Flow labels -->
{}

        <!-- Legend -->
{}
      </svg>",
        num(view_box.0),
        num(view_box.1),
        svg_root_attrs(dataflow.get("meta").unwrap_or(&Value::Null), "data-flow diagram"),
        svg_accessible_text(dataflow.get("meta").unwrap_or(&Value::Null), "data-flow diagram"),
        render_definitions(),
        stages.iter().enumerate().map(|(index, stage)| render_stage(stage, index, view_box)).collect::<Vec<_>>().join("\n\n"),
        flows_raw
            .iter()
            .enumerate()
            .map(|(index, flow)| render_flow_path(dataflow, flow, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        nodes
            .iter()
            .map(|(id, node)| render_node(dataflow, id, node, &stages, &node_steps))
            .collect::<Vec<_>>()
            .join("\n\n"),
        flows_raw
            .iter()
            .enumerate()
            .map(|(index, flow)| render_flow_label(flow, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        render_legend_block(dataflow, &legend_entries, &nodes, &flows_raw, view_box),
    );
    Ok(svg)
}

fn stage_x(index: usize) -> f64 {
    LAYOUT.left_x + index as f64 * LAYOUT.col_gap
}

fn measure_nodes(nodes_raw: &[&Value]) -> HashMap<String, MeasuredNode> {
    let mut nodes = HashMap::new();
    for node in nodes_raw {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };
        let width = node.get("width").and_then(Value::as_f64).unwrap_or(LAYOUT.node_w);
        let height = node.get("height").and_then(Value::as_f64).unwrap_or(LAYOUT.node_h);
        let stage = node.get("stage").and_then(Value::as_u64).unwrap_or(0) as usize;
        let row = node.get("row").and_then(Value::as_u64).unwrap_or(0) as usize;
        let cx = stage_x(stage);
        let y = LAYOUT.row_ys[row.min(4)] + node.get("yOffset").and_then(Value::as_f64).unwrap_or(0.0);
        nodes.insert(
            id.to_string(),
            (
                Rect::new(id, cx - width / 2.0, y, width, height),
                node.get("type").and_then(Value::as_str).unwrap_or("external").to_string(),
                node.get("label").and_then(Value::as_str).unwrap_or(id).to_string(),
                node.get("sublabel").and_then(Value::as_str).map(String::from),
                node.get("tag").and_then(Value::as_str).map(String::from),
                stage,
                row,
            ),
        );
    }
    nodes
}

fn node_steps(dataflow: &Value) -> HashMap<String, f64> {
    let mut steps: HashMap<String, f64> = HashMap::new();
    for (index, flow) in arr(dataflow.get("flows")).iter().enumerate() {
        if let Some(from) = flow.get("from").and_then(Value::as_str) {
            steps.entry(from.to_string()).or_insert(index as f64);
        }
        if let Some(to) = flow.get("to").and_then(Value::as_str) {
            steps.entry(to.to_string()).or_insert(index as f64 + 1.0);
        }
    }
    for (index, node) in arr(dataflow.get("nodes")).iter().enumerate() {
        if let Some(id) = node.get("id").and_then(Value::as_str) {
            steps.entry(id.to_string()).or_insert(index as f64);
        }
    }
    steps
}

fn flow_label_size(flow: &Value) -> (f64, f64) {
    let label = flow.get("label").and_then(Value::as_str).unwrap_or("");
    let classification = flow.get("classification").and_then(Value::as_str).unwrap_or("");
    let longest_line = text_units(label).max(text_units(classification));
    let width = ((longest_line as f64 * 4.9 + 12.0).max(34.0) * 10.0).round() / 10.0;
    let height = if flow.get("classification").is_some() { 27.0 } else { LAYOUT.label_h };
    (width, height)
}

fn route_via(flow: &Value, from: &Rect, to: &Rect, start: Pt, end: Pt) -> Vec<Pt> {
    if let Some(via) = flow.get("via").and_then(Value::as_array) {
        return via.iter().filter_map(|p| p.as_array()).filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect();
    }
    match flow.get("route").and_then(Value::as_str).unwrap_or("auto") {
        "straight" => Vec::new(),
        "vertical-channel" => {
            let x = flow
                .get("channelX")
                .and_then(Value::as_f64)
                .unwrap_or(start.0 + if end.0 > start.0 { 44.0 } else { -44.0 });
            vec![(x, start.1), (x, end.1)]
        }
        "bottom-channel" => {
            let y = flow.get("channelY").and_then(Value::as_f64).unwrap_or((from.y + from.height).max(to.y + to.height) + 26.0);
            vec![(start.0, y), (end.0, y)]
        }
        "top-channel" => {
            let y = flow.get("channelY").and_then(Value::as_f64).unwrap_or(from.y.min(to.y) - 24.0);
            vec![(start.0, y), (end.0, y)]
        }
        _ => {
            if (start.1 - end.1).abs() < 4.0 {
                return Vec::new();
            }
            let mid_x = start.0 + (end.0 - start.0) / 2.0;
            vec![(mid_x, start.1), (mid_x, end.1)]
        }
    }
}

fn path_for(flow: &Value, nodes: &HashMap<String, MeasuredNode>, automatic_ports: &HashMap<usize, (Option<Pt>, Option<Pt>)>, index: usize) -> Vec<Pt> {
    let from_id = flow.get("from").and_then(Value::as_str).unwrap_or("");
    let to_id = flow.get("to").and_then(Value::as_str).unwrap_or("");
    let (Some((from_rect, ..)), Some((to_rect, ..))) = (nodes.get(from_id), nodes.get(to_id)) else {
        return Vec::new();
    };
    let from_side = chosen_side(flow.get("fromSide").and_then(Value::as_str), default_from_side(from_rect, to_rect));
    let to_side = chosen_side(flow.get("toSide").and_then(Value::as_str), default_to_side(from_rect, to_rect));
    let ports = automatic_ports.get(&index);
    let start = ports.and_then(|p| p.0).unwrap_or_else(|| anchor(from_rect, from_side));
    let end = ports.and_then(|p| p.1).unwrap_or_else(|| anchor(to_rect, to_side));
    let mut points = vec![start];
    points.extend(route_via(flow, from_rect, to_rect, start, end));
    points.push(end);
    points
}

fn render_stage(stage: &Value, index: usize, view_box: Pt) -> String {
    let cx = stage_x(index);
    let frame_height = view_box.1 - LAYOUT.stage_y - LAYOUT.stage_bottom_pad;
    format!(
        "        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"stage\" data-composition-frame-id=\"{index}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\" class=\"c-lane\" stroke-width=\"1\"/>\n        <text x=\"{}\" y=\"{}\" class=\"t-dim\" font-size=\"9\" font-weight=\"600\" text-anchor=\"middle\">{:02} / {}</text>",
        num(cx - LAYOUT.stage_w / 2.0),
        num(LAYOUT.stage_y),
        num(LAYOUT.stage_w),
        num(frame_height),
        num(cx),
        num(LAYOUT.stage_y + 22.0),
        index + 1,
        esc(stage.get("label").and_then(Value::as_str).unwrap_or(""))
    )
}

fn render_flow_path(dataflow: &Value, flow: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let (cls, marker) = arrow_class(flow.get("variant").and_then(Value::as_str).unwrap_or("default"));
    let points = routed.get(&index).cloned().unwrap_or_default();
    let stroke_width = flow.get("width").and_then(Value::as_f64).unwrap_or(if flow.get("variant").and_then(Value::as_str) == Some("emphasis") { 1.8 } else { 1.4 });
    let label = flow.get("label").and_then(Value::as_str);
    let id = flow.get("id").and_then(Value::as_str);
    let from = flow.get("from").and_then(Value::as_str).unwrap_or("");
    let to = flow.get("to").and_then(Value::as_str).unwrap_or("");
    format!(
        "        <path {} data-composition-points=\"{}\" d=\"{}\" class=\"{}\"{} stroke-width=\"{}\" marker-end=\"url(#{})\"/>",
        focus_edge_attrs(from, to, label, Some(index), id),
        route_points_value(&points),
        polyline_path(&points),
        cls,
        animate_attr(dataflow.get("meta").unwrap_or(&Value::Null), "edge", Some(index as f64)),
        num(stroke_width),
        marker
    )
}

fn render_flow_label(flow: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let Some(label) = flow.get("label").and_then(Value::as_str) else {
        return String::new();
    };
    let points = routed.get(&index).cloned().unwrap_or_default();
    let label_at = flow.get("labelAt").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_f64).collect());
    let (lx, ly) = label_point(
        label_at.as_ref(),
        flow.get("labelDx").and_then(Value::as_f64),
        flow.get("labelDy").and_then(Value::as_f64),
        flow.get("labelSegment").and_then(Value::as_u64).map(|v| v as usize),
        &points,
    );
    let (label_w, label_h) = flow_label_size(flow);
    let classification = flow
        .get("classification")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(|c| {
            format!(
                "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"t-dim\" font-size=\"7\" text-anchor=\"middle\">{}</text>",
                num(lx),
                num(ly + 11.0),
                esc(c)
            )
        })
        .unwrap_or_default();
    let from = flow.get("from").and_then(Value::as_str).unwrap_or("");
    let to = flow.get("to").and_then(Value::as_str).unwrap_or("");
    let id = flow.get("id").and_then(Value::as_str);
    format!(
        "        <g data-detail=\"context\" {}>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"8\" text-anchor=\"middle\">{}</text>{}\n        </g>",
        focus_edge_attrs(from, to, Some(label), Some(index), id),
        num(lx - label_w / 2.0),
        num(ly - 11.0),
        num(label_w),
        num(label_h),
        num(lx),
        num(ly),
        variant_accent(flow.get("variant").and_then(Value::as_str), "t-messagebus"),
        esc(label),
        classification
    )
}

fn render_node(
    dataflow: &Value,
    id: &str,
    node: &MeasuredNode,
    stages: &[&Value],
    node_steps: &HashMap<String, f64>,
) -> String {
    let (rect, kind, label, sublabel, tag, stage_index, _) = node;
    let fill = component_fill(kind);
    let accent = component_text(kind);
    let has_sub = sublabel.as_ref().is_some_and(|s| !s.is_empty());
    let sub = if has_sub {
        let font = text_fit::fitted_font_size(sublabel.as_deref().unwrap(), rect.width, NODE_TEXT_FIT.0, NODE_TEXT_FIT.1);
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
        let font = text_fit::fitted_font_size(tag, rect.width, NODE_TEXT_FIT.2, NODE_TEXT_FIT.3);
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
    let context = stages
        .get(*stage_index)
        .map(|stage| format!("{:02} / {}", stage_index + 1, stage.get("label").and_then(Value::as_str).unwrap_or("")))
        .unwrap_or_else(|| "Data-flow node".to_string());
    let step = node_steps.get(id).copied();
    let step_attr = animate_attr(dataflow.get("meta").unwrap_or(&Value::Null), "node", step);
    let passport_sublabel = sublabel.as_deref();
    let passport_tag = tag.as_deref();
    format!(
        "        <g {}>\n          {}\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"c-mask\"/>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"{}\"{} stroke-width=\"1.5\"/>\n          {}\n          <text{} x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"10\" font-weight=\"600\" text-anchor=\"middle\">{}</text>{}{}\n        </g>",
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
        tag_html
    )
}

fn render_legend_block(
    dataflow: &Value,
    entries: &[super::legend::LegendEntry],
    nodes: &HashMap<String, MeasuredNode>,
    flows: &[&Value],
    view_box: Pt,
) -> String {
    let has_database = nodes.values().any(|(_, kind, ..)| kind == "database");
    let catalog: Vec<(String, String, &str, &str, f64)> = LEGEND_CATALOG
        .iter()
        .map(|(kind, label)| {
            let (class_name, marker, stroke_width) = match *kind {
                "emphasis" => ("a-emphasis", "arrowhead-emphasis", 1.8),
                "security" => ("a-security", "arrowhead-security", 1.4),
                "dashed" => ("a-dashed", "arrowhead-dashed", 1.4),
                "default" => ("a-default", "arrowhead", 1.4),
                _ => ("", "", 1.4),
            };
            (kind.to_string(), label.to_string(), class_name, marker, stroke_width)
        })
        .collect();
    let mut layout = legend_layout(
        40.0,
        view_box.1 - 36.0,
        view_box.0 - 80.0,
        view_box.1 - 66.0,
        if dataflow.get("meta").and_then(|m| m.get("legend")).is_none() { "hide" } else { "error" },
        "dataflow",
    );
    let _ = flows;
    let mut with_swatches: Vec<super::legend::LegendEntry> = entries.to_vec();
    for entry in &mut with_swatches {
        if entry.kind == "database" {
            entry.swatch_width = None;
            entry.swatch_gap = None;
        } else {
            entry.swatch_width = Some(34.0);
            entry.swatch_gap = Some(9.0);
        }
        entry.interactive = false;
    }
    let swatch = |entry: &super::legend::LegendEntry| {
        if entry.kind == "database" {
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"14\" height=\"9\" rx=\"2\" class=\"c-database\" stroke-width=\"1\"/>",
                num(entry.x),
                num(entry.baseline - 8.0)
            )
        } else {
            let item = catalog.iter().find(|(kind, ..)| *kind == entry.kind).unwrap();
            format!(
                "<path d=\"M {} {} L {} {}\" class=\"{}\" stroke-width=\"{}\" marker-end=\"url(#{})\"/>",
                num(entry.x),
                num(entry.baseline - 3.0),
                num(entry.x + 34.0),
                num(entry.baseline - 3.0),
                item.2,
                num(item.4),
                item.3
            )
        }
    };
    let _ = has_database;
    render_legend(&with_swatches, &layout, &swatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_official_example() {
        let text = include_str!("../../examples/event-stream.dataflow.json");
        let dataflow: Value = serde_json::from_str(text).unwrap();
        let svg = render_svg(&dataflow).unwrap();
        assert!(svg.contains("<!-- Data Stages -->"));
        assert!(svg.contains("data-composition-frame-kind=\"stage\""));
        assert!(svg.contains("data-node-id="));
    }
}
