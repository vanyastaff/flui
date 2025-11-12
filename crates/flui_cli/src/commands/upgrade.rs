use anyhow::{Context, Result};
use console::style;
use std::process::Command;

pub fn execute(self_update: bool, dependencies: bool) -> Result<()> {
    if self_update || !dependencies {
        println!("{}", style("Upgrading flui_cli...").green().bold());
        println!();

        let status = Command::new("cargo")
            .args(["install", "flui_cli", "--force"])
            .status()
            .context("Failed to upgrade flui_cli")?;

        if !status.success() {
            anyhow::bail!("Upgrade failed");
        }

        println!();
        println!("{}", style("✓ flui_cli upgraded").green().bold());
    }

    if dependencies || !self_update {
        println!("{}", style("Updating project dependencies...").green().bold());
        println!();

        let status = Command::new("cargo")
            .arg("update")
            .status()
            .context("Failed to update dependencies")?;

        if !status.success() {
            anyhow::bail!("Update failed");
        }

        println!();
        println!("{}", style("✓ Dependencies updated").green().bold());
    }

    Ok(())
}
