//! The tasks `checks`, `fuzz` and `clean-nested`.
//!
//! `checks` and `fuzz` run every step even after one fails and then report all
//! that failed; `fuzz` prints each command before it runs it. `--dry-run`
//! prints what a task would run or remove without doing it.

mod checks;
mod exec;
mod fuzz;

use std::process::ExitCode;

use exec::{Runner, target_dir};
pub(crate) use fuzz::{FuzzArgs, fuzz};

use crate::util::{human_size, tree_size};

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

/// The nested-cargo tests' build caches, relative to the target directory:
/// separate target roots that `cargo sweep` cannot see.
const NESTED_CACHES: [&[&str]; 3] = [
    &["cli-template-check"],
    &["facade-consumer-check"],
    &["tests", "trybuild"],
];

/// `Ok(ExitCode::SUCCESS)` once `stage` succeeded.
fn done(stage: anyhow::Result<()>) -> anyhow::Result<ExitCode> {
    stage.map(|()| ExitCode::SUCCESS)
}
/// Arguments for `cargo xtask checks`.
#[derive(Debug, clap::Args)]
pub(crate) struct ChecksArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask checks`: the repository's own source checks, which no
/// standard tool makes.
///
/// `toolchain`, `unmounted-tests`, `adr-numbers`, `changelog --check` and the
/// generated files against their sources (`font-assets --package-list` and
/// the rest), in this process. All of them run; exit 1 if any failed.
pub(crate) fn checks(args: &ChecksArgs) -> anyhow::Result<ExitCode> {
    done(checks::run(args.run.runner()))
}

/// Arguments for `cargo xtask clean-nested`.
#[derive(Debug, clap::Args)]
pub(crate) struct CleanNestedArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask clean-nested`: remove the nested-cargo test caches under the target directory.
///
/// `cli-template-check/`, `facade-consumer-check/` and `tests/trybuild/`; the
/// next nextest run that needs them rebuilds them (~9 min cold). A test run using
/// them must not overlap it: run as `cargo heli xtask clean-nested`, which the
/// repository's `.config/heli.toml` makes wait for every build and test heli
/// runs in this checkout. Run directly, it says so before deleting anything.
pub(crate) fn clean_nested(args: &CleanNestedArgs) -> anyhow::Result<ExitCode> {
    let target = target_dir()?;
    if !args.run.dry_run && std::env::var_os("HELI_RUN").is_none_or(|run| run.is_empty()) {
        println!(
            "clean-nested: not run through heli, so nothing waits for a test using these caches; \
             `cargo heli xtask clean-nested` waits for this checkout's builds first"
        );
    }
    let mut found = 0;
    for rel in NESTED_CACHES {
        let mut dir = target.clone();
        dir.extend(rel);
        if !dir.is_dir() {
            continue;
        }
        found += 1;
        let size = human_size(tree_size(&dir));
        if args.run.dry_run {
            println!("would remove {size} {}", dir.display());
            continue;
        }
        println!("removing {size} {}", dir.display());
        std::fs::remove_dir_all(&dir)
            .map_err(|error| anyhow::anyhow!("removing {}: {error}", dir.display()))?;
    }
    if found == 0 {
        println!(
            "clean-nested: no nested-cargo cache under {}",
            target.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}
