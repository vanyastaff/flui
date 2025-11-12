use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;
mod util;

use commands::{
    BenchCmd, CheckCmd, FmtCmd, LintCmd, TestCmd, ValidateCmd, ExamplesCmd, DocsCmd, CiCmd,
};

#[derive(Parser)]
#[command(name = "FLUI Dev Tasks")]
#[command(about = "Development tasks for FLUI project (format, lint, test, CI)", version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(global = true, long, short = 'v')]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Format code with rustfmt
    Fmt(FmtCmd),

    /// Run clippy linter
    Lint(LintCmd),

    /// Check code quality (fmt + clippy + check)
    Check(CheckCmd),

    /// Run tests
    Test(TestCmd),

    /// Run pre-commit validation (check + test)
    Validate(ValidateCmd),

    /// Run benchmarks
    Bench(BenchCmd),

    /// Build all examples
    Examples(ExamplesCmd),

    /// Generate documentation
    Docs(DocsCmd),

    /// Run CI checks (check + test + bench)
    Ci(CiCmd),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_line_number(true)
        .init();

    tracing::info!("FLUI Dev Tasks starting...");

    match cli.command {
        Commands::Fmt(cmd) => cmd.run().await,
        Commands::Lint(cmd) => cmd.run().await,
        Commands::Check(cmd) => cmd.run().await,
        Commands::Test(cmd) => cmd.run().await,
        Commands::Validate(cmd) => cmd.run().await,
        Commands::Bench(cmd) => cmd.run().await,
        Commands::Examples(cmd) => cmd.run().await,
        Commands::Docs(cmd) => cmd.run().await,
        Commands::Ci(cmd) => cmd.run().await,
    }
}
