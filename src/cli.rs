//! Command-line interface definitions (clap derive).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Archify-RS: architecture visualization without Node.js.
///
/// Renders interactive HTML architecture diagrams from JSON IR, Mermaid text,
/// or source repositories. The front-end renderer is the unmodified Archify
/// template; this binary only computes geometry and injects it into the
/// template's sentinel slots.
#[derive(Debug, Parser)]
#[command(
    name = "archify-rs",
    version,
    about = "Archify-RS — architecture visualization, no Node.js required",
    long_about = None
)]
pub struct Cli {
    /// Output debug information
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Specify a configuration file (reserved; accepted for CLI parity)
    #[arg(short = 'c', long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate a JSON IR file against the Archify schema
    Validate(ValidateArgs),
    /// Render a JSON IR file into a standalone HTML diagram
    Render(RenderArgs),
    /// Convert a Mermaid text file (flowchart / sequenceDiagram) into JSON IR
    Convert(ConvertArgs),
    /// Analyze a source repository and emit an architecture JSON IR
    Analyze(AnalyzeArgs),
}

#[derive(Debug, clap::Args)]
pub struct ValidateArgs {
    /// Diagram type: architecture, workflow, sequence, dataflow, lifecycle
    #[arg(short = 't', long, value_name = "TYPE")]
    pub type_name: String,

    /// Input JSON IR file
    #[arg(short = 'i', long, value_name = "INPUT.json")]
    pub input: PathBuf,
}

#[derive(Debug, clap::Args)]
pub struct RenderArgs {
    /// Diagram type: architecture, workflow, sequence, dataflow, lifecycle
    #[arg(short = 't', long, value_name = "TYPE")]
    pub type_name: String,

    /// Input JSON IR file
    #[arg(short = 'i', long, value_name = "INPUT.json")]
    pub input: PathBuf,

    /// Output HTML path (default: ./archify-output.html)
    #[arg(short = 'o', long, value_name = "OUTPUT.html", default_value = "archify-output.html")]
    pub output: PathBuf,

    /// Optional default theme override (dark | light) applied to the artifact
    #[arg(long, value_name = "THEME", value_parser = ["dark", "light"])]
    pub theme: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct ConvertArgs {
    /// Mermaid text file (.mmd)
    #[arg(short = 'f', long, value_name = "input.mmd")]
    pub file: PathBuf,

    /// Target diagram type: workflow (flowchart) or sequence (sequenceDiagram)
    #[arg(short = 't', long, value_name = "TYPE", default_value = "workflow")]
    pub type_name: String,

    /// Output JSON IR path
    #[arg(short = 'o', long, value_name = "IR.json", default_value = "ir.json")]
    pub output: PathBuf,
}

#[derive(Debug, clap::Args)]
pub struct AnalyzeArgs {
    /// Source repository root directory
    #[arg(long, value_name = "REPO_DIR")]
    pub path: PathBuf,

    /// Source language (python, rust, typescript, go, java); auto-detected by default
    #[arg(long, value_name = "LANG")]
    pub lang: Option<String>,

    /// Output JSON IR path
    #[arg(short = 'o', long, value_name = "IR.json", default_value = "architecture.json")]
    pub output: PathBuf,
}
