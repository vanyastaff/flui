use anyhow::{Context, Result};
use console::style;
use std::process::Command;

pub fn execute(
    filter: Option<String>,
    unit: bool,
    integration: bool,
    _platform: Option<String>,
) -> Result<()> {
    println!("{}", style("Running tests...").green().bold());
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("test");

    // Add filter if provided
    if let Some(filter) = filter {
        cmd.arg(&filter);
        println!("  {} Filter: {}", style("→").cyan(), style(&filter).cyan());
    }

    // Add test type flags
    if unit {
        cmd.arg("--lib");
        println!("  {} Running unit tests only", style("→").cyan());
    } else if integration {
        cmd.arg("--test");
        println!("  {} Running integration tests only", style("→").cyan());
    }

    println!();

    let status = cmd.status().context("Failed to run cargo test")?;

    if !status.success() {
        anyhow::bail!("Tests failed");
    }

    println!();
    println!("{}", style("✓ All tests passed").green().bold());

    Ok(())
}
