# Archify-RS Skill Package

This directory contains the skill definition for Archify-RS, enabling AI assistants to use the Rust-based Archify CLI for architecture visualization.

The package is **fully self-contained** — all schemas, examples, and the viewer template are bundled as real files. It does not depend on any other directory in the repository, so deleting sibling folders (such as a Node.js `skills/archify` reference copy) has no effect on this skill.

## Structure

```
skills/archify-rs/
├── SKILL.md           # Skill definition (triggers, description, usage)
├── README.md          # This file
├── schemas/           # 6 official JSON Schemas (architecture, common, dataflow,
│                      #   lifecycle, sequence, workflow)
├── examples/          # 14 validated JSON IR examples (one per diagram type,
│                      #   plus an analyzer-generated architecture sample)
└── assets/            # template.html — the unmodified Archify viewer template
```

## Usage

When an AI assistant has this skill loaded, it can:

1. **Create architecture diagrams** from JSON IR
2. **Convert Mermaid diagrams** to Archify format
3. **Analyze code repositories** to generate architecture diagrams
4. **Validate JSON IR** against official schemas

## Example Interactions

### Create an architecture diagram

```
User: Create an architecture diagram for a web application with:
      - Frontend (React)
      - API Gateway
      - Backend services (User Service, Order Service)
      - Database (PostgreSQL)
      - Cache (Redis)

AI: [Uses archify-rs to generate JSON IR and render HTML]
```

### Convert Mermaid to Archify

```
User: Convert this Mermaid flowchart to an Archify diagram:
      flowchart LR
        A[Client] --> B[API]
        B --> C[Database]

AI: [Uses archify-rs convert to transform Mermaid to JSON IR]
```

### Analyze a repository

```
User: Analyze the code in ./my-project and generate an architecture diagram

AI: [Uses archify-rs analyze to scan the codebase and generate diagram]
```

## Differences from Node.js Skill

The original `skills/archify` uses Node.js (`bin/archify.mjs`), while this skill uses the Rust binary (`archify-rs`). Both produce identical output, but the Rust version:

- ✅ No Node.js required
- ✅ Faster startup (milliseconds vs seconds)
- ✅ Smaller footprint (~9MB binary vs full Node.js runtime)
- ✅ Cross-platform (Windows, Linux, macOS)
- ✅ Self-contained package (no junction links to other folders)

## Installation

1. Download `archify-rs` binary from [GitHub Releases](https://github.com/wolf0x/Archify-RS/releases)
2. Add it to your PATH
3. Load this skill in your AI assistant

## Testing

Verify the skill works (paths are relative to this directory):

```bash
archify-rs --help
archify-rs validate -t architecture -i examples/production-deployment.architecture.json
archify-rs render -t architecture -i examples/production-deployment.architecture.json -o test.html
```
