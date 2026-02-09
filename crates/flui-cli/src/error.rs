//! Error types for the FLUI CLI.
//!
//! This module provides error types following Rust API Guidelines:
//!
//! - **C-GOOD-ERR**: Error types are meaningful and well-behaved
//! - **C-SEND-SYNC**: Error types are Send and Sync
//! - **C-DEBUG**: All types implement Debug
//! - **C-NON-EXHAUSTIVE**: Enum is non-exhaustive for future compatibility
//!
//! # Examples
//!
//! ```ignore
//! use flui_cli::error::{CliError, CliResult, ResultExt};
//!
//! fn do_something() -> CliResult<()> {
//!     std::fs::read_to_string("file.txt")
//!         .context("Failed to read configuration")?;
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for CLI operations.
///
/// This is the standard result type used throughout the CLI.
///
/// # Examples
///
/// ```ignore
/// fn create_project() -> CliResult<()> {
///     // ...
///     Ok(())
/// }
/// ```
pub type CliResult<T> = Result<T, CliError>;

/// Error types that can occur during CLI operations.
///
/// This enum is marked `#[non_exhaustive]` to allow adding new variants
/// in future versions without breaking changes (C-NON-EXHAUSTIVE).
///
/// All variants are designed to be meaningful and provide useful information
/// to the user (C-GOOD-ERR).
///
/// # Send + Sync
///
/// This type implements `Send` and `Sync` (via thiserror), making it safe
/// to use in multi-threaded contexts and with `std::io::Error::new()`.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CliError {
    // ========================================================================
    // Project Creation Errors
    // ========================================================================
    /// Project directory already exists.
    ///
    /// Returned when attempting to create a project in a directory that
    /// already exists.
    #[error("Directory '{path}' already exists")]
    DirectoryExists {
        /// Path to the existing directory
        path: PathBuf,
    },

    /// Invalid project name provided.
    ///
    /// Project names must be valid Rust crate names.
    #[error("Invalid project name '{name}': {reason}")]
    InvalidProjectName {
        /// The invalid project name
        name: String,
        /// Reason why the name is invalid
        reason: String,
    },

    /// Invalid organization identifier provided.
    ///
    /// Organization IDs must be in reverse domain notation (e.g., "com.example").
    #[error("Invalid organization ID '{id}': {reason}")]
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
    #[error("Required tool '{tool}' not found. {suggestion}")]
    ToolNotFound {
        /// Name of the missing tool
        tool: String,
        /// Suggestion for how to install the tool
        suggestion: String,
    },

    /// Not a FLUI project.
    ///
    /// Returned when a command is run outside a FLUI project directory.
    #[error("Not a FLUI project: {reason}")]
    NotFluiProject {
        /// Reason why this is not a FLUI project
        reason: String,
    },

    /// No default device available.
    ///
    /// Returned when no device is available for running the application.
    #[error("No default device for this platform")]
    NoDefaultDevice,

    /// Shell detection failed.
    ///
    /// Returned when the user's shell cannot be automatically detected
    /// for generating completions.
    #[error("Could not detect shell. Please specify explicitly with --shell")]
    ShellDetectionFailed,

    // ========================================================================
    // Build/Run Errors
    // ========================================================================
    /// Build operation failed.
    #[error("Build failed for platform '{platform}': {details}")]
    BuildFailed {
        /// Platform that failed to build
        platform: String,
        /// Details about the build failure
        details: String,
    },

    /// Clean operation failed.
    #[error("Clean failed: {details}")]
    CleanFailed {
        /// Details about the clean failure
        details: String,
    },

    /// Run operation failed.
    #[error("cargo run failed")]
    RunFailed,

    /// Analysis found issues.
    #[error("Analysis found issues")]
    AnalysisFailed,

    /// Test execution failed.
    #[error("Tests failed")]
    TestsFailed,

    // ========================================================================
    // Update/Upgrade Errors
    // ========================================================================
    /// Upgrade operation failed.
    #[error("Upgrade failed")]
    UpgradeFailed,

    /// Update operation failed.
    #[error("Update failed")]
    UpdateFailed,

    // ========================================================================
    // User Interaction Errors
    // ========================================================================
    /// User cancelled interactive operation.
    #[error("Operation cancelled by user")]
    UserCancelled,

    // ========================================================================
    // Format Errors
    // ========================================================================
    /// Code formatting check failed.
    #[error("Code is not formatted. Run 'flui format' to fix.")]
    FormattingCheckFailed,

    /// Code formatting failed.
    #[error("Formatting failed")]
    FormattingFailed,

    // ========================================================================
    // Feature Errors
    // ========================================================================
    /// Feature not yet implemented.
    #[error("{feature} is not yet implemented")]
    NotImplemented {
        /// Name of the unimplemented feature
        feature: String,
    },

    // ========================================================================
    // Wrapped External Errors
    // ========================================================================
    /// I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error.
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error.
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// Build system error.
    #[error("Build system error: {0}")]
    Build(#[from] flui_build::error::BuildError),

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
    #[error("{context} failed with exit code {exit_code:?}")]
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
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    /// let cli_err = CliError::context(err, "Failed to read config");
    /// ```
    pub fn context<E>(err: E, message: impl Into<String>) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::WithContext {
            message: message.into(),
            source: Box::new(err),
        }
    }

    /// Create a new "not implemented" error.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// return Err(CliError::not_implemented("iOS builds"));
    /// ```
    pub fn not_implemented(feature: impl Into<String>) -> Self {
        Self::NotImplemented {
            feature: feature.into(),
        }
    }

    /// Create a new "tool not found" error.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// return Err(CliError::tool_not_found("cargo", "Install Rust from rustup.rs"));
    /// ```
    pub fn tool_not_found(tool: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::ToolNotFound {
            tool: tool.into(),
            suggestion: suggestion.into(),
        }
    }

    /// Create a new "build failed" error.
    pub fn build_failed(platform: impl Into<String>, details: impl Into<String>) -> Self {
        Self::BuildFailed {
            platform: platform.into(),
            details: details.into(),
        }
    }

    /// Create a new "command failed" error.
    pub fn command_failed(context: impl Into<String>, exit_code: Option<i32>) -> Self {
        Self::CommandFailed {
            context: context.into(),
            exit_code,
        }
    }

    /// Check if this error is recoverable.
    ///
    /// Some errors (like user cancellation) are expected and recoverable,
    /// while others indicate actual failures.
    #[must_use]
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::UserCancelled | Self::FormattingCheckFailed)
    }

    /// Get exit code for this error.
    ///
    /// Returns an appropriate exit code for use with `std::process::exit()`.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UserCancelled => 0,
            Self::NotImplemented { .. } => 2,
            _ => 1,
        }
    }
}

/// Extension trait to add context to Results.
///
/// This trait provides a convenient way to add context to any Result
/// with an error type that implements `std::error::Error + Send + Sync`.
///
/// # Examples
///
/// ```ignore
/// use flui_cli::error::ResultExt;
///
/// fn read_config() -> CliResult<String> {
///     std::fs::read_to_string("config.toml")
///         .context("Failed to read configuration file")
/// }
/// ```
pub trait ResultExt<T> {
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

/// Extension trait for Option to convert to `CliError`.
///
/// # Examples
///
/// ```ignore
/// use flui_cli::error::OptionExt;
///
/// fn get_home() -> CliResult<PathBuf> {
///     dirs::home_dir().ok_or_context("Could not find home directory")
/// }
/// ```
pub trait OptionExt<T> {
    /// Convert None to a `CliError` with the given message.
    fn ok_or_context(self, message: impl Into<String>) -> CliResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_context(self, message: impl Into<String>) -> CliResult<T> {
        self.ok_or_else(|| CliError::Missing(message.into()))
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
            "Invalid project name 'fn': reserved keyword"
        );
    }

    #[test]
    fn error_with_context() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = CliError::context(io_err, "Failed to read config");
        assert!(err.to_string().contains("Failed to read config"));
    }

    #[test]
    fn result_ext_context() {
        let result: Result<(), std::io::Error> =
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "oops"));
        let cli_result = result.context("Operation failed");
        assert!(cli_result.is_err());
    }

    #[test]
    fn is_recoverable() {
        assert!(CliError::UserCancelled.is_recoverable());
        assert!(CliError::FormattingCheckFailed.is_recoverable());
        assert!(!CliError::TestsFailed.is_recoverable());
    }

    #[test]
    fn exit_codes() {
        assert_eq!(CliError::UserCancelled.exit_code(), 0);
        assert_eq!(CliError::TestsFailed.exit_code(), 1);
        assert_eq!(CliError::not_implemented("test").exit_code(), 2);
    }

    #[test]
    fn command_failed_error() {
        let err = CliError::command_failed("cargo build", Some(1));
        assert!(err.to_string().contains("cargo build"));
        assert!(err.to_string().contains("1"));
    }
}
