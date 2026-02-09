//! Upgrade command for updating flui_cli and dependencies.
//!
//! Handles both self-update (flui_cli) and project dependency updates.

use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use console::style;

/// Execute the upgrade command.
///
/// # Arguments
///
/// * `self_update` - Update only flui_cli itself
/// * `dependencies` - Update only project dependencies
///
/// # Errors
///
/// Returns `CliError::UpgradeFailed` if flui_cli upgrade fails.
/// Returns `CliError::UpdateFailed` if dependency update fails.
pub fn execute(self_update: bool, dependencies: bool) -> CliResult<()> {
    cliclack::intro(style(" flui upgrade ").on_blue().white())?;

    // If neither flag is set, do both
    let do_self = self_update || !dependencies;
    let do_deps = dependencies || !self_update;

    if do_self {
        let spinner = cliclack::spinner();
        spinner.start("Upgrading flui_cli...");

        CargoCommand::install("flui_cli")
            .force()
            .output_style(OutputStyle::Silent)
            .run()?;

        spinner.stop(format!("{} flui_cli upgraded", style("✓").green()));
    }

    if do_deps {
        let spinner = cliclack::spinner();
        spinner.start("Updating project dependencies...");

        CargoCommand::update()
            .output_style(OutputStyle::Silent)
            .run()?;

        spinner.stop(format!("{} Dependencies updated", style("✓").green()));
    }

    cliclack::outro(style("Upgrade complete").green())?;

    Ok(())
}
