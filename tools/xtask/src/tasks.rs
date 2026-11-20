//! The task `checks`, which runs every check even after one fails and then
//! reports all that failed. `--dry-run` prints what it would run without doing
//! it.

mod checks;
mod exec;

use std::process::ExitCode;

use exec::Runner;

/// Options every task takes.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct RunOpts {
    /// Print the commands instead of running them.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

impl RunOpts {
    fn runner(self) -> Runner {
        Runner {
            dry_run: self.dry_run,
        }
    }
}

/// Arguments for `cargo xtask checks`.
#[derive(Debug, clap::Args)]
pub(crate) struct ChecksArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask checks`: the generated files against their sources and the
/// changelog fragments, in this process. All of them run; exit 1 if any failed.
pub(crate) fn checks(args: &ChecksArgs) -> anyhow::Result<ExitCode> {
    checks::run(args.run.runner()).map(|()| ExitCode::SUCCESS)
}
