use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct ExamplesCmd {
    /// Build in release mode
    #[arg(short, long)]
    pub release: bool,

    /// Package to build examples for (if not specified, builds all)
    #[arg(short, long)]
    pub package: Option<String>,
}

impl ExamplesCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Building examples...\n");

        let mut args = vec!["build", "--examples"];

        // Package filter
        if let Some(package) = &self.package {
            args.push("-p");
            args.push(package);
        } else {
            args.push("--workspace");
        }

        // Release mode
        if self.release {
            args.push("--release");
        }

        tracing::info!("Command: cargo {}", args.join(" "));

        process::run_command("cargo", &args).await?;

        tracing::info!("\n✓ All examples built successfully");
        tracing::info!("  This ensures that all examples compile correctly");
        tracing::info!("  and stay up-to-date with API changes");

        Ok(())
    }
}
