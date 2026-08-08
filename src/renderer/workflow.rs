//! Workflow diagram renderer — faithful port of the original
//! `renderers/workflow/render-workflow.mjs`.

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

const LAYOUT: WorkflowLayout = WorkflowLayout {
    lane_x: 40.0,
    lane_y: 52.0,
    lane_w: 640.0,
    lane_h: 104.0,
    lane_gap: 20.0,
    lane_title_h: 30.0,
    col_xs: [88.0, 220.0, 300.0, 430.0, 500.0, 625.0],
    node_w: 92.0,
    node_h: 52.0,
};

struct WorkflowLayout {
    lane_x: f64,
    lane_y: f64,
    lane_w: f64,
    lane_h: f64,
    lane_gap: f64,
    lane_title_h: f64,
    col_xs: [f64; 6],
    node_w: f64,
    node_h: f64,
}

const NODE_TEXT_FIT: (f64, f64, f64, f64, f64, f64) = (11.0, 9.0, 8.0, 6.0, 7.0, 6.0); // labelPreferred, labelMinimum, sublabelPreferred, sublabelMinimum, tagPreferred, tagMinimum

const LEGEND_CATALOG: [(&str, &str); 7] = [
    ("frontend", "User UI"),
    ("backend", "Agent logic"),
    ("security", "Policy"),
    ("messagebus", "Tool action"),
    ("database", "Context / trace"),
    ("cloud", "Cloud service"),
    ("external", "External system"),
];

type MeasuredNode = (Rect, String, String, Option<String>, Option<String>, String, usize); // rect, kind, label, sublabel, tag, lane, col

pub fn render_svg(workflow: &Value) -> Result<String> {
    let lanes: Vec<&Value> = arr(workflow.get("lanes")).iter().collect();
    let nodes_raw: Vec<&Value> = arr(workflow.get("nodes")).iter().collect();
    let edges_raw: Vec<&Value> = arr(workflow.get("edges")).iter().collect();
    let phases: Vec<&Value> = arr(workflow.get("phases")).iter().collect();
    let groups: Vec<&Value> = arr(workflow.get("groups")).iter().collect();

    let lane_count = lanes.len().max(1);
    let auto_height = LAYOUT.lane_y
        + lane_count as f64 * LAYOUT.lane_h
        + (lane_count as f64 - 1.0) * LAYOUT.lane_gap
        + 124.0;
    let view_box = workflow
        .get("meta")
        .and_then(|m| m.get("viewBox"))
        .and_then(Value::as_array)
        .map(|p| (p[0].as_f64().unwrap_or(720.0), p[1].as_f64().unwrap_or(auto_height)))
        .unwrap_or((720.0, auto_height));

    let lane_index: HashMap<String, usize> = lanes.iter().enumerate().map(|(i, lane)| (lane.get("id").and_then(Value::as_str).unwrap_or("").to_string(), i)).collect();
    let lane_labels: HashMap<String, String> = lanes.iter().filter_map(|lane| Some((lane.get("id")?.as_str()?.to_string(), lane.get("label")?.as_str()?.to_string()))).collect();

    let nodes = measure_nodes(&nodes_raw, &lane_index);
    let main_path: Vec<&str> = workflow.get("mainPath").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let main_path_steps: HashMap<&str, f64> = main_path.iter().enumerate().map(|(i, id)| (*id, i as f64)).collect();
    let edge_steps: HashMap<usize, f64> = edges_raw
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let from_step = edge.get("from").and_then(Value::as_str).and_then(|id| main_path_steps.get(id));
            let to_step = edge.get("to").and_then(Value::as_str).and_then(|id| main_path_steps.get(id));
            let main_step = match (from_step, to_step) {
                (Some(from), Some(to)) if *to == from + 1.0 => Some(*from),
                _ => None,
            };
            (index, main_step.unwrap_or(main_path.len() as f64 + index as f64))
        })
        .collect();

    let node_boxes: HashMap<String, Rect> = nodes.iter().map(|(id, (rect, ..))| (id.clone(), rect.clone())).collect();
    let automatic_ports = automatic_port_spread(&edges_raw, &node_boxes);
    let routed: HashMap<usize, Vec<Pt>> = edges_raw
        .iter()
        .enumerate()
        .map(|(index, edge)| (index, path_for(edge, &nodes, &automatic_ports, index, &lane_index)))
        .collect();

    let present_kinds: Vec<String> = nodes.values().map(|(_, kind, ..)| kind.clone()).collect();
    let legend_entries = resolve_legend(
        workflow.get("meta").and_then(|m| m.get("legend")),
        &LEGEND_CATALOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect::<Vec<_>>(),
        &present_kinds,
    );

    let legend_y = last_lane_bottom(&lanes, &lane_index) + 44.0;

    let svg = format!(
        "      <svg viewBox=\"0 0 {} {}\" {}>
{}
{}

        <!-- Background Grid -->
        <rect width=\"100%\" height=\"100%\" fill=\"url(#grid)\" />

        <!-- Swimlanes -->
{}

        <!-- Phase headers -->
{}

        <!-- Workflow groups -->
{}

        <!-- Edge paths -->
{}

        <!-- Nodes -->
{}

        <!-- Edge labels -->
{}

        <!-- Legend -->
{}
      </svg>",
        num(view_box.0),
        num(view_box.1),
        svg_root_attrs(workflow.get("meta").unwrap_or(&Value::Null), "workflow diagram"),
        svg_accessible_text(workflow.get("meta").unwrap_or(&Value::Null), "workflow diagram"),
        render_definitions(),
        lanes.iter().enumerate().map(|(index, lane)| render_lane(lane, index)).collect::<Vec<_>>().join("\n\n"),
        phases.iter().map(|phase| render_phase(phase)).collect::<Vec<_>>().join("\n"),
        groups.iter().enumerate().map(|(index, group)| render_group(group, index, &lane_index)).collect::<Vec<_>>().join("\n"),
        edges_raw
            .iter()
            .enumerate()
            .map(|(index, edge)| render_edge_path(workflow, edge, index, &routed, &edge_steps))
            .collect::<Vec<_>>()
            .join("\n"),
        nodes
            .iter()
            .map(|(id, node)| render_node(workflow, id, node, &lane_labels, &groups, &phases, &main_path_steps, &nodes_raw))
            .collect::<Vec<_>>()
            .join("\n\n"),
        edges_raw
            .iter()
            .enumerate()
            .map(|(index, edge)| render_edge_label(edge, index, &routed))
            .collect::<Vec<_>>()
            .join("\n"),
        render_legend_block(workflow, &legend_entries, &nodes, legend_y, view_box),
    );
    Ok(svg)
}

fn lane_top(lane_index: &HashMap<String, usize>, id: &str) -> f64 {
    let index = lane_index.get(id).copied().unwrap_or(0);
    LAYOUT.lane_y + index as f64 * (LAYOUT.lane_h + LAYOUT.lane_gap)
}

fn last_lane_bottom(lanes: &[&Value], lane_index: &HashMap<String, usize>) -> f64 {
    let count = lanes.len().max(1);
    let _ = lane_index;
    LAYOUT.lane_y + count as f64 * LAYOUT.lane_h + (count as f64 - 1.0) * LAYOUT.lane_gap
}

fn measure_nodes(nodes_raw: &[&Value], lane_index: &HashMap<String, usize>) -> HashMap<String, MeasuredNode> {
    let mut nodes = HashMap::new();
    for node in nodes_raw {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };
        let lane = node.get("lane").and_then(Value::as_str).unwrap_or("");
        let width = node.get("width").and_then(Value::as_f64).unwrap_or(LAYOUT.node_w);
        let height = node.get("height").and_then(Value::as_f64).unwrap_or(if node.get("tag").is_some() { 68.0 } else { LAYOUT.node_h });
        let col = node.get("col").and_then(Value::as_u64).unwrap_or(0) as usize;
        let cx = LAYOUT.col_xs[col.min(5)];
        let content_h = LAYOUT.lane_h - LAYOUT.lane_title_h;
        let y = lane_top(lane_index, lane) + LAYOUT.lane_title_h + (content_h - height) / 2.0 + node.get("yOffset").and_then(Value::as_f64).unwrap_or(0.0);
        nodes.insert(
            id.to_string(),
            (
                Rect::new(id, cx - width / 2.0, y, width, height),
                node.get("type").and_then(Value::as_str).unwrap_or("external").to_string(),
                node.get("label").and_then(Value::as_str).unwrap_or(id).to_string(),
                node.get("sublabel").and_then(Value::as_str).map(String::from),
                node.get("tag").and_then(Value::as_str).map(String::from),
                lane.to_string(),
                col,
            ),
        );
    }
    nodes
}

fn node_context(
    node: &MeasuredNode,
    lane_labels: &HashMap<String, String>,
    groups: &[&Value],
    phases: &[&Value],
) -> String {
    let (_, _, _, _, _, lane, col) = node;
    let group_label = groups
        .iter()
        .filter(|group| {
            group.get("lane").and_then(Value::as_str) == Some(lane.as_str())
                && *col >= group.get("fromCol").and_then(Value::as_u64).unwrap_or(0) as usize
                && *col <= group.get("toCol").and_then(Value::as_u64).unwrap_or(0) as usize
        })
        .map(|group| group.get("label").and_then(Value::as_str).unwrap_or("").to_string())
        .next();
    let phase_label = phases
        .iter()
        .filter(|phase| {
            *col >= phase.get("fromCol").and_then(Value::as_u64).unwrap_or(0) as usize
                && *col <= phase.get("toCol").and_then(Value::as_u64).unwrap_or(0) as usize
        })
        .map(|phase| phase.get("label").and_then(Value::as_str).unwrap_or("").to_string())
        .next();
    let lane_label = lane_labels.get(lane).cloned();
    let parts: Vec<String> = [lane_label, group_label, phase_label].into_iter().flatten().collect();
    if parts.is_empty() {
        "Workflow node".to_string()
    } else {
        parts.join(" › ")
    }
}

fn render_lane(lane: &Value, index: usize) -> String {
    let y = LAYOUT.lane_y + index as f64 * (LAYOUT.lane_h + LAYOUT.lane_gap);
    let exception = if lane.get("variant").and_then(Value::as_str) == Some("exception") {
        format!(
            "\n        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"exception-lane\" data-composition-frame-id=\"lane-{index}-exception\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" class=\"c-security-group\" stroke-width=\"1\"/>",
            num(LAYOUT.lane_x + 6.0),
            num(y + 6.0),
            num(LAYOUT.lane_w - 12.0),
            num(LAYOUT.lane_h - 12.0)
        )
    } else {
        String::new()
    };
    let label_class = if lane.get("variant").and_then(Value::as_str) == Some("exception") { "t-security" } else { "t-dim" };
    let prefix = if lane.get("variant").and_then(Value::as_str) == Some("exception") {
        "EX".to_string()
    } else {
        format!("{:02}", index + 1)
    };
    format!(
        "        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"lane\" data-composition-frame-id=\"lane-{index}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\" class=\"c-lane\" stroke-width=\"1\"/>{exception}\n        <text x=\"{}\" y=\"{}\" class=\"{label_class}\" font-size=\"10\" font-weight=\"600\">{prefix} / {}</text>",
        num(LAYOUT.lane_x),
        num(y),
        num(LAYOUT.lane_w),
        num(LAYOUT.lane_h),
        num(LAYOUT.lane_x + 14.0),
        num(y + 22.0),
        esc(lane.get("label").and_then(Value::as_str).unwrap_or(""))
    )
}

fn span_for_cols(from_col: usize, to_col: usize, pad: f64) -> (f64, f64, f64) {
    let start = LAYOUT.col_xs[from_col.min(5)] - pad;
    let end = LAYOUT.col_xs[to_col.min(5)] + pad;
    (start, end - start, (start + end) / 2.0)
}

fn render_phase(phase: &Value) -> String {
    let span = span_for_cols(
        phase.get("fromCol").and_then(Value::as_u64).unwrap_or(0) as usize,
        phase.get("toCol").and_then(Value::as_u64).unwrap_or(0) as usize,
        46.0,
    );
    let accent = variant_accent(phase.get("variant").and_then(Value::as_str), "t-messagebus");
    let (line_class, _) = arrow_class(phase.get("variant").and_then(Value::as_str).unwrap_or("default"));
    format!(
        "        <line x1=\"{}\" y1=\"35\" x2=\"{}\" y2=\"35\" class=\"{}\" stroke-width=\"1.1\"/>\n        <rect x=\"{}\" y=\"27\" width=\"{}\" height=\"16\" rx=\"4\" class=\"c-mask\"/>\n        <text x=\"{}\" y=\"39\" class=\"{}\" font-size=\"8\" font-weight=\"600\" text-anchor=\"middle\">{}</text>",
        num(span.0),
        num(span.0 + span.1),
        line_class,
        num(span.0),
        num(span.1),
        num(span.2),
        accent,
        esc(phase.get("label").and_then(Value::as_str).unwrap_or(""))
    )
}

fn render_group(group: &Value, index: usize, lane_index: &HashMap<String, usize>) -> String {
    let span = span_for_cols(
        group.get("fromCol").and_then(Value::as_u64).unwrap_or(0) as usize,
        group.get("toCol").and_then(Value::as_u64).unwrap_or(0) as usize,
        50.0,
    );
    let y = lane_top(lane_index, group.get("lane").and_then(Value::as_str).unwrap_or("")) + LAYOUT.lane_title_h + 8.0;
    let height = LAYOUT.lane_h - LAYOUT.lane_title_h - 16.0;
    let cls = if group.get("variant").and_then(Value::as_str) == Some("security") { "c-security-group" } else { "c-lane" };
    let text_class = variant_accent(group.get("variant").and_then(Value::as_str), "t-messagebus");
    format!(
        "        <rect data-graph-role=\"structural-frame\" data-composition-frame-kind=\"group\" data-composition-frame-id=\"group-{index}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"9\" class=\"{}\" stroke-width=\"1\"/>\n        <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"7\" font-weight=\"600\">{}</text>",
        num(span.0),
        num(y),
        num(span.1),
        num(height),
        cls,
        num(span.0 + 10.0),
        num(y + 14.0),
        text_class,
        esc(group.get("label").and_then(Value::as_str).unwrap_or(""))
    )
}

fn gap_y_between(from_lane: &str, to_lane: &str, lane_index: &HashMap<String, usize>, bias: f64) -> f64 {
    let a = lane_top(lane_index, from_lane) + LAYOUT.lane_h;
    let b = lane_top(lane_index, to_lane);
    a + (b - a) * bias
}

fn same_lane_auto_via(start: Pt, end: Pt) -> Vec<Pt> {
    if start.0 == end.0 || start.1 == end.1 {
        return Vec::new();
    }
    let mid_x = (start.0 + end.0) / 2.0;
    vec![(mid_x, start.1), (mid_x, end.1)]
}

fn route_via(edge: &Value, from_lane: &str, to_lane: &str, from: &Rect, to: &Rect, start: Pt, end: Pt, lane_index: &HashMap<String, usize>) -> Vec<Pt> {
    if let Some(via) = edge.get("via").and_then(Value::as_array) {
        return via.iter().filter_map(|p| p.as_array()).filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect();
    }
    match edge.get("route").and_then(Value::as_str).unwrap_or("auto") {
        "straight" => Vec::new(),
        "drop" => {
            let y = gap_y_between(from_lane, to_lane, lane_index, edge.get("bias").and_then(Value::as_f64).unwrap_or(0.5));
            vec![(start.0, y), (end.0, y)]
        }
        "outside-right" => {
            let x = edge.get("channelX").and_then(Value::as_f64).unwrap_or(LAYOUT.lane_x + LAYOUT.lane_w + 12.0);
            vec![(x, start.1), (x, end.1)]
        }
        "return-left" => {
            let x = edge.get("channelX").and_then(Value::as_f64).unwrap_or(from.x.min(to.x) - 28.0);
            vec![(x, start.1), (x, end.1)]
        }
        "bottom-channel" => {
            let y = edge.get("channelY").and_then(Value::as_f64).unwrap_or((from.y + from.height).max(to.y + to.height) + 32.0);
            vec![(start.0, y), (end.0, y)]
        }
        "up-channel" => {
            let y = edge.get("channelY").and_then(Value::as_f64).unwrap_or(from.y.min(to.y) - 28.0);
            vec![(start.0, y), (end.0, y)]
        }
        _ => {
            if from_lane == to_lane {
                return same_lane_auto_via(start, end);
            }
            let y = gap_y_between(from_lane, to_lane, lane_index, edge.get("bias").and_then(Value::as_f64).unwrap_or(0.5));
            vec![(start.0, y), (end.0, y)]
        }
    }
}

fn path_for(edge: &Value, nodes: &HashMap<String, MeasuredNode>, automatic_ports: &HashMap<usize, (Option<Pt>, Option<Pt>)>, index: usize, lane_index: &HashMap<String, usize>) -> Vec<Pt> {
    let from_id = edge.get("from").and_then(Value::as_str).unwrap_or("");
    let to_id = edge.get("to").and_then(Value::as_str).unwrap_or("");
    let (Some((from_rect, _, _, _, _, from_lane, _)), Some((to_rect, _, _, _, _, to_lane, _))) = (nodes.get(from_id), nodes.get(to_id)) else {
        return Vec::new();
    };
    let from_side = chosen_side(edge.get("fromSide").and_then(Value::as_str), default_from_side(from_rect, to_rect));
    let to_side = chosen_side(edge.get("toSide").and_then(Value::as_str), default_to_side(from_rect, to_rect));
    let ports = automatic_ports.get(&index);
    let start = ports.and_then(|p| p.0).unwrap_or_else(|| anchor(from_rect, from_side));
    let end = ports.and_then(|p| p.1).unwrap_or_else(|| anchor(to_rect, to_side));
    let mut points = vec![start];
    points.extend(route_via(edge, from_lane, to_lane, from_rect, to_rect, start, end, lane_index));
    points.push(end);
    points
}

fn render_edge_path(
    workflow: &Value,
    edge: &Value,
    index: usize,
    routed: &HashMap<usize, Vec<Pt>>,
    edge_steps: &HashMap<usize, f64>,
) -> String {
    let (cls, marker) = arrow_class(edge.get("variant").and_then(Value::as_str).unwrap_or("default"));
    let points = routed.get(&index).cloned().unwrap_or_default();
    let stroke_width = edge.get("width").and_then(Value::as_f64).unwrap_or(if edge.get("variant").and_then(Value::as_str) == Some("emphasis") { 1.8 } else { 1.4 });
    let label = edge.get("label").and_then(Value::as_str);
    let id = edge.get("id").and_then(Value::as_str);
    let from = edge.get("from").and_then(Value::as_str).unwrap_or("");
    let to = edge.get("to").and_then(Value::as_str).unwrap_or("");
    format!(
        "        <path {} data-composition-points=\"{}\" d=\"{}\" class=\"{}\"{} stroke-width=\"{}\" marker-end=\"url(#{})\"/>",
        focus_edge_attrs(from, to, label, Some(index), id),
        route_points_value(&points),
        polyline_path(&points),
        cls,
        animate_attr(workflow.get("meta").unwrap_or(&Value::Null), "edge", edge_steps.get(&index).copied()),
        num(stroke_width),
        marker
    )
}

fn render_edge_label(edge: &Value, index: usize, routed: &HashMap<usize, Vec<Pt>>) -> String {
    let Some(label) = edge.get("label").and_then(Value::as_str) else {
        return String::new();
    };
    let points = routed.get(&index).cloned().unwrap_or_default();
    let label_at = edge.get("labelAt").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_f64).collect());
    let (lx, ly) = label_point(
        label_at.as_ref(),
        edge.get("labelDx").and_then(Value::as_f64),
        edge.get("labelDy").and_then(Value::as_f64),
        edge.get("labelSegment").and_then(Value::as_u64).map(|v| v as usize),
        &points,
    );
    let label_w = (text_units(label) as f64 * 4.8 + 10.0).max(30.0);
    let from = edge.get("from").and_then(Value::as_str).unwrap_or("");
    let to = edge.get("to").and_then(Value::as_str).unwrap_or("");
    let id = edge.get("id").and_then(Value::as_str);
    format!(
        "        <g data-detail=\"context\" {}>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"14\" rx=\"3\" class=\"c-mask\"/>\n          <text x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"8\" text-anchor=\"middle\">{}</text>\n        </g>",
        focus_edge_attrs(from, to, Some(label), Some(index), id),
        num(lx - label_w / 2.0),
        num(ly - 10.0),
        num(label_w),
        num(lx),
        num(ly),
        variant_accent(edge.get("variant").and_then(Value::as_str), "t-database"),
        esc(label)
    )
}

fn render_node(
    workflow: &Value,
    id: &str,
    node: &MeasuredNode,
    lane_labels: &HashMap<String, String>,
    groups: &[&Value],
    phases: &[&Value],
    main_path_steps: &HashMap<&str, f64>,
    nodes_raw: &[&Value],
) -> String {
    let (rect, kind, label, sublabel, tag, _, _) = node;
    let fill = component_fill(kind);
    let accent = component_text(kind);
    let has_sub = sublabel.as_ref().is_some_and(|s| !s.is_empty());
    let label_font = text_fit::fitted_font_size(label, rect.width, NODE_TEXT_FIT.0, NODE_TEXT_FIT.1);
    let sub = if has_sub {
        let font = text_fit::fitted_font_size(sublabel.as_deref().unwrap(), rect.width, NODE_TEXT_FIT.2, NODE_TEXT_FIT.3);
        format!(
            "\n          <text data-detail=\"context\" x=\"{}\" y=\"{}\" class=\"t-muted\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(rect.cx),
            num(rect.y + 38.0),
            num(font),
            esc(sublabel.as_deref().unwrap())
        )
    } else {
        String::new()
    };
    let tag_html = if let Some(tag) = tag.as_deref().filter(|t| !t.is_empty()) {
        let font = text_fit::fitted_font_size(tag, rect.width, NODE_TEXT_FIT.4, NODE_TEXT_FIT.5);
        format!(
            "\n        <text data-detail=\"fine\" x=\"{}\" y=\"{}\" class=\"{}\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
            num(rect.cx),
            num(rect.y + rect.height - 12.0),
            accent,
            num(font),
            esc(tag)
        )
    } else {
        String::new()
    };
    let context = node_context(node, lane_labels, groups, phases);
    let node_step = main_path_steps.get(id).copied().or_else(|| {
        nodes_raw
            .iter()
            .position(|item| item.get("id").and_then(Value::as_str) == Some(id))
            .map(|index| main_path_steps.len() as f64 + index as f64)
    });
    let step_attr = animate_attr(workflow.get("meta").unwrap_or(&Value::Null), "node", node_step);
    let passport_sublabel = sublabel.as_deref();
    let passport_tag = tag.as_deref();
    format!(
        "        <g {}>\n          {}\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"c-mask\"/>\n          <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"{}\"{} stroke-width=\"1.5\"/>\n          {}\n          <text{} x=\"{}\" y=\"{}\" class=\"t-primary\" font-size=\"{}\" font-weight=\"600\" text-anchor=\"middle\">{}</text>{}{}\n        </g>",
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
        num(label_font),
        esc(label),
        sub,
        tag_html
    )
}

fn render_legend_block(
    workflow: &Value,
    entries: &[super::legend::LegendEntry],
    nodes: &HashMap<String, MeasuredNode>,
    legend_y: f64,
    view_box: Pt,
) -> String {
    let mut layout = legend_layout(
        20.0,
        legend_y,
        view_box.0 - 40.0,
        legend_y - 36.0,
        if workflow.get("meta").and_then(|m| m.get("legend")).is_none() { "hide" } else { "error" },
        "workflow",
    );
    layout.font_size = 7.0;
    layout.item_gap = 7.0;
    let _ = nodes;
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
        let text = include_str!("../../examples/agent-tool-call.workflow.json");
        let workflow: Value = serde_json::from_str(text).unwrap();
        let svg = render_svg(&workflow).unwrap();
        assert!(svg.contains("data-composition-frame-kind=\"lane\""));
        assert!(svg.contains("data-node-id=\"user\""));
        assert!(svg.contains("01 / User Interface"));
        assert!(svg.contains("<!-- Phase headers -->"));
    }
}
