use anyhow::Result;
use clap::Args;

use crate::util::process;

#[derive(Args)]
pub struct TestCmd {
    /// Package to test (if not specified, tests all packages)
    #[arg(short, long)]
    pub package: Option<String>,

    /// Test filter (substring of test name)
    pub filter: Option<String>,

    /// Run tests with all features enabled
    #[arg(long)]
    pub all_features: bool,

    /// Run tests with no default features
    #[arg(long)]
    pub no_default_features: bool,

    /// Run only library unit tests
    #[arg(long)]
    pub lib: bool,

    /// Run only documentation tests
    #[arg(long)]
    pub doc: bool,

    /// Run benchmarks
    #[arg(long)]
    pub bench: bool,
}

impl TestCmd {
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Running tests...\n");

        let mut args = vec!["test"];

        // Package filter
        if let Some(package) = &self.package {
            args.push("-p");
            args.push(package);
        } else {
            args.push("--workspace");
        }

        // Feature flags
        if self.all_features {
            args.push("--all-features");
        } else if self.no_default_features {
            args.push("--no-default-features");
        }

        // Test type
        if self.lib {
            args.push("--lib");
        } else if self.doc {
            args.push("--doc");
        } else if self.bench {
            args.push("--benches");
        }

        // Test filter
        if let Some(filter) = &self.filter {
            args.push("--");
            args.push(filter);
        }

        tracing::info!("Command: cargo {}", args.join(" "));

        process::run_command("cargo", &args).await?;

        tracing::info!("\n✓ Tests passed!");
        Ok(())
    }
}
