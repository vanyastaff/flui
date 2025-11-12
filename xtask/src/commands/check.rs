use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct CheckCmd {
    /// Skip formatting check
    #[arg(long)]
    pub skip_fmt: bool,

    /// Skip clippy check
    #[arg(long)]
    pub skip_clippy: bool,
}

impl CheckCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Running workspace checks...\n");

        let mut all_passed = true;

        // 1. Format check
        if !self.skip_fmt {
            tracing::info!("Checking code formatting...");
            match process::run_command("cargo", &["fmt", "--all", "--", "--check"]).await {
                Ok(_) => {
                    tracing::info!("✓ Format check passed\n");
                }
                Err(e) => {
                    tracing::error!("✗ Format check failed: {}\n", e);
                    tracing::info!("  Run 'cargo xtask fmt' to fix formatting\n");
                    all_passed = false;
                }
            }
        }

        // 2. Clippy check
        if !self.skip_clippy {
            tracing::info!("Running clippy...");
            match process::run_command(
                "cargo",
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--",
                    "-D",
                    "warnings",
                ],
            )
            .await
            {
                Ok(_) => {
                    tracing::info!("✓ Clippy passed\n");
                }
                Err(e) => {
                    tracing::error!("✗ Clippy failed: {}\n", e);
                    all_passed = false;
                }
            }
        }

        // 3. Cargo check
        tracing::info!("Running cargo check...");
        match process::run_command("cargo", &["check", "--workspace", "--all-targets"]).await {
            Ok(_) => {
                tracing::info!("✓ Cargo check passed\n");
            }
            Err(e) => {
                tracing::error!("✗ Cargo check failed: {}\n", e);
                all_passed = false;
            }
        }

        if all_passed {
            tracing::info!("✓ All checks passed!");
            Ok(())
        } else {
            anyhow::bail!("Some checks failed")
        }
    }
}
