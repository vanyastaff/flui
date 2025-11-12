use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct BenchCmd {
    /// Package to benchmark (if not specified, runs all benchmarks)
    #[arg(short, long)]
    pub package: Option<String>,

    /// Benchmark name filter
    pub filter: Option<String>,
}

impl BenchCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Running benchmarks...\n");

        let mut args = vec!["bench"];

        // Package filter
        if let Some(package) = &self.package {
            args.push("-p");
            args.push(package);
        } else {
            // Run benchmarks for known packages with benches
            tracing::info!("Running benchmarks for flui_core and flui_types...\n");

            // Run flui_core benchmarks
            tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            tracing::info!("flui_core benchmarks");
            tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            let mut core_args = vec!["bench", "-p", "flui_core"];
            if let Some(filter) = &self.filter {
                core_args.push(filter);
            }

            process::run_command("cargo", &core_args).await?;

            // Run flui_types benchmarks
            tracing::info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            tracing::info!("flui_types benchmarks");
            tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            let mut types_args = vec!["bench", "-p", "flui_types"];
            if let Some(filter) = &self.filter {
                types_args.push(filter);
            }

            process::run_command("cargo", &types_args).await?;

            tracing::info!("\n✓ All benchmarks completed");
            return Ok(());
        }

        // Filter
        if let Some(filter) = &self.filter {
            args.push(filter);
        }

        process::run_command("cargo", &args).await?;

        tracing::info!("\n✓ Benchmarks completed");
        Ok(())
    }
}
