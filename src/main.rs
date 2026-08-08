//! Archify-RS — a zero-dependency Rust reimplementation of the Archify CLI.
//!
//! The front-end template (assets/template.html) and JSON Schemas
//! (schemas/*.schema.json) are embedded verbatim from the official Archify
//! project and are never rewritten: this binary only injects rendered SVG and
//! authored metadata into the template's sentinel slots.

#[cfg(feature = "analyzer")]
mod analyzer;
mod cli;
mod converter;
mod delta;
mod renderer;
mod template;
mod validator;

use anyhow::Result;
use clap::Parser;
use log::info;

use crate::cli::{Cli, Command};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match &cli.command {
        Command::Validate(args) => {
            let report = validator::validate_file(&args.input, args.type_name.as_str())?;
            if cli.verbose {
                info!("validated {} ({} bytes)", args.input.display(), report.bytes);
            }
            println!("✓ valid {} diagram: {}", args.type_name, args.input.display());
            Ok(())
        }
        Command::Render(args) => {
            let output = renderer::render_file(
                &args.input,
                &args.output,
                args.type_name.as_str(),
                args.theme.as_deref(),
            )?;
            if cli.verbose {
                info!("rendered {} -> {}", args.input.display(), output.display());
            }
            println!("{}", output.display());
            Ok(())
        }
        Command::Convert(args) => {
            let ir = converter::convert_file(&args.file, &args.type_name)?;
            let json = serde_json::to_string_pretty(&ir)?;
            std::fs::write(&args.output, json)?;
            if cli.verbose {
                info!("converted {} -> {}", args.file.display(), args.output.display());
            }
            println!("{}", args.output.display());
            Ok(())
        }
        #[cfg(feature = "analyzer")]
        Command::Analyze(args) => {
            let ir = analyzer::analyze_repo(&args.path, args.lang.as_deref())?;
            let json = serde_json::to_string_pretty(&ir)?;
            std::fs::write(&args.output, json)?;
            if cli.verbose {
                info!(
                    "analyzed {} ({} components) -> {}",
                    args.path.display(),
                    ir["components"].as_array().map(|a| a.len()).unwrap_or(0),
                    args.output.display()
                );
            }
            println!("{}", args.output.display());
            Ok(())
        }
        Command::Compare(args) => {
            let receipt = delta::compare_files(&args.base, &args.head, &args.output)?;
            if cli.verbose {
                info!(
                    "compared {} vs {} -> {} ({} changes)",
                    args.base.display(),
                    args.head.display(),
                    args.output.display(),
                    receipt
                        .pointer("/changes/components")
                        .and_then(|c| c.as_array())
                        .map(|c| c.len())
                        .unwrap_or(0)
                );
            }
            println!("{}", args.output.display());
            Ok(())
        }
    }
}
