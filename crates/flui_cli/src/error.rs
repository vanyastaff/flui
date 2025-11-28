//! Error types for the FLUI CLI.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for CLI operations.
pub type CliResult<T> = Result<T, CliError>;

/// Error types that can occur during CLI operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CliError {
    /// Project directory already exists.
    #[error("Directory '{path}' already exists")]
    DirectoryExists { path: PathBuf },

    /// Invalid project name provided.
    #[error("Invalid project name '{name}': {reason}")]
    InvalidProjectName { name: String, reason: String },

    /// Required tool not found on system.
    #[error("Required tool '{tool}' not found. {suggestion}")]
    ToolNotFound { tool: String, suggestion: String },

    /// Build operation failed.
    #[error("Build failed for platform '{platform}': {details}")]
    BuildFailed { platform: String, details: String },

    /// Clean operation failed.
    #[error("Clean failed: {details}")]
    CleanFailed { details: String },

    /// Analysis found issues.
    #[error("Analysis found {count} issue(s)")]
    AnalysisIssues { count: usize },

    /// Shell detection failed.
    #[error("Could not detect shell. Please specify explicitly with --shell")]
    ShellDetection,

    /// User cancelled interactive operation.
    #[error("Operation cancelled by user")]
    UserCancelled,

    /// Feature not yet implemented.
    #[error("{feature} is not yet implemented")]
    NotImplemented { feature: String },

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
    #[error("{message}")]
    WithContext {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl CliError {
    /// Add context to an error.
    pub fn context<E>(err: E, message: impl Into<String>) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::WithContext {
            message: message.into(),
            source: Box::new(err),
        }
    }
}

/// Extension trait to add context to Results.
pub trait ResultExt<T> {
    /// Add context to an error result.
    fn context(self, message: impl Into<String>) -> CliResult<T>;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> CliResult<T> {
        self.map_err(|e| CliError::context(e, message))
    }
}
