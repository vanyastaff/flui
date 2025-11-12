use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct LintCmd {
    /// Fix clippy warnings automatically
    #[arg(long)]
    pub fix: bool,

    /// Allow warnings (don't use -D warnings)
    #[arg(long)]
    pub allow_warnings: bool,
}

impl LintCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Running clippy...\n");

        let mut args = vec!["clippy", "--workspace", "--all-targets", "--all-features"];

        if self.fix {
            args.push("--fix");
            args.push("--allow-dirty");
            args.push("--allow-staged");
        }

        args.push("--");

        if !self.allow_warnings {
            args.push("-D");
            args.push("warnings");
        }

        process::run_command("cargo", &args).await?;

        if self.fix {
            tracing::info!("\n✓ Clippy fixes applied");
        } else {
            tracing::info!("\n✓ Clippy passed with no warnings");
        }

        Ok(())
    }
}
