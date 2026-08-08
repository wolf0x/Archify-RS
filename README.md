# Archify-RS

**Zero-dependency Archify CLI** — generates interactive HTML architecture diagrams from JSON IR, Mermaid text, or source repositories, entirely in Rust. No Node.js required.

## Quick Start

```bash
# Validate a JSON IR file
archify-rs validate -t architecture -i my-architecture.json

# Render an HTML diagram
archify-rs render -t architecture -i my-architecture.json -o diagram.html

# Convert a Mermaid flowchart to IR
archify-rs convert -f diagram.mmd -t workflow -o ir.json

# Analyze a code repository
archify-rs analyze --path ./my-project -o architecture.json
```

## Diagram Types

| Type | Description |
|------|-------------|
| `architecture` | System architecture with components, boundaries, and connections |
| `workflow` | Lane-based workflow with nodes, phases, and groups |
| `sequence` | Timeline-based sequence diagram |
| `dataflow` | Stage-based data-flow diagram |
| `lifecycle` | State lifecycle diagram |

## Building

```bash
cargo build --release
```

The binary will be at `target/release/archify-rs.exe`.

## Features

- **Zero external runtime** — single binary, no Node.js, no npm
- **Pixel-identical output** — uses the same Archify front-end template
- **All 5 diagram types** — architecture, workflow, sequence, dataflow, lifecycle
- **Mermaid converter** — flowchart → workflow, sequenceDiagram → sequence
- **Repository analyzer** — Python, Rust, TypeScript, Go, Java (tree-sitter)
- **JSON Schema validation** — embedded official Archify schemas

## License

MIT