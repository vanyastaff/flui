use anyhow::Result;
use clap::Args;
use crate::commands::{CheckCmd, TestCmd, BenchCmd};

#[derive(Args, Debug)]
pub struct CiCmd {
    /// Skip running benchmarks
    #[arg(long)]
    skip_bench: bool,
}

impl CiCmd {
    pub async fn run(self) -> Result<()> {
        tracing::info!("Running CI checks...");
        tracing::info!("═══════════════════════════════════════════════════");

        // 1. Code quality checks (fmt + clippy + check)
        tracing::info!("Step 1/3: Running code quality checks...");
        CheckCmd {
            skip_fmt: false,
            skip_clippy: false,
        }.run().await?;

        // 2. Run tests
        tracing::info!("Step 2/3: Running tests...");
        TestCmd {
            package: None,
            filter: None,
            all_features: false,
            no_default_features: false,
            lib: false,
            doc: false,
            bench: false,
        }.run().await?;

        // 3. Run benchmarks (optional)
        if !self.skip_bench {
            tracing::info!("Step 3/3: Running benchmarks...");
            BenchCmd {
                package: None,
                filter: None,
            }.run().await?;
        } else {
            tracing::info!("Step 3/3: Skipping benchmarks (--skip-bench)");
        }

        tracing::info!("═══════════════════════════════════════════════════");
        tracing::info!("✓ All CI checks passed!");
        Ok(())
    }
}
