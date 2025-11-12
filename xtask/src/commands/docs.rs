use anyhow::Result;
use clap::Args;
use crate::util::process::run_command;

#[derive(Args, Debug)]
pub struct DocsCmd {
    /// Open documentation in browser
    #[arg(long)]
    open: bool,

    /// Include private items in documentation
    #[arg(long)]
    document_private_items: bool,

    /// Generate documentation for all workspace members
    #[arg(long)]
    workspace: bool,
}

impl DocsCmd {
    pub async fn run(self) -> Result<()> {
        tracing::info!("Generating documentation...");

        let mut args = vec!["doc", "--no-deps"];

        if self.open {
            args.push("--open");
        }

        if self.document_private_items {
            args.push("--document-private-items");
        }

        if self.workspace {
            args.push("--workspace");
        }

        run_command("cargo", &args).await?;

        tracing::info!("✓ Documentation generated successfully");
        Ok(())
    }
}
