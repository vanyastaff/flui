//! Test command for running project tests.
//!
//! Wraps `cargo test` with additional options for filtering
//! and selecting test types.

use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use console::style;

/// Execute the test command.
///
/// # Arguments
///
/// * `filter` - Optional test name filter
/// * `unit` - Run only unit tests (--lib)
/// * `integration` - Run only integration tests (--test)
/// * `_platform` - Platform filter (not yet implemented)
///
/// # Errors
///
/// Returns `CliError::TestsFailed` if any tests fail.
#[expect(
    clippy::needless_pass_by_value,
    reason = "mirrors clap argument structure"
)]
pub fn execute(
    filter: Option<String>,
    unit: bool,
    integration: bool,
    _platform: Option<String>,
) -> CliResult<()> {
    cliclack::intro(style(" flui test ").on_yellow().black())?;

    let mut cmd = CargoCommand::test();

    // Add filter if provided
    if let Some(ref f) = filter {
        cmd = cmd.filter(f);
        cliclack::log::info(format!("Filter: {}", style(f).cyan()))?;
    }

    // Add test type flags
    if unit {
        cmd = cmd.lib_only();
        cliclack::log::info("Running unit tests only")?;
    } else if integration {
        cmd = cmd.integration_only();
        cliclack::log::info("Running integration tests only")?;
    }

    let _ = cmd.output_style(OutputStyle::Streaming).run()?;

    cliclack::outro(style("All tests passed").green())?;

    Ok(())
}
