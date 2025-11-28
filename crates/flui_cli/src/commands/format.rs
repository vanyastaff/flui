use crate::error::{CliError, CliResult, ResultExt};
use console::style;
use std::process::Command;

pub fn execute(check: bool) -> CliResult<()> {
    if check {
        println!("{}", style("Checking code formatting...").green().bold());
    } else {
        println!("{}", style("Formatting code...").green().bold());
    }
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("fmt");
    cmd.arg("--all");

    if check {
        cmd.arg("--check");
    }

    let status = cmd.status().context("Failed to run cargo fmt")?;

    if !status.success() {
        if check {
            return Err(CliError::FormattingCheck);
        }
        return Err(CliError::FormattingFailed);
    }

    println!();
    if check {
        println!("{}", style("✓ Code is properly formatted").green().bold());
    } else {
        println!("{}", style("✓ Code formatted successfully").green().bold());
    }

    Ok(())
}
