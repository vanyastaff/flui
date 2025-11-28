use crate::error::{CliError, CliResult, ResultExt};
use console::style;
use std::process::Command;

pub fn execute(fix: bool, pedantic: bool) -> CliResult<()> {
    println!("{}", style("Analyzing code...").green().bold());
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("clippy");
    cmd.arg("--workspace");
    cmd.arg("--");
    cmd.arg("-D").arg("warnings");

    if pedantic {
        cmd.arg("-W").arg("clippy::pedantic");
        println!("  {} Pedantic mode enabled", style("→").cyan());
    }

    if fix {
        println!("  {} Auto-fixing issues...", style("→").cyan());
        cmd.arg("--fix");
    }

    println!();

    let status = cmd.status().context("Failed to run cargo clippy")?;

    if !status.success() {
        return Err(CliError::AnalysisIssues);
    }

    println!();
    println!("{}", style("✓ No issues found").green().bold());

    Ok(())
}
