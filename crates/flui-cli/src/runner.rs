//! Command execution system for FLUI CLI.
//!
//! This module provides a unified way to execute external commands with:
//! - Consistent output formatting
//! - Progress indication (spinners)
//! - Error handling with context
//! - Builder pattern for command construction
//!
//! # Architecture
//!
//! ```text
//! CommandBuilder (builder pattern)
//!        │
//!        ▼
//! ┌─────────────────┐
//! │  CargoCommand   │  ◄── Specialized builders
//! │  GitCommand     │
//! │  NpmCommand     │
//! └─────────────────┘
//!        │
//!        ▼
//! ┌─────────────────┐
//! │  CommandRunner  │  ◄── Execution with output handling
//! └─────────────────┘
//!        │
//!        ▼
//! ┌─────────────────┐
//! │  OutputStyle    │  ◄── Silent, Spinner, Streaming, Verbose
//! └─────────────────┘
//! ```

use crate::error::{CliError, CliResult, ResultExt};
use crate::ui;
use std::process::{Command, ExitStatus, Stdio};

// ============================================================================
// Output Styles
// ============================================================================

/// How a child command's output reaches the user.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum OutputStyle {
    /// No output, only the exit status.
    Silent,

    /// Inherit stdout and stderr, so the tool's output streams through.
    #[default]
    Streaming,

    /// Capture both streams; only the exit status is reported.
    Captured,
}

// ============================================================================
// Command Failure Types
// ============================================================================

/// Which `CliError` a failing command turns into. Stored in the builder,
/// converted when the command actually fails.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandFailure {
    /// Tests failed.
    Test,
    /// Clippy analysis found issues.
    Clippy,
    /// Code formatting failed.
    Format,
    /// Code formatting check failed (code is not formatted).
    FormatCheck,
    /// Clean operation failed.
    Clean,
    /// Dependencies update failed.
    Update,
}

impl From<CommandFailure> for CliError {
    fn from(failure: CommandFailure) -> Self {
        match failure {
            CommandFailure::Test => CliError::TestsFailed,
            CommandFailure::Clippy => CliError::AnalysisFailed,
            CommandFailure::Format => CliError::FormattingFailed,
            CommandFailure::FormatCheck => CliError::FormattingCheckFailed,
            CommandFailure::Clean => CliError::CleanFailed {
                details: "cargo clean failed".into(),
            },
            CommandFailure::Update => CliError::UpdateFailed,
        }
    }
}

// ============================================================================
// Command Runner
// ============================================================================

/// Result of a command execution.
#[derive(Debug)]
#[must_use]
pub(crate) struct CommandResult {
    /// Exit status of the command.
    pub(crate) status: ExitStatus,
}

impl CommandResult {
    /// Check if command succeeded.
    #[must_use]
    pub(crate) fn success(&self) -> bool {
        self.status.success()
    }

    /// Get exit code.
    #[must_use]
    pub(crate) fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Runs one external command under an [`OutputStyle`] and maps a non-zero
/// exit to the configured [`CommandFailure`].
#[derive(Debug)]
pub(crate) struct CommandRunner {
    command: Command,
    output_style: OutputStyle,
    error_context: String,
    failure_type: Option<CommandFailure>,
}

impl CommandRunner {
    /// Create a new runner for an existing Command.
    pub(crate) fn new(command: Command, context: impl Into<String>) -> Self {
        Self {
            command,
            output_style: OutputStyle::default(),
            error_context: context.into(),
            failure_type: None,
        }
    }

    /// Set output style.
    #[must_use]
    pub(crate) fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Set failure type to return on command failure.
    #[must_use]
    pub(crate) fn on_failure(mut self, failure: CommandFailure) -> Self {
        self.failure_type = Some(failure);
        self
    }

    /// Execute the command and handle output.
    pub(crate) fn run(mut self) -> CliResult<CommandResult> {
        ui::debug(format!("running: {:?}", self.command));
        let status = match self.output_style {
            OutputStyle::Silent => {
                self.command.stdout(Stdio::null()).stderr(Stdio::null());
                self.command.status()
            }
            OutputStyle::Streaming => {
                self.command
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
                self.command.status()
            }
            OutputStyle::Captured => {
                self.command.stdout(Stdio::piped()).stderr(Stdio::piped());
                self.command.output().map(|output| output.status)
            }
        }
        .with_context(|| format!("failed to execute: {}", self.error_context))?;

        let result = CommandResult { status };
        if result.success() {
            Ok(result)
        } else if let Some(failure) = self.failure_type {
            Err(failure.into())
        } else {
            Err(CliError::command_failed(&self.error_context, result.code()))
        }
    }
}

// ============================================================================
// Cargo Command Builder
// ============================================================================

/// Builder for the cargo invocations the maintenance commands run.
#[derive(Debug, Clone)]
pub(crate) struct CargoCommand {
    subcommand: String,
    args: Vec<String>,
    /// Arguments after `--`, handed to the tool cargo invokes.
    separator_args: Vec<String>,
    output_style: OutputStyle,
    failure_type: Option<CommandFailure>,
}

impl CargoCommand {
    /// Create a cargo test command.
    #[must_use]
    pub(crate) fn test() -> Self {
        Self::new("test").on_failure(CommandFailure::Test)
    }

    /// Create a cargo clippy command.
    #[must_use]
    pub(crate) fn clippy() -> Self {
        Self::new("clippy").on_failure(CommandFailure::Clippy)
    }

    /// Create a cargo fmt command.
    #[must_use]
    pub(crate) fn fmt() -> Self {
        Self::new("fmt").on_failure(CommandFailure::Format)
    }

    /// Create a cargo clean command.
    #[must_use]
    pub(crate) fn clean() -> Self {
        Self::new("clean").on_failure(CommandFailure::Clean)
    }

    /// Create a cargo update command.
    #[must_use]
    pub(crate) fn update() -> Self {
        Self::new("update").on_failure(CommandFailure::Update)
    }

    /// Create a custom cargo command.
    pub(crate) fn new(subcommand: impl Into<String>) -> Self {
        Self {
            subcommand: subcommand.into(),
            args: Vec::new(),
            separator_args: Vec::new(),
            output_style: OutputStyle::Streaming,
            failure_type: None,
        }
    }

    /// Add a single argument.
    #[must_use]
    pub(crate) fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add argument after -- separator.
    #[must_use]
    pub(crate) fn separator_arg(mut self, arg: impl Into<String>) -> Self {
        self.separator_args.push(arg.into());
        self
    }

    /// Add multiple arguments after -- separator.
    #[must_use]
    pub(crate) fn separator_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.separator_args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Build in release mode.
    #[must_use]
    pub(crate) fn release(self) -> Self {
        self.arg("--release")
    }

    /// Target all workspace members.
    #[must_use]
    pub(crate) fn workspace(self) -> Self {
        self.arg("--workspace")
    }

    /// Add --all flag.
    #[must_use]
    pub(crate) fn all(self) -> Self {
        self.arg("--all")
    }

    /// Add --check flag (for cargo fmt).
    #[must_use]
    pub(crate) fn check(mut self) -> Self {
        self.failure_type = Some(CommandFailure::FormatCheck);
        self.arg("--check")
    }

    /// Add test filter.
    #[must_use]
    pub(crate) fn filter(self, filter: impl Into<String>) -> Self {
        self.arg(filter)
    }

    /// Add --lib flag (test only library).
    #[must_use]
    pub(crate) fn lib_only(self) -> Self {
        self.arg("--lib")
    }

    /// Add --tests flag (test only integration tests).
    ///
    /// `--test` (singular) needs a harness *name* and is a usage error
    /// without one; `--tests` (plural) means "every integration-test
    /// target", which is what "integration tests only" means here.
    #[must_use]
    pub(crate) fn integration_only(self) -> Self {
        self.arg("--tests")
    }

    /// Add --all-targets flag (lint/build every target: lib, bins, tests,
    /// examples, benches).
    #[must_use]
    pub(crate) fn all_targets(self) -> Self {
        self.arg("--all-targets")
    }

    /// Add -D warnings (for clippy).
    #[must_use]
    pub(crate) fn deny_warnings(self) -> Self {
        self.separator_arg("-D").separator_arg("warnings")
    }

    /// Add -W `clippy::pedantic` (for clippy).
    #[must_use]
    pub(crate) fn pedantic(self) -> Self {
        self.separator_arg("-W").separator_arg("clippy::pedantic")
    }

    /// Add --fix flag (for clippy).
    #[must_use]
    pub(crate) fn fix(self) -> Self {
        self.arg("--fix")
    }

    /// Set output style.
    #[must_use]
    pub(crate) fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Set failure type.
    #[must_use]
    pub(crate) fn on_failure(mut self, failure: CommandFailure) -> Self {
        self.failure_type = Some(failure);
        self
    }

    /// Execute the command.
    pub(crate) fn run(self) -> CliResult<CommandResult> {
        let mut cmd = Command::new("cargo");
        cmd.arg(&self.subcommand);
        for arg in &self.args {
            cmd.arg(arg);
        }
        if !self.separator_args.is_empty() {
            cmd.arg("--");
            for arg in &self.separator_args {
                cmd.arg(arg);
            }
        }

        let mut runner = CommandRunner::new(cmd, format!("cargo {}", self.subcommand))
            .output_style(self.output_style);
        if let Some(failure) = self.failure_type {
            runner = runner.on_failure(failure);
        }
        runner.run()
    }
}

// ============================================================================
// Git Command Builder
// ============================================================================

/// Builder for git commands.
#[derive(Debug, Clone)]
pub(crate) struct GitCommand {
    subcommand: String,
    output_style: OutputStyle,
    /// The repository directory the command runs in. `None` inherits the
    /// process's working directory — never what a project-creating caller
    /// wants: `flui create` once ran `git init` wherever the user happened
    /// to stand, which is how a stray empty repository ended up inside this
    /// crate's own source tree from a test run.
    current_dir: Option<std::path::PathBuf>,
}

impl GitCommand {
    /// Create a git init command.
    #[must_use]
    pub(crate) fn init() -> Self {
        Self::new("init")
    }

    /// Create a custom git command.
    pub(crate) fn new(subcommand: impl Into<String>) -> Self {
        Self {
            subcommand: subcommand.into(),
            output_style: OutputStyle::Silent,
            current_dir: None,
        }
    }

    /// Run the command inside `dir` rather than the process's working
    /// directory. Every command that targets a specific repository must set
    /// this; see the field's doc for the failure it prevents.
    #[must_use]
    pub(crate) fn current_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.current_dir = Some(dir.into());
        self
    }

    /// Set output style.
    #[must_use]
    pub(crate) fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Execute the command.
    pub(crate) fn run(self) -> CliResult<CommandResult> {
        let mut cmd = Command::new("git");
        cmd.arg(&self.subcommand);
        if let Some(dir) = &self.current_dir {
            cmd.current_dir(dir);
        }

        CommandRunner::new(cmd, format!("git {}", self.subcommand))
            .output_style(self.output_style)
            .run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_command_builds_correctly() {
        let cmd = CargoCommand::test().release().workspace();
        assert!(cmd.args.contains(&"--release".to_string()));
        assert!(cmd.args.contains(&"--workspace".to_string()));
    }

    #[test]
    fn cargo_clippy_with_separator_args() {
        let cmd = CargoCommand::clippy()
            .workspace()
            .deny_warnings()
            .pedantic();
        assert!(cmd.args.contains(&"--workspace".to_string()));
        assert!(cmd.separator_args.contains(&"-D".to_string()));
        assert!(cmd.separator_args.contains(&"warnings".to_string()));
        assert!(cmd.separator_args.contains(&"-W".to_string()));
        assert!(cmd.separator_args.contains(&"clippy::pedantic".to_string()));
    }

    #[test]
    fn git_command_targets_the_given_repository() {
        let cmd = GitCommand::init().current_dir("/tmp/repo");
        assert_eq!(
            cmd.current_dir.as_deref(),
            Some(std::path::Path::new("/tmp/repo"))
        );
    }

    #[test]
    fn command_failure_converts_to_cli_error() {
        let failure = CommandFailure::Test;
        let error: CliError = failure.into();
        assert!(matches!(error, CliError::TestsFailed));
    }
}
