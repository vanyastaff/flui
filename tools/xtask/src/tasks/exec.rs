//! Running a task's steps: this crate's own commands, in this process.
//!
//! [`Runner`] prints each step as the command line it stands for before it
//! runs it, and with `dry_run` only prints it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};

use crate::util::repo_root;

/// Runs a task's steps; with `dry_run` it only prints them.
#[derive(Debug, Clone, Copy)]
pub(super) struct Runner {
    pub(super) dry_run: bool,
}

impl Runner {
    /// Runs one of this crate's own commands in this process, printed as the
    /// command line it stands for; a non-zero exit code is an error.
    pub(super) fn in_process(
        self,
        command: &str,
        check: impl FnOnce() -> anyhow::Result<ExitCode>,
    ) -> anyhow::Result<()> {
        println!("$ cargo xtask {command}");
        if self.dry_run {
            return Ok(());
        }
        if check()? != ExitCode::SUCCESS {
            bail!("`cargo xtask {command}` failed");
        }
        Ok(())
    }
}

/// Parses `argv` as the arguments of one of this crate's commands, so an
/// in-process call passes exactly what its command line would.
pub(super) fn parsed<T: clap::Args + clap::FromArgMatches>(
    argv: impl IntoIterator<Item = impl Into<OsString>>,
) -> anyhow::Result<T> {
    let argv = std::iter::once(OsString::from("xtask")).chain(argv.into_iter().map(Into::into));
    let matches = T::augment_args(clap::Command::new("xtask")).try_get_matches_from(argv)?;
    Ok(T::from_arg_matches(&matches)?)
}

/// The workspace's target directory as cargo resolves it: `CARGO_TARGET_DIR`
/// (relative to the repository root), `build.target-dir`, or `target/`.
pub(super) fn target_dir() -> anyhow::Result<PathBuf> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(repo_root())
        .no_deps()
        .exec()
        .context("running `cargo metadata`")?;
    Ok(metadata.target_directory.into_std_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step whose command exits non-zero is an error naming its command
    /// line, an error the command returns is passed on as it is (the caller
    /// prints the command beside it), and a dry run runs nothing.
    #[test]
    fn a_failing_exit_is_an_error_naming_the_step_and_a_dry_run_runs_nothing() {
        let runner = Runner { dry_run: false };
        let failed = runner
            .in_process("toolchain", || Ok(ExitCode::FAILURE))
            .expect_err("a failing exit code is an error");
        assert_eq!(failed.to_string(), "`cargo xtask toolchain` failed");
        let errored = runner
            .in_process("toolchain", || bail!("no rust-toolchain.toml"))
            .expect_err("an error is passed on");
        assert_eq!(errored.to_string(), "no rust-toolchain.toml");
        assert!(
            runner
                .in_process("toolchain", || Ok(ExitCode::SUCCESS))
                .is_ok()
        );
        let dry = Runner { dry_run: true };
        assert!(
            dry.in_process("toolchain", || panic!("a dry run runs nothing"))
                .is_ok()
        );
    }
}
