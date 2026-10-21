//! dokt: turn the Markdown files in a repository into a documentation site.
//!
//! The pipeline, one module per stage:
//!
//! * [`config`]   reads `dokt.toml`.
//! * [`crawl`]    walks the project and finds pages and images, pruning excluded folders.
//! * [`markdown`] renders Markdown to HTML and resolves `[[wikilinks]]`, images and Mermaid blocks.
//! * [`todo`](mod@todo) finds and highlights TODOs.
//! * [`theme`]    holds the color palette and generates `theme.css`.
//! * [`site`]     writes the static site.
//! * [`serve`]    serves it locally and rebuilds when files change.
//! * [`export`]   builds the one-document PDF; [`chrome`] prints it through a headless browser.

mod chrome;
mod config;
mod crawl;
mod export;
mod markdown;
mod serve;
mod site;
mod theme;
mod todo;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dokt",
    version,
    about = "Build, serve or export a docs site from the Markdown files in a repo"
)]
/// Command-line interface: a global `--config` option and one subcommand.
struct Cli {
    /// Path to the config file (default: search upwards for `dokt.toml`)
    #[arg(long, short, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

/// The subcommands.
#[derive(Subcommand)]
enum Cmd {
    /// Write a starter dokt.toml in the current directory
    Init {
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },
    /// Generate the static site (the same as `export --web`)
    Build {
        /// Output directory (overrides out_dir from the config)
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Exit with an error if there are warnings (broken links, ...)
        #[arg(long)]
        strict: bool,
    },
    /// Build and serve locally
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, short, default_value_t = 3000)]
        port: u16,
        /// Output directory (overrides out_dir from the config)
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Export as a static site (--web), a single PDF (--pdf), or both
    ///
    /// The PDF has a cover page, a table of contents in sidebar order, every page,
    /// and an appendix of TODOs. Requires Chrome, Chromium, or Edge to be installed.
    Export {
        /// Write the static site. Optionally give the directory (default: out_dir)
        #[arg(long, num_args = 0..=1, value_name = "DIR")]
        web: Option<Option<PathBuf>>,
        /// Write a PDF. Optionally give the file (default: out_dir, named after the title)
        #[arg(long, num_args = 0..=1, value_name = "FILE")]
        pdf: Option<Option<PathBuf>>,
        /// Paper size for the PDF (default: [export] paper, else a4)
        #[arg(long, value_enum)]
        paper: Option<config::Paper>,
        /// Chrome/Chromium/Edge executable to print with (default: found automatically)
        #[arg(long, value_name = "PATH")]
        browser: Option<PathBuf>,
        /// Run the browser without its sandbox. Needed in containers and CI, or as root
        #[arg(long)]
        no_sandbox: bool,
        /// Exit with an error if there are warnings (broken links, ...)
        #[arg(long)]
        strict: bool,
    },
}

/// Run the command and turn any error into a message and a non-zero exit code.
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Parse the command line and dispatch to the right stage.
fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Init { force } => {
            let path = std::env::current_dir()?.join(config::CONFIG_NAME);
            if path.exists() && !force {
                bail!("{} already exists (use --force to overwrite)", path.display());
            }
            std::fs::write(&path, config::example_config())?;
            println!("Wrote {}", path.display());
        }
        Cmd::Build { out, strict } => {
            let loaded = config::load(cli.config.as_deref())?;
            let out_dir = site::out_dir_for(&loaded.root, &loaded.config, out.as_deref());
            let crawled = crawl::crawl(&loaded.root, &loaded.config, &out_dir)?;
            let report = site::build(&loaded.root, &loaded.config, &out_dir, &crawled)?;
            site::print_report(&report, &out_dir);
            if strict && !report.warnings.is_empty() {
                bail!("{} warning(s) in strict mode", report.warnings.len());
            }
        }
        Cmd::Serve { host, port, out } => {
            let loaded = config::load(cli.config.as_deref())?;
            serve::serve(loaded, out, &host, port)?;
        }
        Cmd::Export { web, pdf, paper, browser, no_sandbox, strict } => {
            let loaded = config::load(cli.config.as_deref())?;
            export::run(loaded, export::ExportArgs { web, pdf, paper, browser, no_sandbox, strict })?;
        }
    }
    Ok(())
}
