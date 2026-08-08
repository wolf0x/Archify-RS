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

# Compare two architecture snapshots and render a delta review page
archify-rs compare -a base.architecture.json -b head.architecture.json -o delta.html

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

### Binary size

The release profile optimizes for size (`opt-level = "z"`, `panic = "abort"`, LTO):

| Build | Size | Includes |
|---|---|---|
| Default (`analyzer` feature) | ~6.6 MB | `validate`, `render`, `convert`, `compare`, `analyze` |
| `--no-default-features` | ~3.1 MB | `validate`, `render`, `convert`, `compare` |

Build the minimal binary (no tree-sitter repository analyzer):

```bash
cargo build --release --no-default-features
```

## Features

- **Zero external runtime** — single binary, no Node.js, no npm
- **Pixel-identical output** — uses the same Archify front-end template
- **All 5 diagram types** — architecture, workflow, sequence, dataflow, lifecycle
- **Architecture delta (`compare`)** — base → head change review with per-entity add/remove/change/move states, phantom baselines, and a guided review strip
- **Mermaid converter** — flowchart → workflow, sequenceDiagram → sequence
- **Repository analyzer** — Python, Rust, TypeScript, Go, Java (tree-sitter)
- **JSON Schema validation** — embedded official Archify schemas
- **AI Skill support** — includes skill definition for AI assistant integration

## AI Skill Integration

Archify-RS includes a skill definition for AI assistants in `skills/archify-rs/`. This enables AI tools to:

- Generate architecture diagrams from natural language descriptions
- Convert Mermaid diagrams to Archify format
- Analyze code repositories automatically
- Validate JSON IR against official schemas

See `skills/archify-rs/README.md` for details.

## License

MIT