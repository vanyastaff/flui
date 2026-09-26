//! Running a task's commands.
//!
//! A command line is data ([`Cmd`]): a plan is a list of them ([`Step`]), so it
//! can be printed (`--dry-run`), compared with the CI job it mirrors in a unit
//! test, or run. [`Runner`] prints each command before it runs it, as `just`
//! did, and stops at the first failure. Every command runs from the repository
//! root with this process's environment, so `CARGO_TARGET_DIR`,
//! `CARGO_BUILD_JOBS` and the rest reach the nested cargo unchanged.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, bail};

use crate::util::repo_root;

/// The OS a task runs on: some steps exist on one host only (`xvfb-run` is
/// Linux's, the iOS SDK behind `xcrun` is macOS').
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Host {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl Host {
    /// The host this process runs on.
    pub(super) fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "macos" => Self::MacOs,
            _ => Self::Other,
        }
    }
}

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

    /// Appends a whitespace-separated argument list (the fast lane's
    /// `pkg_args`, `features`, ...: package names and flags, never quoted).
    pub(super) fn split(self, list: &str) -> Self {
        self.args(list.split_whitespace())
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

    /// Runs it with stdout and stderr merged into one stream, echoed as it
    /// arrives and also returned, with whether it succeeded: for a caller that
    /// reads the output and must still show all of it when the command fails.
    pub(super) fn merged(&self) -> anyhow::Result<(bool, String)> {
        let spawn = || -> std::io::Result<_> {
            let (reader, writer) = std::io::pipe()?;
            // The `Command` owns both write ends; it is dropped at the end of
            // this closure, so the pipe reaches EOF once the child tree exits.
            let child = self
                .command()
                .stdout(writer.try_clone()?)
                .stderr(writer)
                .spawn()?;
            Ok((reader, child))
        };
        let (mut reader, mut child) = spawn().with_context(|| format!("running `{self}`"))?;
        let mut captured = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut out = std::io::stdout().lock();
        loop {
            let read = reader
                .read(&mut chunk)
                .with_context(|| format!("reading the output of `{self}`"))?;
            if read == 0 {
                break;
            }
            out.write_all(&chunk[..read])?;
            captured.extend_from_slice(&chunk[..read]);
        }
        out.flush()?;
        let status = child
            .wait()
            .with_context(|| format!("waiting for `{self}`"))?;
        Ok((
            status.success(),
            String::from_utf8_lossy(&captured).into_owned(),
        ))
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

/// One step of a plan: a command, or a line saying why one does not run here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    Run(Cmd),
    Note(String),
}

impl From<Cmd> for Step {
    fn from(cmd: Cmd) -> Self {
        Self::Run(cmd)
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run(cmd) => write!(f, "$ {cmd}"),
            Self::Note(note) => f.write_str(note),
        }
    }
}

/// Runs a task's steps in order (`steps` stops at the first failure, `every`
/// runs them all); with `dry_run` it only prints them.
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

    /// Runs every step in order.
    pub(super) fn steps(self, steps: &[Step]) -> anyhow::Result<()> {
        for step in steps {
            match step {
                Step::Run(cmd) => self.run(cmd)?,
                Step::Note(note) => println!("{note}"),
            }
        }
        Ok(())
    }

    /// Runs every step even after one fails; an error naming each failed
    /// command if any did.
    pub(super) fn every(self, steps: &[Step]) -> anyhow::Result<()> {
        let mut failed = Vec::new();
        for step in steps {
            match step {
                Step::Run(cmd) => {
                    if let Err(error) = self.run(cmd) {
                        eprintln!("{error:#}");
                        failed.push(cmd.to_string());
                    }
                }
                Step::Note(note) => println!("{note}"),
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

/// No arguments, for [`parsed`].
pub(super) const NO_ARGS: [&str; 0] = [];

/// Parses `argv` as the arguments of one of this crate's commands, so an
/// in-process call passes exactly what its command line would.
pub(super) fn parsed<T: clap::Args + clap::FromArgMatches>(
    argv: impl IntoIterator<Item = impl Into<OsString>>,
) -> anyhow::Result<T> {
    let argv = std::iter::once(OsString::from("xtask")).chain(argv.into_iter().map(Into::into));
    let matches = T::augment_args(clap::Command::new("xtask")).try_get_matches_from(argv)?;
    Ok(T::from_arg_matches(&matches)?)
}

/// Whether `program <args>` runs and exits 0: an optional tool's presence.
pub(super) fn installed(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The rustup targets installed for the toolchain this checkout pins; empty
/// when rustup is not there to ask.
pub(super) fn installed_targets() -> BTreeSet<String> {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(repo_root())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
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

/// A binary cargo built for the host: `target_dir` joined with `dirs` (the
/// profile, then `examples` for an example) and `name` plus the host's
/// executable suffix.
pub(super) fn host_binary(target_dir: &Path, dirs: &[&str], name: &str) -> PathBuf {
    let mut path = target_dir.to_path_buf();
    path.extend(dirs);
    path.push(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_a_pasteable_shell_line() {
        let cmd = Cmd::new("xvfb-run")
            .args(["-a", "-s", "-screen 0 1200x800x24", "cargo"])
            .env("FLUI_HEADLESS", "1")
            .env("RUSTDOCFLAGS", "-D warnings");
        assert_eq!(
            cmd.to_string(),
            "FLUI_HEADLESS=1 RUSTDOCFLAGS='-D warnings' xvfb-run -a -s '-screen 0 1200x800x24' cargo"
        );
        assert_eq!(
            Cmd::cargo(["tree", "-i", "it's"]).to_string(),
            r#"cargo tree -i 'it'"'"'s'"#
        );
        assert_eq!(Cmd::cargo([""]).to_string(), "cargo ''");
    }

    #[test]
    fn split_takes_the_lane_lists_word_by_word() {
        let cmd = Cmd::cargo(["nextest", "run"])
            .split("-p flui -p flui-material")
            .split("")
            .split(" --features  flui/cupertino,flui/localizations ");
        assert_eq!(
            cmd.to_string(),
            "cargo nextest run -p flui -p flui-material --features flui/cupertino,flui/localizations"
        );
    }

    #[test]
    fn steps_print_as_commands_or_notes() {
        assert_eq!(
            Step::from(Cmd::cargo(["fmt"])).to_string(),
            "$ cargo fmt".to_owned()
        );
        assert_eq!(Step::Note("skipped".to_owned()).to_string(), "skipped");
    }

    #[test]
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

    #[test]
    fn merged_output_is_captured_whatever_the_exit() {
        let (ok, out) = Cmd::cargo(["--version"]).merged().expect("runs");
        assert!(ok);
        assert!(out.starts_with("cargo "), "{out}");
        let (ok, out) = Cmd::cargo(["--no-such-flag-xtask"]).merged().expect("runs");
        assert!(!ok);
        assert!(out.contains("--no-such-flag-xtask"), "{out}");
    }

    #[test]
    fn target_dir_is_what_cargo_resolves() {
        let root = repo_root();
        let scratch = std::env::temp_dir().join(format!("xtask-target-{}", std::process::id()));
        assert_eq!(
            target_dir_with(&root, Some(scratch.as_os_str())).expect("metadata"),
            scratch
        );
        // a relative CARGO_TARGET_DIR is relative to where cargo runs: the root
        assert_eq!(
            target_dir_with(&root, Some(OsStr::new("elsewhere"))).expect("metadata"),
            root.join("elsewhere")
        );
    }

    #[test]
    fn host_binaries_carry_the_platform_suffix() {
        let bin = host_binary(Path::new("t"), &["debug", "examples"], "sliver_demo");
        assert_eq!(
            bin,
            Path::new("t")
                .join("debug")
                .join("examples")
                .join(format!("sliver_demo{}", std::env::consts::EXE_SUFFIX))
        );
    }
}
