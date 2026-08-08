---
name: "archify-rs"
description: "Create polished, validated architecture, workflow, sequence, data-flow, and lifecycle/state diagrams as explorable standalone HTML with inline SVG, dark/light themes, optional trace motion, and PNG/JPEG/WebP/SVG/WebM export. Accept plain-language requirements or pasted Mermaid input. Use when the user asks to visualize system architecture, infrastructure, technical workflows, API call sequences, data pipelines, or to convert Mermaid diagrams. Zero-dependency Rust implementation - no Node.js required."
triggers:
  - "archify-rs"
  - "archify"
  - "architecture diagram"
  - "架构图"
  - "流程图"
  - "时序图"
  - "数据流图"
  - "生命周期图"
  - "mermaid"
---


# Archify-RS

Create a self-contained, interactive HTML diagram from a small typed JSON specification. Static output is the default; enable motion only when the user asks for a demo or presentation.

**This is the Rust implementation** — zero dependencies, no Node.js required. Single binary, fast startup, cross-platform.

## Fast authoring path

Use this bounded path for ordinary generation. Do not read the optional Viewer Runtime reference unless the user asks about those features.

1. Choose `architecture`, `workflow`, `sequence`, `dataflow`, or `lifecycle` from the question.
2. Read one matching schema in `schemas/`, `schemas/common.schema.json`, and one matching JSON example in `examples/`. Read only those files. Fresh authorship means new stable IDs, domain wording, and layout; use the example for field shape, not facts.
3. Artifact first: the next tool action must write the candidate. Write the candidate before inspecting renderer internals. Do not plan exact coordinates in prose. Start with one clear main path, short side branches, sparse labels, and at most 12 primary nodes. Set `meta.quality_profile` to `"showcase"` unless the user explicitly requests a dense `standard` map. Start with automatic routes and labels. Do not add `via`, `channelX`, `channelY`, or `labelAt` before a diagnostic calls for one; apply at most one diagnosed geometry control per repair.
4. Validate after every candidate edit and immediately before handoff:

   ```bash
   archify-rs validate -t <type> -i <candidate.json>
   ```

   A passing validation means the JSON conforms to the schema. If validation fails, fix the reported errors and rerun.
5. Render the final HTML:

   ```bash
   archify-rs render -t <type> -i <candidate.json> -o <output.html>
   ```

   A non-zero exit can never be described as success. If rendering fails, check the error message and fix the input.

## Type router

| Type | Use for |
|---|---|
| `architecture` | Components, services, cloud/security boundaries, infrastructure |
| `workflow` | Processes, approval gates, tool calls, runbooks, CI/CD |
| `sequence` | API call chains, request lifecycles, async traces, returns |
| `dataflow` | Pipelines, ETL/ELT, lineage, governance, consumers |
| `lifecycle` | State/status transitions, retries, waiting and terminal states |

## Mermaid input

Convert Mermaid diagrams to Archify JSON IR, then render:

```bash
# Convert flowchart to workflow IR
archify-rs convert -f diagram.mmd -t workflow -o diagram.json

# Convert sequenceDiagram to sequence IR
archify-rs convert -f sequence.mmd -t sequence -o sequence.json

# Render the converted IR
archify-rs render -t workflow -i diagram.json -o output.html
```

- `flowchart` / `graph` → `workflow`, or `architecture` for a component map.
- `sequenceDiagram` → `sequence`; participants become semantic participants and arrows become messages.

## Repository analysis

Analyze a code repository to generate an architecture diagram automatically:

```bash
archify-rs analyze --path ./my-project -o architecture.json
archify-rs render -t architecture -i architecture.json -o architecture.html
```

Supported languages: Python, Rust, TypeScript, Go, Java.

## Authoring invariants

- One obvious main path; side branches leave the nearest main-path node. Remove low-value edges before adding routing controls.
- Omit `meta.legend` for the truthful `auto` default. When needed, use only `mode: auto|all|hidden` and renderer-supported `entries.<kind>.label|visible`; labels never change semantics.
- Component types are `frontend`, `backend`, `database`, `cloud`, `security`, `messagebus`, and `external`; variants are `default`, `emphasis`, `security`, and `dashed`.
- Spacing means clear gap, not center distance. For a relationship label, clear gap must exceed its measured mask width; otherwise omit the label or move it deliberately.
- Automatic routes own their endpoint sides. A side is a direction contract: the first and final segment must leave/enter perpendicular to that side.

## Delivery

Use `validate` during repair and `render` once for final output. The rendered HTML is self-contained with all CSS/JS inline.

Optional: add `--theme dark` or `--theme light` to set the default theme (defaults to dark).

```bash
archify-rs render -t architecture -i input.json -o output.html --theme dark
```

## Optional viewer capabilities

Generated HTML already contains theme switching, pan/zoom, search, focus, relationship tracing, semantic views, presentation, and truthful exports. These are reader capabilities, not extra authoring work. `meta.animation: "trace"` is opt-in; `meta.views` is optional and should contain at most five curated chapters.

## Setup and installation

Download the binary for your platform from [GitHub Releases](https://github.com/wolf0x/Archify-RS/releases) and add it to your PATH.

Verify installation:

```bash
archify-rs --help
```

## Output

Return the rendered HTML path, diagram type, and validation status. Do not claim success for a non-zero command.

## Differences from Node.js version

- **No Node.js required** — single binary, zero dependencies
- **Faster startup** — milliseconds vs seconds
- **Smaller footprint** — ~9MB binary vs full Node.js runtime
- **Same output** — pixel-identical HTML using the same template
- **Same schemas** — 100% compatible with official Archify JSON IR
