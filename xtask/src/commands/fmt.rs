use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct FmtCmd {
    /// Check formatting without modifying files
    #[arg(long)]
    pub check: bool,
}

impl FmtCmd {
    pub async fn run(&self) -> Result<()> {
        let mut args = vec!["fmt", "--all"];

        if self.check {
            tracing::info!("Checking code formatting...\n");
            args.push("--");
            args.push("--check");
        } else {
            tracing::info!("Formatting code...\n");
        }

        process::run_command("cargo", &args).await?;

        if self.check {
            tracing::info!("✓ Code is properly formatted");
        } else {
            tracing::info!("✓ Code formatted successfully");
        }

        Ok(())
    }
}
