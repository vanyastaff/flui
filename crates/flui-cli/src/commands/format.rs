//! Format command for code formatting.
//!
//! Wraps `cargo fmt` with check mode support.

use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use console::style;

/// Execute the format command.
///
/// # Arguments
///
/// * `check` - If true, only check formatting without modifying files
///
/// # Errors
///
/// Returns `CliError::FormattingCheckFailed` if check mode finds unformatted code.
/// Returns `CliError::FormattingFailed` if formatting fails.
pub fn execute(check: bool) -> CliResult<()> {
    cliclack::intro(style(" flui format ").on_magenta().black())?;

    let mode = if check { "check" } else { "format" };
    cliclack::log::info(format!("Mode: {}", style(mode).cyan()))?;

    let mut cmd = CargoCommand::fmt().all();

    if check {
        cmd = cmd.check();
    }

    cmd.output_style(OutputStyle::Streaming).run()?;

    if check {
        cliclack::outro(style("Code is properly formatted").green())?;
    } else {
        cliclack::outro(style("Code formatted successfully").green())?;
    }

    Ok(())
}
