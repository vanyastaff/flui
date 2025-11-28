use anyhow::{Context, Result};
use console::style;
use std::process::Command;

pub fn execute(check: bool) -> Result<()> {
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
            anyhow::bail!("Code is not formatted. Run 'flui format' to fix.");
        }
        anyhow::bail!("Formatting failed");
    }

    println!();
    if check {
        println!("{}", style("✓ Code is properly formatted").green().bold());
    } else {
        println!("{}", style("✓ Code formatted successfully").green().bold());
    }

    Ok(())
}
