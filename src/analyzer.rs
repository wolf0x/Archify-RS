//! Repository analyzer — walks a source tree, parses files with tree-sitter
//! and emits an `architecture` JSON IR: one component per module, one
//! connection per internal dependency edge.
//!
//! Supported languages: Python, Rust, TypeScript/JavaScript, Go, Java.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use walkdir::WalkDir;

/// Language descriptors: extension filter + optional override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Python,
    Rust,
    TypeScript,
    Go,
    Java,
}

impl Lang {
    fn from_name(name: &str) -> Option<Lang> {
        match name.to_lowercase().as_str() {
            "python" | "py" => Some(Lang::Python),
            "rust" | "rs" => Some(Lang::Rust),
            "typescript" | "ts" | "javascript" | "js" | "tsx" | "jsx" => Some(Lang::TypeScript),
            "go" | "golang" => Some(Lang::Go),
            "java" => Some(Lang::Java),
            _ => None,
        }
    }

    fn detect(path: &Path) -> Option<Lang> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("py") => Some(Lang::Python),
            Some("rs") => Some(Lang::Rust),
            Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Some(Lang::TypeScript),
            Some("go") => Some(Lang::Go),
            Some("java") => Some(Lang::Java),
            _ => None,
        }
    }

    fn grammar(&self) -> tree_sitter::Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    fn statement_kinds(&self) -> &'static [&'static str] {
        match self {
            Lang::Python => &["import_statement", "import_from_statement"],
            Lang::Rust => &["use_declaration"],
            Lang::TypeScript => &["import_statement", "export_statement", "import_require_clause"],
            Lang::Go => &["import_declaration"],
            Lang::Java => &["import_declaration"],
        }
    }
}

struct Module {
    id: String,
    label: String,
    path: String,
    kind: String,
    imports: Vec<String>,
}

/// Sanitize a relative module path into a schema-valid id
/// (`^[a-zA-Z][a-zA-Z0-9_-]*$`).
fn sanitize_id(relative: &str) -> String {
    let sanitized: String = relative
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Strip leading underscores efficiently
    let trimmed = sanitized.trim_start_matches('_');
    if trimmed.is_empty() || !trimmed.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        format!("m{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Infer a component kind from the module path.
fn infer_kind(relative: &str) -> &'static str {
    let lower = relative.to_lowercase();
    if lower.contains("test") || lower.contains("spec") || lower.contains("__init__") || lower.contains("migration") {
        return "external";
    }
    if ["db", "database", "model", "models", "repo", "repository", "store", "cache", "redis", "schema", "dao"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return "database";
    }
    if ["auth", "security", "policy", "acl", "jwt"].iter().any(|k| lower.contains(k)) {
        return "security";
    }
    if ["ui", "view", "views", "page", "pages", "component", "components", "frontend", "web", "app"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return "frontend";
    }
    if ["api", "server", "service", "services", "handler", "handlers", "controller", "controllers", "routes", "endpoint", "worker", "main"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return "backend";
    }
    "backend"
}

/// Extract import targets from a statement's raw text.
fn extract_imports(lang: Lang, text: &str) -> Vec<String> {
    match lang {
        Lang::Python => {
            let mut out = Vec::new();
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("from ") {
                    if let Some(module) = rest.split(" import ").next() {
                        out.push(module.trim().to_string());
                    }
                } else if let Some(rest) = trimmed.strip_prefix("import ") {
                    // `import a, b` or `import a.b as c`
                    for part in rest.split(',') {
                        let module = part.trim().split(" as ").next().unwrap_or("").trim();
                        if !module.is_empty() {
                            out.push(module.to_string());
                        }
                    }
                }
            }
            out
        }
        Lang::Rust => {
            let mut out = Vec::new();
            for line in text.lines() {
                let trimmed = line.trim().trim_end_matches(';').trim();
                if let Some(rest) = trimmed.strip_prefix("use ") {
                    if let Some(path) = rest.split(" as ").next() {
                        if let Some(path) = path.split('{').next() {
                            out.push(path.trim().to_string());
                        }
                    }
                }
            }
            out
        }
        Lang::TypeScript => {
            let mut out = Vec::new();
            let mut rest = text;
            while let Some(idx) = rest.find("from") {
                let after = rest[idx + 4..].trim_start();
                if let Some(quoted) = after.strip_prefix(['\'', '\"']) {
                    if let Some(end_rel) = quoted.find(['\'', '\"']) {
                        out.push(quoted[..end_rel].to_string());
                        rest = &quoted[end_rel + 1..];
                        continue;
                    }
                }
                rest = after;
            }
            // dynamic import() / require()
            let mut rest = text;
            while let Some(idx) = rest.find(['\'', '"']) {
                let quote = rest.as_bytes()[idx] as char;
                let start = idx + 1;
                if let Some(end_rel) = rest[start..].find(quote) {
                    let end = start + end_rel;
                    let target = &rest[start..end];
                    let before = &rest[..idx];
                    if before.trim_end().ends_with('(') || before.trim_end().ends_with("require") {
                        out.push(target.to_string());
                    }
                    rest = &rest[end + 1..];
                    continue;
                }
                break;
            }
            out
        }
        Lang::Go => {
            let mut out = Vec::new();
            for line in text.lines() {
                let trimmed = line.trim().trim_end_matches(';');
                if let Some(rest) = trimmed.strip_prefix("import") {
                    for part in rest.split('"') {
                        let part = part.trim();
                        if !part.is_empty() && !part.contains('(') && !part.contains(')') {
                            out.push(part.to_string());
                        }
                    }
                }
            }
            out
        }
        Lang::Java => {
            let mut out = Vec::new();
            for line in text.lines() {
                let trimmed = line.trim().trim_end_matches(';');
                if let Some(rest) = trimmed.strip_prefix("import ") {
                    if let Some(static_idx) = rest.find("static") {
                        out.push(rest[static_idx + 6..].trim().to_string());
                    } else {
                        out.push(rest.trim().to_string());
                    }
                }
            }
            out
        }
    }
}

/// Resolve an import target to a known module id by path-prefix matching.
fn resolve_import(target: &str, modules: &HashMap<String, String>) -> Option<String> {
    let raw = target.replace('\\', "/");
    // Normalize relative prefixes (./x, ../x, @/x) away for matching.
    let mut normalized = raw.trim_matches('/').to_string();
    while normalized.starts_with("./") || normalized.starts_with("../") || normalized.starts_with("@/") {
        if normalized.starts_with("./") || normalized.starts_with("@/") {
            normalized = normalized[2..].to_string();
        } else {
            normalized = normalized[3..].to_string();
        }
    }
    if normalized.is_empty() {
        return None;
    }
    for (id, path) in modules {
        let path = path.replace('\\', "/");
        if path == normalized {
            return Some(id.clone());
        }
        // a/b/c imports a.b.c or a/b/c (python dotted / path styles)
        let dotted = normalized.replace('/', ".");
        let as_path = normalized.replace('.', "/");
        if path == dotted || path == as_path || path.ends_with(&format!("/{as_path}")) {
            return Some(id.clone());
        }
        // Strip the extension from the known path (utils.ts vs utils).
        let stem = path.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&path);
        if stem == as_path || stem.ends_with(&format!("/{as_path}")) {
            return Some(id.clone());
        }
        // Prefix match: import of a submodule implies a dependency on the
        // nearest known ancestor module.
        if normalized.starts_with(&path) && normalized.len() > path.len() {
            return Some(id.clone());
        }
    }
    None
}

fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "target" | "dist" | "build" | ".git" | ".venv" | "venv" | "__pycache__" | ".next" | "vendor" | ".idea" | ".vscode" | "coverage"
    )
}

/// Analyze a repository directory and produce an architecture IR.
pub fn analyze_repo(root: &Path, lang_override: Option<&str>) -> Result<Value> {
    if !root.is_dir() {
        bail!("path {} is not a directory", root.display());
    }
    let lang = match lang_override {
        Some(name) => Lang::from_name(name)
            .ok_or_else(|| anyhow!("unsupported language \"{name}\" (python, rust, typescript, go, java)"))?,
        None => Lang::Python,
    };
    let lang_used = lang;

    // Discover source files.
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .strip_prefix(root)
            .ok()
            .and_then(|rel| rel.components().next())
            .and_then(|c| c.as_os_str().to_str())
            .map(is_ignored_dir)
            .unwrap_or(false)
        {
            continue;
        }
        let detected = Lang::detect(path);
        if lang_override.is_none() {
            if detected.is_some() {
                files.push(path.to_path_buf());
            }
        } else if detected == Some(lang_used) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    if files.is_empty() {
        bail!("no source files found under {}", root.display());
    }

    // Parse every file and collect modules.
    let mut modules: Vec<Module> = Vec::new();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang_used.grammar())
        .with_context(|| "failed to load tree-sitter grammar")?;

    for file in &files {
        let relative = file
            .strip_prefix(root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(_) => continue, // binary or non-UTF8
        };
        let tree = parser.parse(&source, None);
        let Some(tree) = tree else { continue };
        let mut imports: Vec<String> = Vec::new();
        let mut stack = vec![tree.root_node().to_owned()];
        while let Some(node) = stack.pop() {
            let kind = node.kind();
            if lang_used.statement_kinds().contains(&kind) {
                let text = node.utf8_text(source.as_bytes()).unwrap_or("");
                imports.extend(extract_imports(lang_used, text));
            }
            let mut child_cursor = node.walk();
            for child in node.children(&mut child_cursor) {
                stack.push(child);
            }
        }
        imports.sort();
        imports.dedup();
        let id = sanitize_id(&relative);
        modules.push(Module {
            id: id.clone(),
            label: relative.rsplit('/').next().unwrap_or(&relative).to_string(),
            path: relative.clone(),
            kind: infer_kind(&relative).to_string(),
            imports,
        });
    }

    // Build id → relative-path map for resolution.
    let path_by_id: HashMap<String, String> = modules.iter().map(|m| (m.id.clone(), m.path.clone())).collect();

    // Build internal dependency edges.
    let mut depth: HashMap<String, usize> = modules.iter().map(|m| (m.id.clone(), 0)).collect();
    let mut internal_edges: Vec<(String, String)> = Vec::new();
    for module in &modules {
        for import in &module.imports {
            if import.starts_with('.') {
                continue; // relative python imports need package resolution (v1: skip)
            }
            if let Some(target) = resolve_import(import, &path_by_id) {
                if target != module.id {
                    internal_edges.push((module.id.clone(), target));
                }
            }
        }
    }
    // Compute grid columns via topological depth of the dependency graph.
    // Use Bellman-Ford with iteration limit to detect cycles.
    let mut depth: HashMap<String, usize> = modules.iter().map(|m| (m.id.clone(), 0)).collect();
    let max_iterations = modules.len() + 1;
    for iteration in 0..max_iterations {
        let mut changed = false;
        for (from, to) in &internal_edges {
            let from_depth = depth.get(from).copied().unwrap_or(0);
            let to_depth = depth.get(to).copied().unwrap_or(0);
            if from_depth + 1 > to_depth {
                depth.insert(to.clone(), from_depth + 1);
                changed = true;
            }
        }
        if !changed {
            break;
        }
        if iteration == max_iterations - 1 {
            // Cycle detected — fall back to depth 0 for all modules
            log::warn!("dependency cycle detected; using flat layout");
            for d in depth.values_mut() {
                *d = 0;
            }
        }
    }

    // Assign rows so that same-column modules stack without overlap.
    let mut col_counts: HashMap<usize, usize> = HashMap::new();
    let mut rows: HashMap<String, usize> = HashMap::new();
    let mut ordered: Vec<&Module> = modules.iter().collect();
    ordered.sort_by(|a, b| {
        depth
            .get(&a.id)
            .unwrap_or(&0)
            .cmp(depth.get(&b.id).unwrap_or(&0))
            .then_with(|| a.path.cmp(&b.path))
    });
    for module in &ordered {
        let col = depth.get(&module.id).copied().unwrap_or(0);
        let row = col_counts.entry(col).or_insert(0);
        rows.insert(module.id.clone(), *row);
        *row += 1;
    }

    let mut component_list: Vec<Value> = Vec::new();
    for module in &modules {
        if module.kind == "external" {
            continue; // drop test scaffolding and package markers
        }
        let row = rows.get(&module.id).copied().unwrap_or(0);
        let col = depth.get(&module.id).copied().unwrap_or(0);
        component_list.push(json!({
            "id": module.id,
            "type": module.kind,
            "label": module.label,
            "sublabel": module.path,
            "row": row,
            "col": col,
        }));
    }
    if component_list.is_empty() {
        bail!("no analyzable modules found under {}", root.display());
    }

    let mut connection_list: Vec<Value> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    internal_edges.sort();
    for (from, to) in internal_edges {
        if from == to || !seen.insert((from.clone(), to.clone())) {
            continue;
        }
        connection_list.push(json!({ "from": from, "to": to }));
    }

    let repo_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("repository");
    Ok(json!({
        "schema_version": 1,
        "diagram_type": "architecture",
        "meta": {
            "title": format!("{repo_name} Architecture"),
            "subtitle": format!("Analyzed by archify-rs ({} files, {} modules)", files.len(), modules.len()),
            "animation": "trace"
        },
        "layout": {
            "mode": "grid",
            "origin": [40, 80],
            "cols": 12,
            "gapX": 30,
            "gapY": 40,
            "cellW": 130,
            "cellH": 64
        },
        "components": component_list,
        "connections": connection_list
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ids_are_schema_valid() {
        assert_eq!(sanitize_id("src/utils/logger.py"), "src_utils_logger_py");
        assert!(sanitize_id("123abc.rs").starts_with('m'));
        assert_eq!(sanitize_id("main.rs"), "main_rs");
    }

    #[test]
    fn infer_kinds_from_paths() {
        assert_eq!(infer_kind("src/models/user.py"), "database");
        assert_eq!(infer_kind("src/api/routes.ts"), "backend");
        assert_eq!(infer_kind("src/ui/App.tsx"), "frontend");
        assert_eq!(infer_kind("src/auth/jwt.rs"), "security");
        assert_eq!(infer_kind("src/util/helpers.rs"), "backend");
    }

    #[test]
    fn extracts_python_imports() {
        let imports = extract_imports(Lang::Python, "import os\nfrom .models import User\nfrom src.db import session as s\n");
        assert_eq!(imports, vec!["os", ".models", "src.db"]);
    }

    #[test]
    fn extracts_rust_imports() {
        let imports = extract_imports(Lang::Rust, "use crate::db::pool;\nuse std::collections::HashMap;\n");
        assert_eq!(imports, vec!["crate::db::pool", "std::collections::HashMap"]);
    }

    #[test]
    fn extracts_ts_imports() {
        let imports = extract_imports(
            Lang::TypeScript,
            "import { x } from './utils';\nimport y from \"@/api/client\";\nconst z = require('lodash');\n",
        );
        assert!(imports.contains(&"./utils".to_string()));
        assert!(imports.contains(&"@/api/client".to_string()));
        assert!(imports.contains(&"lodash".to_string()));
    }

    #[test]
    fn resolves_import_to_module() {
        let modules = HashMap::from([
            ("src_api_routes_ts".to_string(), "src/api/routes.ts".to_string()),
            ("src_utils_ts".to_string(), "src/utils.ts".to_string()),
        ]);
        assert_eq!(resolve_import("./utils", &modules), Some("src_utils_ts".to_string()));
        assert_eq!(resolve_import("src/api/routes", &modules), Some("src_api_routes_ts".to_string()));
        assert_eq!(resolve_import("unknown/pkg", &modules), None);
    }
}
