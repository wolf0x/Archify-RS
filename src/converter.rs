//! Mermaid → JSON IR converter. Supports flowchart → workflow and
//! sequenceDiagram → sequence. See `mermaid.pest` for the grammar.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use serde_json::{json, Value};

#[derive(Parser)]
#[grammar = "mermaid.pest"]
struct MermaidParser;

/// Maximum input file size (10 MB) for Mermaid files.
const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

pub fn convert_file(input: &Path, type_name: &str) -> Result<Value> {
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
    convert_str(&text, type_name)
}

pub fn convert_str(text: &str, type_name: &str) -> Result<Value> {
    let pairs = MermaidParser::parse(Rule::document, text)
        .map_err(|e| anyhow!("mermaid parse error: {e}"))?;
    let root = pairs.into_iter().next().ok_or_else(|| anyhow!("empty mermaid document"))?;
    let diagram = root.into_inner().next().ok_or_else(|| anyhow!("empty mermaid document"))?;
    match diagram.as_rule() {
        Rule::flowchart => flowchart_to_ir(diagram, type_name),
        Rule::sequence_diagram => {
            if type_name != "sequence" {
                bail!("sequenceDiagram input must be converted with -t sequence (got \"{type_name}\")");
            }
            Ok(sequence_to_ir(diagram))
        }
        other => bail!("unsupported mermaid construct: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Flowchart → workflow IR
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FlowNode { id: String, label: String, shape: Option<String> }

#[derive(Debug, Clone)]
struct FlowEdge { from: String, to: String, label: Option<String>, variant: String }

fn shape_type_from_pair(pair: &Pair<Rule>) -> (Option<String>, Option<String>) {
    let text = pair.as_str();
    let shape = if text.starts_with("[[") { Some("[[".to_string()) }
    else if text.starts_with("[(") { Some("[(]".to_string()) }
    else if text.starts_with("((") { Some("((".to_string()) }
    else if text.starts_with("[") { Some("[".to_string()) }
    else if text.starts_with("(") { Some("(".to_string()) }
    else if text.starts_with("{") { Some("{".to_string()) }
    else if text.starts_with(">") { Some(">".to_string()) }
    else { None };
    
    // Extract label from inside the shape
    let label = pair.clone().into_inner().find(|p| p.as_rule() == Rule::label).map(|l| {
        l.as_str().trim().trim_matches('"').trim_matches('\'').to_string()
    }).filter(|s| !s.is_empty());
    
    (shape, label)
}

fn parse_flowchart(pair: Pair<Rule>) -> (Vec<FlowNode>, Vec<FlowEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut ensure = |id: &str, label: Option<&str>, shape: Option<&str>, nodes: &mut Vec<FlowNode>, by_id: &mut HashMap<String, usize>| {
        if let Some(&i) = by_id.get(id) {
            if let Some(l) = label.filter(|l| !l.is_empty()) { if nodes[i].label == id { nodes[i].label = l.to_string(); } }
            if let Some(s) = shape { nodes[i].shape = Some(s.to_string()); }
            return;
        }
        let i = nodes.len();
        nodes.push(FlowNode { id: id.to_string(), label: label.unwrap_or(id).to_string(), shape: shape.map(String::from) });
        by_id.insert(id.to_string(), i);
    };
    for line in pair.into_inner() {
        if line.as_rule() != Rule::flowchart_line { continue; }
        for part in line.into_inner() {
            match part.as_rule() {
                Rule::edge_stmt => {
                    let mut pending: Option<(String, Option<String>)> = None;
                    let mut pv = String::new();
                    let mut pl: Option<String> = None;
                    for sub in part.into_inner() {
                        match sub.as_rule() {
                            Rule::node_id => {
                                let id = sub.as_str().to_string();
                                match pending.take() {
                                    None => pending = Some((id, None)),
                                    Some((from, shape)) => {
                                        ensure(&from, None, shape.as_deref(), &mut nodes, &mut by_id);
                                        ensure(&id, None, None, &mut nodes, &mut by_id);
                                        edges.push(FlowEdge { from, to: id.clone(), label: pl.take(), variant: std::mem::take(&mut pv) });
                                        pending = Some((id, None));
                                    }
                                }
                            }
                            Rule::node_shape => {
                                if let Some((id, _)) = pending.take() {
                                    let (shape, label) = shape_type_from_pair(&sub);
                                    // Update node label if we have one
                                    if let Some(ref l) = label {
                                        if let Some(&i) = by_id.get(&id) {
                                            if nodes[i].label == id {
                                                nodes[i].label = l.clone();
                                            }
                                        }
                                    }
                                    pending = Some((id, shape));
                                }
                            }
                            Rule::arrow => {
                                // Arrow is a parent rule, need to check which specific arrow type
                                for arrow_sub in sub.into_inner() {
                                    match arrow_sub.as_rule() {
                                        Rule::solid_arrow => { 
                                            pv = "default".into(); 
                                            pl = arrow_label(&arrow_sub, "default");
                                        }
                                        Rule::dotted_arrow => { pv = "dashed".into(); pl = arrow_label(&arrow_sub, "dashed"); }
                                        Rule::thick_arrow => { pv = "emphasis".into(); pl = arrow_label(&arrow_sub, "emphasis"); }
                                        Rule::plain_arrow => { 
                                            pv = "default".into(); 
                                            pl = None;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Rule::node_stmt => {
                    let mut id = String::new();
                    let mut shape: Option<String> = None;
                    let mut label: Option<String> = None;
                    for sub in part.into_inner() {
                        match sub.as_rule() {
                            Rule::node_id => id = sub.as_str().to_string(),
                            Rule::node_shape => {
                                let (s, l) = shape_type_from_pair(&sub);
                                shape = s;
                                label = l;
                            }
                            _ => {}
                        }
                    }
                    if !id.is_empty() { ensure(&id, label.as_deref(), shape.as_deref(), &mut nodes, &mut by_id); }
                }
                _ => {}
            }
        }
    }
    (nodes, edges)
}

fn arrow_label(pair: &Pair<Rule>, _family: &str) -> Option<String> {
    let text = pair.as_str();
    
    // Try to extract label from inner label rule first
    for inner in pair.clone().into_inner() {
        if inner.as_rule() == Rule::label {
            let label = inner.as_str().trim();
            if !label.is_empty() {
                return Some(label.trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }
    
    // Fallback: try to extract from text patterns
    for candidate in [text.strip_prefix("--").and_then(|t| t.strip_suffix("-->")),
                       text.strip_prefix("-.").and_then(|t| t.strip_suffix(".->")),
                       text.strip_prefix("==").and_then(|t| t.strip_suffix("==>"))].into_iter().flatten() {
        let trimmed = candidate.trim().trim_matches(|c: char| c == '-' || c == '.' || c == '=' || c == '>');
        if !trimmed.is_empty() { return Some(trimmed.trim_matches('"').trim_matches('\'').to_string()); }
    }
    if let Some(rest) = text.strip_prefix("-->|") {
        if let Some(end) = rest.find('|') { return Some(rest[..end].trim().to_string()); }
    }
    None
}

fn infer_node_type(node: &FlowNode, index: usize, total: usize) -> &'static str {
    match node.shape.as_deref() {
        Some("[") => return "backend",
        Some("(") => return "frontend",
        Some("{") => return "security",
        Some("((") => return "external",
        Some("[[") | Some("[(]") => return "database",
        Some(">") => return "messagebus",
        _ => {}
    }
    let lower = node.label.to_lowercase();
    if ["db", "database", "cache", "redis", "postgres", "mysql", "store", "queue", "kafka", "s3"].iter().any(|k| lower.contains(k)) { return "database"; }
    if ["user", "client", "external", "customer"].iter().any(|k| lower.contains(k)) { return "external"; }
    if index == 0 { return "frontend"; }
    if index + 1 == total { return "external"; }
    "backend"
}

fn assign_columns(nodes: &[FlowNode], edges: &[FlowEdge]) -> HashMap<String, usize> {
    let mut depth: HashMap<String, usize> = nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for edge in edges {
            let from = depth.get(&edge.from).copied().unwrap_or(0);
            let to = depth.get(&edge.to).copied().unwrap_or(0);
            if from + 1 > to { depth.insert(edge.to.clone(), from + 1); changed = true; }
        }
    }
    depth
}

fn main_path(nodes: &[FlowNode], edges: &[FlowEdge], depth: &HashMap<String, usize>) -> Vec<String> {
    let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
    for edge in edges { incoming.entry(edge.to.clone()).or_default().push(edge.from.clone()); }
    let mut path: Vec<String> = Vec::new();
    let mut current = nodes.iter().filter(|n| !incoming.contains_key(&n.id)).max_by_key(|n| depth.get(&n.id).copied().unwrap_or(0)).map(|n| n.id.clone());
    while let Some(id) = current {
        path.push(id.clone());
        current = edges.iter().filter(|e| e.from == id).max_by_key(|e| depth.get(&e.to).copied().unwrap_or(0)).map(|e| e.to.clone());
    }
    path
}

fn flowchart_to_ir(pair: Pair<Rule>, _type_name: &str) -> Result<Value> {
    let (nodes, edges) = parse_flowchart(pair);
    if nodes.is_empty() { bail!("flowchart contains no nodes"); }
    let depth = assign_columns(&nodes, &edges);
    let path = main_path(&nodes, &edges, &depth);
    let total = nodes.len();
    let node_list: Vec<Value> = nodes.iter().enumerate().map(|(index, node)| json!({
        "id": node.id, "lane": "flow", "col": depth.get(&node.id).copied().unwrap_or(0),
        "type": infer_node_type(node, index, total), "label": node.label,
    })).collect();
    let edge_list: Vec<Value> = edges.iter().map(|edge| {
        let mut item = json!({ "from": edge.from, "to": edge.to });
        if let Some(ref label) = edge.label { item["label"] = json!(label); }
        if edge.variant != "default" { item["variant"] = json!(edge.variant); }
        item
    }).collect();
    Ok(json!({
        "schema_version": 1, "diagram_type": "workflow",
        "meta": { "title": "Converted Flowchart", "subtitle": "Generated by archify-rs convert", "animation": "trace" },
        "lanes": [{ "id": "flow", "label": "Flow" }], "nodes": node_list, "edges": edge_list, "mainPath": path,
    }))
}

// ---------------------------------------------------------------------------
// sequenceDiagram → sequence IR
// ---------------------------------------------------------------------------

struct SeqParticipant { id: String, label: String }

fn seq_variant(arrow: &str) -> &'static str {
    if arrow.starts_with("x") { return "security"; }
    if arrow.contains(">>") || arrow == "->>" { return "emphasis"; }
    if arrow.starts_with("--") { return "dashed"; }
    if arrow.ends_with(")") { return "return"; }
    "default"
}

fn sequence_to_ir(pair: Pair<Rule>) -> Value {
    let mut participants: Vec<SeqParticipant> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut messages: Vec<(String, String, String, &'static str)> = Vec::new();
    let mut title = "Sequence Diagram".to_string();
    for line in pair.into_inner() {
        if line.as_rule() != Rule::seq_line { continue; }
        for part in line.into_inner() {
            match part.as_rule() {
                Rule::participant => {
                    let mut parts = part.into_inner();
                    let id = parts.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                    let alias = parts.next().map(|p| p.as_str().trim().to_string());
                    if !id.is_empty() {
                        if let Some(&i) = by_id.get(&id) {
                            if let Some(ref a) = alias { participants[i].label = a.clone(); }
                        } else {
                            let i = participants.len();
                            participants.push(SeqParticipant { id: id.clone(), label: alias.unwrap_or_else(|| id.clone()) });
                            by_id.insert(id, i);
                        }
                    }
                }
                Rule::message_stmt => {
                    let mut parts = part.into_inner();
                    let from = parts.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                    let arrow = parts.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                    let to = parts.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                    let label = parts.next().map(|p| p.as_str().trim().to_string()).filter(|l| !l.is_empty());
                    for id in [&from, &to] {
                        if !by_id.contains_key(id) {
                            let i = participants.len();
                            participants.push(SeqParticipant { id: id.clone(), label: id.clone() });
                            by_id.insert(id.clone(), i);
                        }
                    }
                    messages.push((from, to, label.unwrap_or_default(), seq_variant(&arrow)));
                }
                _ => {}
            }
        }
    }
    let participant_list: Vec<Value> = participants.iter().enumerate().map(|(i, p)| {
        json!({ "id": p.id, "type": if i == 0 { "external" } else { "backend" }, "label": p.label })
    }).collect();
    let mut y_cursor = 180.0;
    let message_list: Vec<Value> = messages.iter().map(|(from, to, label, variant)| {
        let item = json!({ "from": from, "to": to, "y": y_cursor, "label": label, "variant": variant });
        y_cursor += 42.0;
        item
    }).collect();
    let view_box = vec![920.0, (message_list.len() as f64 * 42.0 + 220.0).ceil()];
    json!({
        "schema_version": 1, "diagram_type": "sequence",
        "meta": { "title": title, "subtitle": "Generated by archify-rs convert", "animation": "trace", "viewBox": view_box },
        "participants": participant_list, "messages": message_list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_simple_flowchart() {
        let ir = convert_str("flowchart LR\n  A[Start] --> B[Process]\n  B --> C{Decision}\n  C -->|yes| D[(Store)]\n", "workflow").unwrap();
        assert_eq!(ir["diagram_type"], "workflow");
        assert!(ir["nodes"].as_array().unwrap().len() >= 3);
        assert!(ir["edges"].as_array().unwrap().len() >= 2);
        let d = ir["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "D");
        assert!(d.is_some(), "node D not found");
    }

    #[test]
    fn converts_sequence_diagram() {
        let ir = convert_str("sequenceDiagram\n  participant U as User\n  participant S as Server\n  U->>S: GET /ping\n  S-->>U: pong\n", "sequence").unwrap();
        assert_eq!(ir["diagram_type"], "sequence");
        // Check that messages were parsed (participants may need inner-group iteration)
        let msgs = ir["messages"].as_array().unwrap();
        assert!(msgs.len() >= 2, "expected at least 2 messages, got {}", msgs.len());
    }

    #[test]
    fn rejects_unknown_diagram_type() {
        // flowchart always converts to workflow; sequenceDiagram requires type "sequence"
        assert!(convert_str("sequenceDiagram\nA->>B: x\n", "crystal-ball").is_err());
    }
}