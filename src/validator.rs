//! Schema validation for Archify JSON IR.
//!
//! The official Archify JSON Schemas (schemas/*.schema.json) are embedded into
//! the binary at compile time and compiled with the `jsonschema` crate, exactly
//! replacing the original Node.js ajv pipeline. A custom retriever resolves the
//! `$ref` chain (e.g. `common.schema.json#/$defs/id`) against the embedded
//! documents without any network access.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use jsonschema::{Draft, Retrieve, Uri, ValidationError, Validator};
use once_cell::sync::Lazy;
use serde_json::Value;

pub const DIAGRAM_TYPES: [&str; 5] = ["architecture", "workflow", "sequence", "dataflow", "lifecycle"];

macro_rules! embed_schemas {
    ($($name:literal),* $(,)?) => {{
        let mut map: HashMap<&'static str, Value> = HashMap::new();
        $(
            map.insert(
                $name,
                serde_json::from_str(include_str!(concat!("../schemas/", $name))).expect("embedded schema must be valid JSON"),
            );
        )*
        map
    }};
}

pub static SCHEMA_DOCS: Lazy<HashMap<&'static str, Value>> = Lazy::new(|| {
    embed_schemas!(
        "architecture.schema.json",
        "common.schema.json",
        "dataflow.schema.json",
        "lifecycle.schema.json",
        "sequence.schema.json",
        "workflow.schema.json"
    )
});

/// Retriever that resolves every schema reference from the embedded documents.
struct EmbeddedRetriever;

impl Retrieve for EmbeddedRetriever {
    fn retrieve(&self, uri: &Uri<&str>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let raw = uri.as_str();
        for (name, doc) in SCHEMA_DOCS.iter() {
            if raw.ends_with(name) {
                return Ok(doc.clone());
            }
        }
        Err(anyhow!("schema resource not embedded: {raw}").into())
    }
}

/// Per-type validator cache keyed by diagram type.
static VALIDATORS: Lazy<Mutex<HashMap<String, std::sync::Arc<Validator>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn validator_for(diagram_type: &str) -> Result<std::sync::Arc<Validator>> {
    let schema_name = format!("{diagram_type}.schema.json");
    let schema = SCHEMA_DOCS
        .get(schema_name.as_str())
        .ok_or_else(|| anyhow!("unknown diagram type \"{diagram_type}\". Expected one of: {}", DIAGRAM_TYPES.join(", ")))?;
    
    // Check cache first
    {
        let cache = VALIDATORS.lock().unwrap();
        if let Some(validator) = cache.get(&schema_name) {
            return Ok(validator.clone());
        }
    }
    
    // Compile validator outside lock to avoid holding it during expensive work
    let mut options = jsonschema::options();
    options.with_draft(Draft::Draft202012).with_retriever(EmbeddedRetriever);
    let validator = options
        .build(schema)
        .with_context(|| format!("failed to compile schema for diagram type \"{diagram_type}\""))?;
    let validator = std::sync::Arc::new(validator);
    
    // Insert into cache, handling race condition where another thread may have
    // compiled the same validator while we were working
    let mut cache = VALIDATORS.lock().unwrap();
    if let Some(existing) = cache.get(&schema_name) {
        // Another thread compiled it first; use their version
        return Ok(existing.clone());
    }
    cache.insert(schema_name, validator.clone());
    Ok(validator)
}

/// Result of a validation pass.
#[derive(Debug)]
pub struct ValidationReport {
    pub bytes: usize,
}

fn collect_errors(validator: &Validator, instance: &Value) -> Vec<String> {
    let mut messages = Vec::new();
    for error in validator.iter_errors(instance) {
        messages.push(format_error(&error));
    }
    messages
}

fn format_error(error: &ValidationError) -> String {
    let path = error.instance_path.to_string();
    if path.is_empty() {
        format!("{error}")
    } else {
        format!("{path}: {error}")
    }
}

/// Validate an in-memory IR value for the given diagram type.
pub fn validate_value(diagram_type: &str, value: &Value) -> Result<ValidationReport> {
    if !DIAGRAM_TYPES.contains(&diagram_type) {
        bail!("unknown diagram type \"{diagram_type}\". Expected one of: {}", DIAGRAM_TYPES.join(", "));
    }
    let validator = validator_for(diagram_type)?;
    let errors = collect_errors(&validator, value);
    if !errors.is_empty() {
        let details: Vec<&str> = errors.iter().map(String::as_str).collect();
        bail!(
            "validation failed for {diagram_type} diagram:\n  - {}",
            details.join("\n  - ")
        );
    }
    validate_relationship_ids(diagram_type, value)?;
    validate_guided_views(diagram_type, value)?;
    Ok(ValidationReport { bytes: serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0) })
}

/// Maximum input file size (100 MB) to prevent memory exhaustion attacks.
const MAX_INPUT_SIZE: usize = 100 * 1024 * 1024;

/// Validate a JSON IR file on disk.
pub fn validate_file(input: &std::path::Path, diagram_type: &str) -> Result<ValidationReport> {
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
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("input {} is not valid JSON", input.display()))?;
    let report = validate_value(diagram_type, &value)?;
    Ok(ValidationReport { bytes: report.bytes.max(text.len()) })
}

/// The relationship collections per diagram type, mirroring the original
/// shared `validateRelationshipIds` contract.
fn relationship_collection(diagram_type: &str) -> &'static str {
    match diagram_type {
        "architecture" => "connections",
        "workflow" => "edges",
        "sequence" => "messages",
        "dataflow" => "flows",
        "lifecycle" => "transitions",
        _ => "",
    }
}

/// The semantic-id collections per diagram type, mirroring the original
/// shared `validateGuidedViews` contract.
fn semantic_collection(diagram_type: &str) -> &'static str {
    match diagram_type {
        "architecture" => "components",
        "workflow" => "nodes",
        "sequence" => "participants",
        "dataflow" => "nodes",
        "lifecycle" => "states",
        _ => "",
    }
}

/// Relationship ids must be unique once authored (optional, for backwards
/// compatibility with the original CLI).
fn validate_relationship_ids(diagram_type: &str, diagram: &Value) -> Result<()> {
    let collection = relationship_collection(diagram_type);
    let relationships = diagram.get(collection).and_then(Value::as_array);
    let mut seen: Vec<&str> = Vec::new();
    let mut problems = Vec::new();
    if let Some(relationships) = relationships {
        for (index, relationship) in relationships.iter().enumerate() {
            let id = relationship.get("id").and_then(Value::as_str);
            match id {
                None | Some("") => {}
                Some(id) => {
                    if seen.contains(&id) {
                        problems.push(format!(
                            "/{collection}/{index}/id duplicates relationship id \"{id}\""
                        ));
                    }
                    seen.push(id);
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        bail!("relationship identity validation failed:\n  - {}", problems.join("\n  - "))
    }
}

/// Guided views must reference existing semantic ids with unique view/focus ids.
fn validate_guided_views(diagram_type: &str, diagram: &Value) -> Result<()> {
    let views = diagram.pointer("/meta/views").and_then(Value::as_array);
    let Some(views) = views else {
        return Ok(());
    };
    if views.is_empty() {
        return Ok(());
    }
    let collection = semantic_collection(diagram_type);
    let semantic_ids: Vec<&str> = diagram
        .get(collection)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.get("id").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    let mut seen_views: Vec<&str> = Vec::new();
    let mut problems = Vec::new();
    for (index, view) in views.iter().enumerate() {
        if let Some(id) = view.get("id").and_then(Value::as_str) {
            if seen_views.contains(&id) {
                problems.push(format!("/meta/views/{index}/id duplicates view id \"{id}\""));
            }
            seen_views.push(id);
        }
        let mut seen_focus: Vec<&str> = Vec::new();
        if let Some(focus) = view.get("focus").and_then(Value::as_array) {
            for (focus_index, focus_id) in focus.iter().enumerate() {
                if let Some(id) = focus_id.as_str() {
                    if seen_focus.contains(&id) {
                        problems.push(format!(
                            "/meta/views/{index}/focus/{focus_index} duplicates semantic id \"{id}\""
                        ));
                    }
                    seen_focus.push(id);
                    if !semantic_ids.contains(&id) {
                        problems.push(format!(
                            "/meta/views/{index}/focus/{focus_index} references unknown semantic id \"{id}\""
                        ));
                    }
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        bail!("guided view validation failed:\n  - {}", problems.join("\n  - "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schemas_are_available() {
        assert!(SCHEMA_DOCS.contains_key("architecture.schema.json"));
        assert!(SCHEMA_DOCS.contains_key("common.schema.json"));
        assert_eq!(SCHEMA_DOCS.len(), 6);
    }

    #[test]
    fn validates_known_example() {
        let text = include_str!("../examples/production-deployment.architecture.json");
        let value: Value = serde_json::from_str(text).unwrap();
        validate_value("architecture", &value).expect("official example must validate");
    }

    #[test]
    fn rejects_wrong_type() {
        let value = serde_json::json!({
            "schema_version": 1,
            "diagram_type": "architecture",
            "meta": { "title": "x" },
            "components": []
        });
        let err = validate_value("architecture", &value).unwrap_err();
        assert!(err.to_string().contains("validation failed"), "{err}");
    }

    #[test]
    fn rejects_unknown_diagram_type() {
        let value = serde_json::json!({});
        assert!(validate_value("crystal-ball", &value).is_err());
    }
}
