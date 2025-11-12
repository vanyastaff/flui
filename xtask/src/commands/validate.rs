use anyhow::Result;
use clap::Args;

use super::{CheckCmd, TestCmd};

#[derive(Args)]
pub struct ValidateCmd {
    /// Skip running tests (only check code quality)
    #[arg(long)]
    pub skip_tests: bool,
}

impl ValidateCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Running pre-commit validation...\n");
        tracing::info!("This will run: check + test\n");

        // Step 1: Run all checks
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("Step 1/2: Code Quality Checks");
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let check_cmd = CheckCmd {
            skip_fmt: false,
            skip_clippy: false,
        };

        check_cmd.run().await?;

        // Step 2: Run tests (unless skipped)
        if !self.skip_tests {
            tracing::info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            tracing::info!("Step 2/2: Tests");
            tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            let test_cmd = TestCmd {
                package: None,
                filter: None,
                all_features: false,
                no_default_features: false,
                lib: false,
                doc: false,
                bench: false,
            };

            test_cmd.run().await?;
        }

        tracing::info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("✓ Validation completed successfully!");
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        Ok(())
    }
}
