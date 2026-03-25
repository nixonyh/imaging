// Copyright 2026 the Imaging Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Developer commands for image snapshot review.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use kompari::{DirDiffConfig, SizeOptimizationLevel};
use kompari_html::{ReportConfig, render_html_report, start_review_server};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Backend {
    Skia,
    Vello,
    #[value(name = "vello_cpu", alias = "vello-cpu")]
    VelloCpu,
    #[value(name = "vello_hybrid", alias = "vello-hybrid")]
    VelloHybrid,
}

impl Backend {
    fn feature(self) -> &'static str {
        match self {
            Self::Skia => "skia",
            Self::Vello => "vello",
            Self::VelloCpu => "vello_cpu",
            Self::VelloHybrid => "vello_hybrid",
        }
    }

    fn test_name(self) -> &'static str {
        match self {
            Self::Skia => "skia_snapshots",
            Self::Vello => "vello_snapshots",
            Self::VelloCpu => "vello_cpu_snapshots",
            Self::VelloHybrid => "vello_hybrid_snapshots",
        }
    }

    fn snapshot_dir_name(self) -> &'static str {
        self.feature()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OptimizeSize {
    None,
    Fast,
    High,
}

impl OptimizeSize {
    fn to_level(self) -> SizeOptimizationLevel {
        match self {
            Self::None => SizeOptimizationLevel::None,
            Self::Fast => SizeOptimizationLevel::Fast,
            Self::High => SizeOptimizationLevel::High,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "Snapshot diff and review helpers")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate an HTML report comparing blessed vs current images.
    Report(ReportArgs),
    /// Start a local review server for blessed vs current images.
    Review(ReviewArgs),
    /// Regenerate current images for one backend.
    Generate(GenerateArgs),
}

#[derive(Clone, Debug, Parser)]
struct SnapshotSelection {
    /// Snapshot backend to operate on.
    #[arg(long)]
    backend: Backend,

    /// Restrict generation/reporting to images whose filename contains this string.
    #[arg(long)]
    case: Option<String>,
}

#[derive(Debug, Parser)]
struct GenerateArgs {
    #[command(flatten)]
    selection: SnapshotSelection,

    /// Print per-case execution logs from the snapshot runner.
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Debug, Parser)]
struct ReportArgs {
    #[command(flatten)]
    selection: SnapshotSelection,

    /// Regenerate current images before building the report.
    #[arg(long, default_value_t = false)]
    generate: bool,

    /// Output HTML path.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Embed images directly in the HTML report.
    #[arg(long, default_value_t = false)]
    embed_images: bool,

    /// Optimize image sizes in the report.
    #[arg(long, default_value = "none")]
    optimize_size: OptimizeSize,

    /// Print per-case execution logs from the snapshot runner.
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Debug, Parser)]
struct ReviewArgs {
    #[command(flatten)]
    selection: SnapshotSelection,

    /// Regenerate current images before starting the review server.
    #[arg(long, default_value_t = false)]
    generate: bool,

    /// Port for the review server.
    #[arg(long, default_value_t = 7200)]
    port: u16,

    /// Optimize image sizes in generated HTML.
    #[arg(long, default_value = "none")]
    optimize_size: OptimizeSize,

    /// Print per-case execution logs from the snapshot runner.
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::Generate(args) => generate_current_images(&args.selection, args.verbose),
        Commands::Report(args) => {
            if args.generate {
                generate_current_images(&args.selection, args.verbose)?;
            }
            write_report(
                &args.selection,
                args.output,
                args.embed_images,
                args.optimize_size,
            )
        }
        Commands::Review(args) => {
            if args.generate {
                generate_current_images(&args.selection, args.verbose)?;
            }
            start_review(&args.selection, args.port, args.optimize_size)
        }
    }
}

fn generate_current_images(selection: &SnapshotSelection, verbose: bool) -> Result<()> {
    let workspace_root = workspace_root();
    let mut command = Command::new("cargo");
    command
        .current_dir(&workspace_root)
        .arg("test")
        .arg("-p")
        .arg("imaging_snapshot_tests")
        .arg("--features")
        .arg(selection.backend.feature())
        .arg("--test")
        .arg(selection.backend.test_name())
        .arg("--")
        .arg("--nocapture")
        .env("IMAGING_TEST", "generate-all");

    if verbose {
        command.env("IMAGING_TEST_VERBOSE", "1");
    }
    if let Some(case) = selection.case.as_deref() {
        command.env("IMAGING_CASE", case);
    }

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(
            std::io::Error::other(format!("snapshot generation failed with status {status}"))
                .into(),
        )
    }
}

fn write_report(
    selection: &SnapshotSelection,
    output: Option<PathBuf>,
    embed_images: bool,
    optimize_size: OptimizeSize,
) -> Result<()> {
    let diff = create_diff_config(selection).create_diff()?;
    let mut report_config = default_report_config();
    report_config.set_embed_images(embed_images);
    report_config.set_size_optimization(optimize_size.to_level());

    let output = output.unwrap_or_else(|| {
        workspace_root()
            .join("imaging_snapshot_tests")
            .join("tests")
            .join(format!(
                "{}_report.html",
                selection.backend.snapshot_dir_name()
            ))
    });
    let report = render_html_report(&report_config, diff.results())?;
    std::fs::write(&output, report)?;
    println!("Report written to {}", output.display());
    Ok(())
}

fn start_review(
    selection: &SnapshotSelection,
    port: u16,
    optimize_size: OptimizeSize,
) -> Result<()> {
    let diff_config = create_diff_config(selection);
    let mut report_config = default_report_config();
    report_config.set_size_optimization(optimize_size.to_level());
    start_review_server(&diff_config, &report_config, port)?;
    Ok(())
}

fn create_diff_config(selection: &SnapshotSelection) -> DirDiffConfig {
    let backend = selection.backend.snapshot_dir_name();
    let tests_dir = workspace_root()
        .join("imaging_snapshot_tests")
        .join("tests");
    let mut config = DirDiffConfig::new(
        tests_dir.join("snapshots").join(backend),
        tests_dir.join("current").join(backend),
    );
    if let Some(case) = selection.case.as_ref() {
        config.set_filter_name(Some(case.clone()));
    }
    config
}

fn default_report_config() -> ReportConfig {
    let mut report_config = ReportConfig::default();
    report_config.set_left_title("Reference");
    report_config.set_right_title("Current");
    report_config
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live at the workspace root")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::Backend;

    #[test]
    fn backend_metadata_matches_snapshot_layout() {
        assert_eq!(Backend::Skia.feature(), "skia");
        assert_eq!(Backend::Vello.test_name(), "vello_snapshots");
        assert_eq!(Backend::VelloCpu.snapshot_dir_name(), "vello_cpu");
        assert_eq!(Backend::VelloHybrid.snapshot_dir_name(), "vello_hybrid");
    }
}
