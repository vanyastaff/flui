//! Running a task's commands.
//!
//! A command line is data ([`Cmd`]), so a plan of them can be printed
//! (`--dry-run`) or run. [`Runner`] prints each command before it runs it.
//! Every command runs from the repository
//! root with this process's environment, so `CARGO_TARGET_DIR`,
//! `CARGO_BUILD_JOBS`, `CARGO_MAKEFLAGS` and the rest reach the nested cargo
//! unchanged.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};

use crate::util::repo_root;

/// One command line: a program, its arguments and the variables it adds to
/// the inherited environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cmd {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl Cmd {
    /// `program` with no arguments.
    pub(super) fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    /// `cargo <args>`. Resolved on `PATH`, not through `$CARGO`: `+nightly`
    /// only means something to the rustup proxy.
    pub(super) fn cargo<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> Self {
        Self::new("cargo").args(args)
    }

    /// Appends arguments.
    pub(super) fn args<S: AsRef<str>>(mut self, args: impl IntoIterator<Item = S>) -> Self {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        self
    }

    /// Sets `key=value` for this command only.
    pub(super) fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .current_dir(repo_root());
        command
    }

    /// Runs it with inherited stdio; a non-zero exit is an error naming it.
    pub(super) fn status(&self) -> anyhow::Result<()> {
        let status = self
            .command()
            .status()
            .with_context(|| format!("running `{self}`"))?;
        if !status.success() {
            bail!("`{self}` failed ({status})");
        }
        Ok(())
    }
}

/// `value` as a POSIX shell word, for display: bare when it needs no quoting,
/// single-quoted otherwise.
fn shell_word(value: &str) -> String {
    let bare = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !value.is_empty() && value.chars().all(bare) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', r#"'"'"'"#))
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (key, value) in &self.env {
            write!(f, "{key}={} ", shell_word(value))?;
        }
        f.write_str(&shell_word(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", shell_word(arg))?;
        }
        Ok(())
    }
}

/// Runs a task's commands (`every` runs them all, even after one fails); with
/// `dry_run` it only prints them.
#[derive(Debug, Clone, Copy)]
pub(super) struct Runner {
    pub(super) dry_run: bool,
}

impl Runner {
    /// Prints `cmd`, then runs it.
    pub(super) fn run(self, cmd: &Cmd) -> anyhow::Result<()> {
        println!("$ {cmd}");
        if self.dry_run {
            return Ok(());
        }
        cmd.status()
    }

    /// Runs every command even after one fails; an error naming each failed
    /// command if any did.
    pub(super) fn every(self, cmds: &[Cmd]) -> anyhow::Result<()> {
        let mut failed = Vec::new();
        for cmd in cmds {
            if let Err(error) = self.run(cmd) {
                eprintln!("{error:#}");
                failed.push(cmd.to_string());
            }
        }
        if failed.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "{} step(s) failed:\n  {}",
            failed.len(),
            failed.join("\n  ")
        )
    }

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
/// (relative to the repository root, where every task's cargo runs),
/// `build.target-dir`, or `target/`. Built artifacts are looked up here, never
/// under a hardcoded `target/`.
pub(super) fn target_dir() -> anyhow::Result<PathBuf> {
    target_dir_with(&repo_root(), None)
}

/// [`target_dir`] for the workspace at `root`, with `CARGO_TARGET_DIR`
/// overridden when `target_dir` is given.
fn target_dir_with(root: &Path, target_dir: Option<&OsStr>) -> anyhow::Result<PathBuf> {
    let mut command = cargo_metadata::MetadataCommand::new();
    command.current_dir(root).no_deps();
    if let Some(dir) = target_dir {
        command.env("CARGO_TARGET_DIR", dir);
    }
    let metadata = command.exec().context("running `cargo metadata`")?;
    Ok(metadata.target_directory.into_std_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_failing_command_is_an_error_naming_it() {
        let error = Cmd::cargo(["--no-such-flag-xtask"])
            .status()
            .expect_err("cargo rejects the flag");
        assert!(
            error
                .to_string()
                .starts_with("`cargo --no-such-flag-xtask` failed"),
            "{error}"
        );
        let missing = Cmd::new("flui-xtask-no-such-program").status();
        assert!(missing.is_err());
    }

    /// A failing command fails the task: `every` runs the rest but still
    /// fails, naming each failed command. A task returns this error from its
    /// `run`, which `main` turns into exit 1.
    fn a_failing_step_fails_the_task() {
        let runner = Runner { dry_run: false };
        let error = runner
            .every(&[
                Cmd::cargo(["--no-such-flag-xtask"]),
                Cmd::cargo(["--version"]),
                Cmd::new("flui-xtask-no-such-program"),
            ])
            .expect_err("`every` still fails when a step did");
        let message = error.to_string();
        assert!(message.starts_with("2 step(s) failed"), "{message}");
        assert!(message.contains("cargo --no-such-flag-xtask"), "{message}");
        assert!(message.contains("flui-xtask-no-such-program"), "{message}");

        // a dry run executes nothing, so nothing fails
        assert!(
            Runner { dry_run: true }
                .every(&[Cmd::cargo(["--no-such-flag-xtask"])])
                .is_ok()
        );
    }

    #[test]
    fn exec_failure_contract() {
        crate::table_test::run_table(
            "exec_failure_contract",
            &[
                (
                    "a_failing_command_is_an_error_naming_it",
                    a_failing_command_is_an_error_naming_it as fn(),
                ),
                (
                    "a_failing_step_fails_the_task",
                    a_failing_step_fails_the_task as fn(),
                ),
            ],
        );
    }
}
