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

This skill package is fully self-contained: the JSON Schemas (`schemas/`), example IR files (`examples/`), and the viewer template (`assets/template.html`) are bundled as real files inside this directory. Nothing here depends on other directories in the repository, so the skill keeps working even if sibling folders are deleted.

## Setup and Installation

This skill requires the `archify-rs.exe` binary inside the skill directory. The binary is **not** distributed with the SKILL text files — it must be downloaded from GitHub Releases during installation.

### Step 1: Install the skill via `install_skill`

This creates the skill directory with all schemas, examples, and templates.

### Step 2: Download the binary

```powershell
# Resolve skill directory (using $HOME to be portable across user accounts)
$SKILL_ROOT = "$env:USERPROFILE\.RustAgent\workspace\skills\archify-rs"

# Download the Windows binary
# Replace VERSION with the latest release tag (e.g. v0.2.0)
curl.exe -sL "https://github.com/wolf0x/Archify-RS/releases/download/VERSION/archify-rs-x86_64-pc-windows-msvc.exe" -o "$SKILL_ROOT\archify-rs.exe"
```

### Step 3: Verify

```powershell
& "$env:USERPROFILE\.RustAgent\workspace\skills\archify-rs\archify-rs.exe" --help
```

Expected output: Usage information showing all available commands.

### Binary path convention

The binary must be at `$SKILL_ROOT\archify-rs.exe`. All commands in this SKILL resolve `$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"` and call `& "$SKILL_ROOT\archify-rs.exe"` — no PATH dependency.

## Runtime Compatibility

Before reading a reference or running a command, resolve the skill directory as `SKILL_ROOT`. Do not assume the current working directory is the skill directory.

**Binary location**: `$SKILL_ROOT\archify-rs.exe`

Every command block below resolves `SKILL_ROOT` itself. The `$HOME` fallback path is `$HOME/.RustAgent/workspace/skills/archify-rs`.

## CLI: `archify-rs.exe`

The unified binary provides all rendering, validation, conversion, analysis, and comparison commands:

### 1. `render` — JSON IR → Interactive HTML
```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" render -t <type> -i <candidate.json> -o <output.html> [--theme dark|light]
```

### 2. `validate` — Validate JSON IR against schema
```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" validate -t <type> -i <candidate.json>
```

### 3. `convert` — Mermaid → Archify JSON IR
```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" convert -f <diagram.mmd> -t <type> -o <output.json>
```

### 4. `analyze` — Code repository → Architecture JSON IR
```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" analyze --path <./my-project> -o <architecture.json>
```

### 5. `compare` — Two architecture snapshots → Delta HTML
```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" compare -a <base.json> -b <head.json> -o <delta.html>
```

## Fast authoring path

Use this bounded path for ordinary generation. Do not read the optional Viewer Runtime reference unless the user asks about those features.

1. **Choose type** — `architecture`, `workflow`, `sequence`, `dataflow`, or `lifecycle` from the question.

2. **Read reference** — Read one matching schema in `schemas/`, `schemas/common.schema.json`, and one matching JSON example in `examples/`. Read only those files. Fresh authorship means new stable IDs, domain wording, and layout; use the example for field shape, not facts.

3. **Write JSON** — Artifact first: the next tool action must write the candidate. Write the candidate before inspecting renderer internals. Do not plan exact coordinates in prose. Start with one clear main path, short side branches, sparse labels, and at most 12 primary nodes. Set `meta.quality_profile` to `"showcase"` unless the user explicitly requests a dense `standard` map. Start with automatic routes and labels. Do not add `via`, `channelX`, `channelY`, or `labelAt` before a diagnostic calls for one; apply at most one diagnosed geometry control per repair.

4. **Validate** — Validate after every candidate edit and immediately before handoff:
   ```powershell
   $SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
   & "$SKILL_ROOT\archify-rs.exe" validate -t <type> -i <candidate.json>
   ```
   A passing validation means the JSON conforms to the schema. If validation fails, fix the reported errors and rerun.

5. **Render** — Render the final HTML:
   ```powershell
   $SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
   & "$SKILL_ROOT\archify-rs.exe" render -t <type> -i <candidate.json> -o <output.html>
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

```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" convert -f diagram.mmd -t workflow -o diagram.json
& "$SKILL_ROOT\archify-rs.exe" render -t workflow -i diagram.json -o output.html
```

- `flowchart` / `graph` → `workflow`, or `architecture` for a component map.
- `sequenceDiagram` → `sequence`; participants become semantic participants and arrows become messages.

## Repository analysis

Analyze a code repository to generate an architecture diagram automatically:

```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" analyze --path ./my-project -o architecture.json
& "$SKILL_ROOT\archify-rs.exe" render -t architecture -i architecture.json -o architecture.html
```

Supported languages: Python, Rust, TypeScript, Go, Java.

## Architecture comparison

Compare two architecture snapshots to visualize what changed:

```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" compare -a base.architecture.json -b head.architecture.json -o delta.html
```

The delta page shows:
- Added, removed, and modified components/connections/boundaries
- Before/Delta/After views with interactive review
- Per-entity change markers and classifications

## Bundled reference material

| Path | Purpose |
|---|---|
| `schemas/*.schema.json` | Official Archify JSON Schemas (draft 2020-12) — validation contract |
| `examples/*.json` | One validated example per diagram type, plus an `archify-repo` analyzer sample |
| `assets/template.html` | The unmodified Archify viewer template embedded into every output |

Use these for field shapes and vocabulary only. Never copy example facts (names, IDs, domain wording) into new diagrams.

## Authoring invariants

- One obvious main path; side branches leave the nearest main-path node. Remove low-value edges before adding routing controls.
- Omit `meta.legend` for the truthful `auto` default. When needed, use only `mode: auto|all|hidden` and renderer-supported `entries.<kind>.label|visible`; labels never change semantics.
- Component types are `frontend`, `backend`, `database`, `cloud`, `security`, `messagebus`, and `external`; variants are `default`, `emphasis`, `security`, and `dashed`.
- Spacing means clear gap, not center distance. For a relationship label, clear gap must exceed its measured mask width; otherwise omit the label or move it deliberately.
- Automatic routes own their endpoint sides. A side is a direction contract: the first and final segment must leave/enter perpendicular to that side.

## Delivery

Use `validate` during repair and `render` once for final output. The rendered HTML is self-contained with all CSS/JS inline.

Optional: add `--theme dark` or `--theme light` to set the default theme (defaults to dark).

```powershell
$SKILL_ROOT = "$HOME\.RustAgent\workspace\skills\archify-rs"
& "$SKILL_ROOT\archify-rs.exe" render -t architecture -i input.json -o output.html --theme dark
```

## Optional viewer capabilities

Generated HTML already contains theme switching, pan/zoom, search, focus, relationship tracing, semantic views, presentation, and truthful exports. These are reader capabilities, not extra authoring work. `meta.animation: "trace"` is opt-in; `meta.views` is optional and should contain at most five curated chapters.

## Output

Return the rendered HTML path, diagram type, and validation status. Do not claim success for a non-zero command.

All outputs should be written to the workspace output directory for consistency.

## Differences from Node.js version

- **No Node.js required** — single binary, zero dependencies
- **Faster startup** — milliseconds vs seconds
- **Smaller footprint** — 6.6MB binary (3.1MB minimal) vs full Node.js runtime
- **Same output** — pixel-identical HTML using the same template
- **Same schemas** — 100% compatible with official Archify JSON IR
- **Architecture delta** — compare two snapshots and visualize changes
