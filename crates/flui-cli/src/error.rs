//! The CLI's error type and its exit-code contract.
//!
//! Every command returns [`CliResult`]; `main` maps the error to the exit
//! code in [`CliError::exit_code`] and prints the chain of causes. Messages
//! start lowercase and carry no trailing period, so they read naturally
//! after `error:` and `Caused by:`.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for CLI operations.
pub(crate) type CliResult<T> = Result<T, CliError>;

/// Everything a command can fail with. Each variant maps to one exit code;
/// wrapped errors keep their source so the cause chain stays visible.
#[derive(Error, Debug)]
pub(crate) enum CliError {
    // ========================================================================
    // Project Creation Errors
    // ========================================================================
    /// Project directory already exists.
    ///
    /// Returned when attempting to create a project in a directory that
    /// already exists.
    #[error("directory '{path}' already exists")]
    DirectoryExists {
        /// Path to the existing directory
        path: PathBuf,
    },

    /// Invalid project name provided.
    ///
    /// Project names must be valid Rust crate names.
    #[error("invalid project name '{name}': {reason}")]
    InvalidProjectName {
        /// The invalid project name
        name: String,
        /// Reason why the name is invalid
        reason: String,
    },

    /// Invalid organization identifier provided.
    ///
    /// Organization IDs must be in reverse domain notation (e.g., "com.example").
    #[error("invalid organization ID '{id}': {reason}")]
    InvalidOrganizationId {
        /// The invalid organization ID
        id: String,
        /// Reason why the ID is invalid
        reason: String,
    },

    // ========================================================================
    // Tool/Environment Errors
    // ========================================================================
    /// Required tool not found on system.
    ///
    /// Returned when a required external tool (e.g., cargo, git) is not
    /// available in the system PATH.
    #[error("required tool '{tool}' not found; {suggestion}")]
    ToolNotFound {
        /// Name of the missing tool
        tool: String,
        /// Suggestion for how to install the tool
        suggestion: String,
    },

    /// Not a FLUI project.
    ///
    /// Returned when a command is run outside a FLUI project directory.
    #[error("not a FLUI project: {reason}")]
    NotFluiProject {
        /// Reason why this is not a FLUI project
        reason: String,
    },

    /// A named device, emulator or simulator does not exist.
    #[error("device '{name}' not found; {hint}")]
    DeviceNotFound {
        /// What the user asked for.
        name: String,
        /// How to see what exists (`flui devices`, `flui emulators list`).
        hint: String,
    },

    /// `flui doctor` found a required component missing or broken.
    #[error("{failed} of {total} environment checks failed")]
    EnvironmentCheckFailed {
        /// Checks that reported an error (not warnings).
        failed: usize,
        /// Checks that ran.
        total: usize,
    },

    /// A prompt was needed but the session cannot prompt.
    ///
    /// Raised instead of blocking when stdin is not a terminal, `CI` is set,
    /// or `--non-interactive` was passed.
    #[error("{what} needs an interactive terminal; {hint}")]
    NonInteractive {
        /// What would have prompted.
        what: String,
        /// The flag(s) that make the prompt unnecessary.
        hint: String,
    },

    /// The user interrupted a long-running command (Ctrl-C).
    #[error("interrupted")]
    Interrupted,

    /// The arguments are well-formed for clap but make no sense together
    /// (a selector conflict clap cannot express). Exit code 2, like clap's
    /// own usage errors.
    #[error("{0}")]
    Usage(String),

    /// The requested platform cannot be driven from this host or by this
    /// command (e.g. `flui run --device <android serial>` today).
    #[error("{what} is not supported: {reason}")]
    Unsupported {
        /// What was asked for.
        what: String,
        /// Why, and what to do instead.
        reason: String,
    },

    // ========================================================================
    // Build/Run Errors
    // ========================================================================
    /// Build operation failed.
    #[error("build for '{platform}' failed: {details}")]
    BuildFailed {
        /// Platform that failed to build
        platform: String,
        /// Details about the build failure
        details: String,
    },

    /// Clean operation failed.
    #[error("clean failed: {details}")]
    CleanFailed {
        /// Details about the clean failure
        details: String,
    },

    /// The application exited unsuccessfully.
    #[error("the application failed: {details}")]
    RunFailed {
        /// How it ended (exit code or signal).
        details: String,
    },

    /// Analysis found issues.
    #[error("analysis found issues")]
    AnalysisFailed,

    /// Test execution failed.
    #[error("tests failed")]
    TestsFailed,

    // ========================================================================
    // Update/Upgrade Errors
    // ========================================================================
    /// Upgrade operation failed.
    #[error("upgrade failed")]
    UpgradeFailed,

    /// Update operation failed.
    #[error("update failed")]
    UpdateFailed,

    // ========================================================================
    // User Interaction Errors
    // ========================================================================
    /// User cancelled interactive operation.
    #[error("cancelled")]
    UserCancelled,

    // ========================================================================
    // Format Errors
    // ========================================================================
    /// Code formatting check failed.
    #[error("code is not formatted; run `flui format`")]
    FormattingCheckFailed,

    /// Code formatting failed.
    #[error("formatting failed")]
    FormattingFailed,

    // ========================================================================
    // Wrapped External Errors
    // ========================================================================
    /// I/O error occurred.
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// TOML parsing error.
    #[error("TOML parse error")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error.
    #[error("TOML serialization error")]
    TomlSerialize(#[from] toml::ser::Error),

    /// Build system error.
    #[error("build failed")]
    Build(#[from] crate::build::error::BuildError),

    /// Generic error with context.
    ///
    /// Used when wrapping errors with additional context.
    /// The source is always Send + Sync for thread safety.
    #[error("{message}")]
    WithContext {
        /// Error message with context
        message: String,
        /// Underlying source error (always Send + Sync)
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A required value was missing.
    ///
    /// Used when converting `Option::None` to an error without
    /// an underlying source error.
    #[error("{0}")]
    Missing(String),

    /// Command execution failed.
    ///
    /// Used when an external command fails with a specific exit code.
    #[error("{context} failed{}", match .exit_code {
        Some(code) => format!(" with exit code {code}"),
        None => " (terminated by a signal)".to_string(),
    })]
    CommandFailed {
        /// Context describing what was being executed
        context: String,
        /// Exit code from the command (None if terminated by signal)
        exit_code: Option<i32>,
    },
}

impl CliError {
    /// Add context to an error.
    ///
    /// Wraps any error with additional context message while preserving
    /// the original error as the source.
    pub(crate) fn context<E>(err: E, message: impl Into<String>) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::WithContext {
            message: message.into(),
            source: Box::new(err),
        }
    }

    /// Create a new "build failed" error.
    pub(crate) fn build_failed(platform: impl Into<String>, details: impl Into<String>) -> Self {
        Self::BuildFailed {
            platform: platform.into(),
            details: details.into(),
        }
    }

    /// Create a new "command failed" error.
    pub(crate) fn command_failed(context: impl Into<String>, exit_code: Option<i32>) -> Self {
        Self::CommandFailed {
            context: context.into(),
            exit_code,
        }
    }

    /// The process exit code for this error.
    ///
    /// The table is part of the CLI's contract (documented in the crate
    /// README under "Exit codes") so scripts can branch on *why* a command
    /// failed instead of parsing text:
    ///
    /// | code | meaning |
    /// |-----:|---------|
    /// | 0 | success (also: the user cancelled a prompt on purpose) |
    /// | 1 | generic failure |
    /// | 2 | usage error (clap, or a selector conflict) or an unsupported target |
    /// | 3 | environment: a required tool is missing or `doctor` found errors |
    /// | 4 | the project's build, tests, lints or format check failed |
    /// | 5 | the requested device / emulator does not exist |
    /// | 6 | not a FLUI project (run from the wrong directory) |
    /// | 7 | a prompt was needed but the session is non-interactive |
    /// | 130 | interrupted with Ctrl-C |
    #[must_use]
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            Self::UserCancelled => exit_code::SUCCESS,
            Self::Usage(_) | Self::Unsupported { .. } => exit_code::USAGE,
            Self::ToolNotFound { .. } | Self::EnvironmentCheckFailed { .. } => {
                exit_code::ENVIRONMENT
            }
            Self::BuildFailed { .. }
            | Self::Build(_)
            | Self::RunFailed { .. }
            | Self::AnalysisFailed
            | Self::TestsFailed
            | Self::FormattingCheckFailed
            | Self::FormattingFailed => exit_code::BUILD,
            Self::DeviceNotFound { .. } => exit_code::DEVICE,
            Self::NotFluiProject { .. } => exit_code::NOT_A_PROJECT,
            Self::NonInteractive { .. } => exit_code::NON_INTERACTIVE,
            Self::Interrupted => exit_code::INTERRUPTED,
            _ => exit_code::FAILURE,
        }
    }
}

/// The exit codes `flui` uses. See [`CliError::exit_code`] for the table.
pub(crate) mod exit_code {
    /// The command completed.
    pub(crate) const SUCCESS: i32 = 0;
    /// Generic failure.
    pub(crate) const FAILURE: i32 = 1;
    /// Usage error (clap's own convention, or a selector conflict) or an
    /// unsupported target.
    pub(crate) const USAGE: i32 = 2;
    /// A required tool is missing or the environment check found errors.
    pub(crate) const ENVIRONMENT: i32 = 3;
    /// The project failed to build, test, lint or format-check.
    pub(crate) const BUILD: i32 = 4;
    /// The requested device or emulator does not exist.
    pub(crate) const DEVICE: i32 = 5;
    /// The working directory is not a FLUI project.
    pub(crate) const NOT_A_PROJECT: i32 = 6;
    /// A prompt was needed in a non-interactive session.
    pub(crate) const NON_INTERACTIVE: i32 = 7;
    /// Interrupted with Ctrl-C (128 + SIGINT).
    pub(crate) const INTERRUPTED: i32 = 130;
}

/// Extension trait to add context to Results.
///
/// This trait provides a convenient way to add context to any Result
/// with an error type that implements `std::error::Error + Send + Sync`.
pub(crate) trait ResultExt<T> {
    /// Add context to an error result.
    ///
    /// Converts any error into a `CliError::WithContext` with the given message.
    fn context(self, message: impl Into<String>) -> CliResult<T>;

    /// Add context to an error result using a closure.
    ///
    /// The closure is only called if there is an error, which can be
    /// more efficient when constructing the context message is expensive.
    fn with_context<F>(self, f: F) -> CliResult<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> CliResult<T> {
        self.map_err(|e| CliError::context(e, message))
    }

    fn with_context<F>(self, f: F) -> CliResult<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| CliError::context(e, f()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CliError>();
    }

    #[test]
    fn error_display() {
        let err = CliError::InvalidProjectName {
            name: "fn".to_string(),
            reason: "reserved keyword".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "invalid project name 'fn': reserved keyword"
        );
    }

    #[test]
    fn error_with_context() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = CliError::context(io_err, "failed to read config");
        assert!(err.to_string().contains("failed to read config"));
    }

    #[test]
    fn result_ext_context() {
        let result: Result<(), std::io::Error> =
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "oops"));
        let cli_result = result.context("operation failed");
        assert!(cli_result.is_err());
    }

    #[test]
    fn exit_codes() {
        assert_eq!(CliError::UserCancelled.exit_code(), 0);
        assert_eq!(CliError::Missing("x".into()).exit_code(), 1);
        assert_eq!(CliError::Usage("bad flag".into()).exit_code(), 2);
        assert_eq!(
            CliError::ToolNotFound {
                tool: "adb".into(),
                suggestion: String::new()
            }
            .exit_code(),
            3
        );
        assert_eq!(
            CliError::EnvironmentCheckFailed {
                failed: 1,
                total: 5
            }
            .exit_code(),
            3
        );
        assert_eq!(CliError::TestsFailed.exit_code(), 4);
        assert_eq!(CliError::build_failed("desktop", "").exit_code(), 4);
        assert_eq!(CliError::FormattingCheckFailed.exit_code(), 4);
        assert_eq!(
            CliError::DeviceNotFound {
                name: "pixel".into(),
                hint: String::new()
            }
            .exit_code(),
            5
        );
        assert_eq!(
            CliError::NotFluiProject {
                reason: String::new()
            }
            .exit_code(),
            6
        );
        assert_eq!(
            CliError::NonInteractive {
                what: "x".into(),
                hint: String::new()
            }
            .exit_code(),
            7
        );
        assert_eq!(CliError::Interrupted.exit_code(), 130);
    }

    #[test]
    fn command_failed_error() {
        let err = CliError::command_failed("cargo build", Some(1));
        assert!(err.to_string().contains("cargo build"));
        assert!(err.to_string().contains('1'));
    }
}
