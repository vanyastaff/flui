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
//!
//! # Examples
//!
//! ```ignore
//! use flui_cli::runner::{CargoCommand, OutputStyle};
//!
//! // Simple cargo build
//! CargoCommand::build()
//!     .release()
//!     .run()?;
//!
//! // With spinner
//! CargoCommand::test()
//!     .filter("my_test")
//!     .output_style(OutputStyle::Spinner("Running tests..."))
//!     .run()?;
//!
//! // Verbose output
//! CargoCommand::clippy()
//!     .workspace()
//!     .output_style(OutputStyle::Streaming)
//!     .run()?;
//! ```

use crate::error::{CliError, CliResult, ResultExt};
use std::process::{Command, ExitStatus, Stdio};

// ============================================================================
// Output Styles
// ============================================================================

/// Output style for command execution.
///
/// Controls how command output is displayed to the user.
///
/// Marked `#[non_exhaustive]` to allow adding new output styles without
/// breaking changes (C-NON-EXHAUSTIVE).
#[derive(Debug, Clone)]
#[non_exhaustive]
#[derive(Default)]
pub enum OutputStyle {
    /// No output, only return status.
    Silent,

    /// Show a spinner with message while running.
    Spinner(String),

    /// Stream output in real-time.
    #[default]
    Streaming,

    /// Capture and display all output after completion.
    Captured,

    /// Show verbose output with command details.
    Verbose,
}

// ============================================================================
// Command Failure Types
// ============================================================================

/// Failure type for command execution.
///
/// This enum is Clone-able and can be stored in builders, then converted
/// to `CliError` when the command actually fails.
///
/// Marked `#[non_exhaustive]` to allow adding new failure types without
/// breaking changes (C-NON-EXHAUSTIVE).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CommandFailure {
    /// Build failed for the specified platform.
    Build(String),
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
    /// Application run failed.
    Run,
    /// CLI upgrade failed.
    Upgrade,
    /// Dependencies update failed.
    Update,
    /// Custom failure with context message.
    Custom(String),
}

impl From<CommandFailure> for CliError {
    fn from(failure: CommandFailure) -> Self {
        match failure {
            CommandFailure::Build(platform) => CliError::BuildFailed {
                platform,
                details: "cargo build failed".into(),
            },
            CommandFailure::Test => CliError::TestsFailed,
            CommandFailure::Clippy => CliError::AnalysisFailed,
            CommandFailure::Format => CliError::FormattingFailed,
            CommandFailure::FormatCheck => CliError::FormattingCheckFailed,
            CommandFailure::Clean => CliError::CleanFailed {
                details: "cargo clean failed".into(),
            },
            CommandFailure::Run => CliError::RunFailed,
            CommandFailure::Upgrade => CliError::UpgradeFailed,
            CommandFailure::Update => CliError::UpdateFailed,
            CommandFailure::Custom(context) => CliError::CommandFailed {
                context,
                exit_code: None,
            },
        }
    }
}

// ============================================================================
// Command Runner
// ============================================================================

/// Result of a command execution.
#[derive(Debug)]
#[must_use]
pub struct CommandResult {
    /// Exit status of the command.
    pub status: ExitStatus,
    /// Captured stdout (if `OutputStyle::Captured`).
    pub stdout: Option<String>,
    /// Captured stderr (if `OutputStyle::Captured`).
    pub stderr: Option<String>,
}

impl CommandResult {
    /// Check if command succeeded.
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Get exit code.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Core command runner that handles execution and output.
#[derive(Debug)]
pub struct CommandRunner {
    command: Command,
    output_style: OutputStyle,
    error_context: String,
    success_message: Option<String>,
    failure_type: Option<CommandFailure>,
}

impl CommandRunner {
    /// Create a new runner for an existing Command.
    pub fn new(command: Command, context: impl Into<String>) -> Self {
        Self {
            command,
            output_style: OutputStyle::default(),
            error_context: context.into(),
            success_message: None,
            failure_type: None,
        }
    }

    /// Set output style.
    #[must_use]
    pub fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Set message to show on success.
    #[must_use]
    pub fn on_success(mut self, message: impl Into<String>) -> Self {
        self.success_message = Some(message.into());
        self
    }

    /// Set failure type to return on command failure.
    #[must_use]
    pub fn on_failure(mut self, failure: CommandFailure) -> Self {
        self.failure_type = Some(failure);
        self
    }

    /// Execute the command and handle output.
    pub fn run(mut self) -> CliResult<CommandResult> {
        let style = std::mem::take(&mut self.output_style);
        match style {
            OutputStyle::Silent => self.run_silent(),
            OutputStyle::Spinner(msg) => self.run_with_spinner(&msg),
            OutputStyle::Streaming => self.run_streaming(),
            OutputStyle::Captured => self.run_captured(),
            OutputStyle::Verbose => self.run_verbose(),
        }
    }

    fn run_silent(&mut self) -> CliResult<CommandResult> {
        self.command.stdout(Stdio::null()).stderr(Stdio::null());

        let status = self
            .command
            .status()
            .with_context(|| format!("Failed to execute: {}", self.error_context))?;

        self.handle_result(status, None, None)
    }

    fn run_with_spinner(&mut self, message: &str) -> CliResult<CommandResult> {
        let spinner = cliclack::spinner();
        spinner.start(message);

        self.command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = self
            .command
            .output()
            .with_context(|| format!("Failed to execute: {}", self.error_context))?;

        spinner.stop(message);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        self.handle_result(output.status, Some(stdout), Some(stderr))
    }

    fn run_streaming(&mut self) -> CliResult<CommandResult> {
        self.command
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = self
            .command
            .status()
            .with_context(|| format!("Failed to execute: {}", self.error_context))?;

        self.handle_result(status, None, None)
    }

    fn run_captured(&mut self) -> CliResult<CommandResult> {
        self.command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = self
            .command
            .output()
            .with_context(|| format!("Failed to execute: {}", self.error_context))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        self.handle_result(output.status, Some(stdout), Some(stderr))
    }

    fn run_verbose(&mut self) -> CliResult<CommandResult> {
        // Print command being executed
        let _ = cliclack::log::remark(format!("Running: {:?}", self.command));

        self.run_streaming()
    }

    fn handle_result(
        &self,
        status: ExitStatus,
        stdout: Option<String>,
        stderr: Option<String>,
    ) -> CliResult<CommandResult> {
        let result = CommandResult {
            status,
            stdout,
            stderr,
        };

        if result.success() {
            if let Some(msg) = &self.success_message {
                let _ = cliclack::log::success(msg);
            }
            Ok(result)
        } else if let Some(failure) = &self.failure_type {
            Err(failure.clone().into())
        } else {
            Err(CliError::command_failed(&self.error_context, result.code()))
        }
    }
}

// ============================================================================
// Cargo Command Builder
// ============================================================================

/// Builder for cargo commands.
///
/// Provides a fluent API for constructing cargo commands with proper
/// argument handling and output styling.
///
/// # Examples
///
/// ```ignore
/// // Build in release mode
/// CargoCommand::build()
///     .release()
///     .run()?;
///
/// // Run tests with filter
/// CargoCommand::test()
///     .filter("integration")
///     .run()?;
///
/// // Clippy with workspace
/// CargoCommand::clippy()
///     .workspace()
///     .deny_warnings()
///     .run()?;
/// ```
#[derive(Debug, Clone)]
pub struct CargoCommand {
    subcommand: String,
    args: Vec<String>,
    separator_args: Vec<String>, // args after --
    env_vars: Vec<(String, String)>,
    output_style: OutputStyle,
    success_message: Option<String>,
    failure_type: Option<CommandFailure>,
}

impl CargoCommand {
    /// Create a cargo build command.
    #[must_use]
    pub fn build() -> Self {
        Self::new("build").on_failure(CommandFailure::Build("default".into()))
    }

    /// Create a cargo test command.
    #[must_use]
    pub fn test() -> Self {
        Self::new("test").on_failure(CommandFailure::Test)
    }

    /// Create a cargo clippy command.
    #[must_use]
    pub fn clippy() -> Self {
        Self::new("clippy").on_failure(CommandFailure::Clippy)
    }

    /// Create a cargo fmt command.
    #[must_use]
    pub fn fmt() -> Self {
        Self::new("fmt").on_failure(CommandFailure::Format)
    }

    /// Create a cargo clean command.
    #[must_use]
    pub fn clean() -> Self {
        Self::new("clean").on_failure(CommandFailure::Clean)
    }

    /// Create a cargo run command.
    #[must_use]
    pub fn run_app() -> Self {
        Self::new("run").on_failure(CommandFailure::Run)
    }

    /// Create a cargo update command.
    #[must_use]
    pub fn update() -> Self {
        Self::new("update").on_failure(CommandFailure::Update)
    }

    /// Create a cargo install command.
    pub fn install(package: impl Into<String>) -> Self {
        Self::new("install")
            .arg(package.into())
            .on_failure(CommandFailure::Upgrade)
    }

    /// Create a custom cargo command.
    pub fn new(subcommand: impl Into<String>) -> Self {
        Self {
            subcommand: subcommand.into(),
            args: Vec::new(),
            separator_args: Vec::new(),
            env_vars: Vec::new(),
            output_style: OutputStyle::Streaming,
            success_message: None,
            failure_type: None,
        }
    }

    /// Add a single argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Add argument after -- separator.
    #[must_use]
    pub fn separator_arg(mut self, arg: impl Into<String>) -> Self {
        self.separator_args.push(arg.into());
        self
    }

    /// Add multiple arguments after -- separator.
    #[must_use]
    pub fn separator_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.separator_args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Build in release mode.
    #[must_use]
    pub fn release(self) -> Self {
        self.arg("--release")
    }

    /// Target all workspace members.
    #[must_use]
    pub fn workspace(self) -> Self {
        self.arg("--workspace")
    }

    /// Target specific package.
    #[must_use]
    pub fn package(self, name: impl Into<String>) -> Self {
        self.arg("-p").arg(name)
    }

    /// Add --all flag.
    #[must_use]
    pub fn all(self) -> Self {
        self.arg("--all")
    }

    /// Add --check flag (for cargo fmt).
    #[must_use]
    pub fn check(mut self) -> Self {
        self.failure_type = Some(CommandFailure::FormatCheck);
        self.arg("--check")
    }

    /// Add --force flag.
    #[must_use]
    pub fn force(self) -> Self {
        self.arg("--force")
    }

    /// Add test filter.
    #[must_use]
    pub fn filter(self, filter: impl Into<String>) -> Self {
        self.arg(filter)
    }

    /// Add --lib flag (test only library).
    #[must_use]
    pub fn lib_only(self) -> Self {
        self.arg("--lib")
    }

    /// Add --test flag (test only integration tests).
    #[must_use]
    pub fn integration_only(self) -> Self {
        self.arg("--test")
    }

    /// Add -D warnings (for clippy).
    #[must_use]
    pub fn deny_warnings(self) -> Self {
        self.separator_arg("-D").separator_arg("warnings")
    }

    /// Add -W `clippy::pedantic` (for clippy).
    #[must_use]
    pub fn pedantic(self) -> Self {
        self.separator_arg("-W").separator_arg("clippy::pedantic")
    }

    /// Add --fix flag (for clippy).
    #[must_use]
    pub fn fix(self) -> Self {
        self.arg("--fix")
    }

    /// Add --verbose flag.
    #[must_use]
    pub fn verbose(self) -> Self {
        self.arg("--verbose")
    }

    /// Set custom target triple.
    #[must_use]
    pub fn target(self, triple: impl Into<String>) -> Self {
        self.arg("--target").arg(triple)
    }

    /// Set custom profile.
    #[must_use]
    pub fn profile(self, profile: impl Into<String>) -> Self {
        self.arg(format!("--profile={}", profile.into()))
    }

    /// Set environment variable.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Set output style.
    #[must_use]
    pub fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Set success message.
    #[must_use]
    pub fn on_success(mut self, message: impl Into<String>) -> Self {
        self.success_message = Some(message.into());
        self
    }

    /// Set failure type.
    #[must_use]
    pub fn on_failure(mut self, failure: CommandFailure) -> Self {
        self.failure_type = Some(failure);
        self
    }

    /// Execute the command.
    pub fn run(self) -> CliResult<CommandResult> {
        let mut cmd = Command::new("cargo");
        cmd.arg(&self.subcommand);

        // Add environment variables
        for (key, value) in &self.env_vars {
            cmd.env(key, value);
        }

        // Add regular args
        for arg in &self.args {
            cmd.arg(arg);
        }

        // Add separator args
        if !self.separator_args.is_empty() {
            cmd.arg("--");
            for arg in &self.separator_args {
                cmd.arg(arg);
            }
        }

        let mut runner = CommandRunner::new(cmd, format!("cargo {}", self.subcommand))
            .output_style(self.output_style);

        if let Some(msg) = self.success_message {
            runner = runner.on_success(msg);
        }

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
pub struct GitCommand {
    subcommand: String,
    args: Vec<String>,
    output_style: OutputStyle,
}

impl GitCommand {
    /// Create a git init command.
    #[must_use]
    pub fn init() -> Self {
        Self::new("init")
    }

    /// Create a git add command.
    #[must_use]
    pub fn add() -> Self {
        Self::new("add")
    }

    /// Create a git commit command.
    #[must_use]
    pub fn commit() -> Self {
        Self::new("commit")
    }

    /// Create a custom git command.
    pub fn new(subcommand: impl Into<String>) -> Self {
        Self {
            subcommand: subcommand.into(),
            args: Vec::new(),
            output_style: OutputStyle::Silent,
        }
    }

    /// Add argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add all files.
    #[must_use]
    pub fn all(self) -> Self {
        self.arg("-A")
    }

    /// Set commit message.
    #[must_use]
    pub fn message(self, msg: impl Into<String>) -> Self {
        self.arg("-m").arg(msg)
    }

    /// Set output style.
    #[must_use]
    pub fn output_style(mut self, style: OutputStyle) -> Self {
        self.output_style = style;
        self
    }

    /// Execute the command.
    pub fn run(self) -> CliResult<CommandResult> {
        let mut cmd = Command::new("git");
        cmd.arg(&self.subcommand);

        for arg in &self.args {
            cmd.arg(arg);
        }

        CommandRunner::new(cmd, format!("git {}", self.subcommand))
            .output_style(self.output_style)
            .run()
    }
}

// ============================================================================
// Console Output (cliclack) - Re-exports for interactive prompts
// ============================================================================

/// Re-export cliclack interactive prompt types.
pub use cliclack::{confirm, input, multiselect, password, select};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_command_builds_correctly() {
        let cmd = CargoCommand::build().release().workspace();
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
    fn git_command_builds_correctly() {
        let cmd = GitCommand::commit().message("test commit");
        assert!(cmd.args.contains(&"-m".to_string()));
        assert!(cmd.args.contains(&"test commit".to_string()));
    }

    #[test]
    fn command_failure_converts_to_cli_error() {
        let failure = CommandFailure::Test;
        let error: CliError = failure.into();
        assert!(matches!(error, CliError::TestsFailed));
    }
}
