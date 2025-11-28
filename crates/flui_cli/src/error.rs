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
    DirectoryExists {
        /// Path to the existing directory
        path: PathBuf,
    },

    /// Invalid project name provided.
    #[error("Invalid project name '{name}': {reason}")]
    InvalidProjectName {
        /// The invalid project name
        name: String,
        /// Reason why the name is invalid
        reason: String,
    },

    /// Required tool not found on system.
    #[error("Required tool '{tool}' not found. {suggestion}")]
    ToolNotFound {
        /// Name of the missing tool
        tool: String,
        /// Suggestion for how to install the tool
        suggestion: String,
    },

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

    /// Analysis found issues.
    #[error("Analysis found issues")]
    AnalysisIssues,

    /// Test execution failed.
    #[error("Tests failed")]
    TestsFailed,

    /// Upgrade operation failed.
    #[error("Upgrade failed")]
    UpgradeFailed,

    /// Update operation failed.
    #[error("Update failed")]
    UpdateFailed,

    /// Run operation failed.
    #[error("cargo run failed")]
    RunFailed,

    /// Not a FLUI project.
    #[error("Not a FLUI project: {reason}")]
    NotFluiProject {
        /// Reason why this is not a FLUI project
        reason: String,
    },

    /// No default device available.
    #[error("No default device for this platform")]
    NoDefaultDevice,

    /// Shell detection failed.
    #[error("Could not detect shell. Please specify explicitly with --shell")]
    ShellDetection,

    /// User cancelled interactive operation.
    #[error("Operation cancelled by user")]
    UserCancelled,

    /// Feature not yet implemented.
    #[error("{feature} is not yet implemented")]
    NotImplemented {
        /// Name of the unimplemented feature
        feature: String,
    },

    /// Code formatting check failed.
    #[error("Code is not formatted. Run 'flui format' to fix.")]
    FormattingCheck,

    /// Code formatting failed.
    #[error("Formatting failed")]
    FormattingFailed,

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

    /// Dialog interaction error.
    #[error("Dialog error: {0}")]
    Dialog(#[from] dialoguer::Error),

    /// Generic error with context.
    #[error("{message}")]
    WithContext {
        /// Error message with context
        message: String,
        /// Underlying source error
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

// Allow conversion from anyhow::Error for internal modules
impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> Self {
        let root_cause = err.root_cause();
        Self::WithContext {
            message: root_cause.to_string(),
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                root_cause.to_string(),
            )),
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
