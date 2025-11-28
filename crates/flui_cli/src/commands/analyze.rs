//! Analyze command for code linting.
//!
//! Wraps `cargo clippy` with workspace support, pedantic mode,
//! and auto-fix capabilities.

use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use console::style;

/// Execute the analyze command.
///
/// # Arguments
///
/// * `fix` - Automatically fix issues where possible
/// * `pedantic` - Enable pedantic lints
///
/// # Errors
///
/// Returns `CliError::AnalysisFailed` if clippy finds issues.
pub fn execute(fix: bool, pedantic: bool) -> CliResult<()> {
    cliclack::intro(style(" flui analyze ").on_blue().black())?;

    let mut cmd = CargoCommand::clippy().workspace().deny_warnings();

    if pedantic {
        cmd = cmd.pedantic();
        cliclack::log::info(format!("Pedantic mode: {}", style("enabled").cyan()))?;
    }

    if fix {
        cmd = cmd.fix();
        cliclack::log::info("Auto-fixing issues...")?;
    }

    cmd.output_style(OutputStyle::Streaming).run()?;

    cliclack::outro(style("Analysis complete - no issues found").green())?;

    Ok(())
}
