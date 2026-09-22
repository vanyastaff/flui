/// Custom error types for the `build` module.
///
/// This module provides type-safe error handling with detailed error messages
/// and context. All build errors are represented by the `BuildError` enum.
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during the build process.
///
/// This enum covers all error conditions in the build system, from
/// missing tools to failed builds. All variants include enough context
/// to provide actionable error messages.
///
/// # Examples
///
/// ## Tool Not Found
///
/// ## Command Failed
#[derive(Debug, Error)]
#[non_exhaustive]
pub(crate) enum BuildError {
    /// Required tool not found (cargo, wasm-pack, gradle, etc.)
    #[error("{tool} not found; install with: {install_hint}")]
    ToolNotFound {
        /// Name of the missing tool
        tool: String,
        /// Installation instructions
        install_hint: String,
    },

    /// Platform target not installed
    #[error("Rust target '{target}' is not installed; install with: {install_cmd}")]
    TargetNotInstalled {
        /// Rust target triple (e.g., "aarch64-linux-android")
        target: String,
        /// Command to install the target
        install_cmd: String,
    },

    /// Environment variable missing or invalid
    #[error("environment variable {var}: {reason}")]
    EnvVarError {
        /// Environment variable name (e.g., "`ANDROID_HOME`")
        var: String,
        /// Error description
        reason: String,
    },

    /// Build command failed
    #[error("`{command}` failed with exit code {exit_code}: {stderr}")]
    CommandFailed {
        /// Command that was executed
        command: String,
        /// Exit code from the command
        exit_code: i32,
        /// Error output from stderr
        stderr: String,
    },

    /// File or directory not found
    #[error("path not found: {} ({context})", .path.display())]
    PathNotFound {
        /// Path that was not found
        path: PathBuf,
        /// Context explaining what was being looked for
        context: String,
    },

    /// Invalid platform configuration
    #[error("invalid platform: {reason}")]
    InvalidPlatform {
        /// Error description
        reason: String,
    },

    /// Invalid build configuration
    #[error("invalid config for '{field}': {reason}")]
    InvalidConfig {
        /// Field name that is invalid
        field: String,
        /// Error description
        reason: String,
    },

    /// I/O error occurred
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// Other error with custom message
    #[error("{0}")]
    Other(String),
}

// Conversion from String for convenience
impl From<String> for BuildError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

// Conversion from &str for convenience
impl From<&str> for BuildError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

/// Result type alias for build operations.
///
/// This is a convenience alias for `Result<T, BuildError>`.
pub(crate) type BuildResult<T> = Result<T, BuildError>;

impl BuildError {
    /// Create a `PathNotFound` error.
    pub(crate) fn path_not_found(path: PathBuf, context: impl Into<String>) -> Self {
        Self::PathNotFound {
            path,
            context: context.into(),
        }
    }

    /// Create an `InvalidPlatform` error.
    pub(crate) fn invalid_platform(reason: impl Into<String>) -> Self {
        Self::InvalidPlatform {
            reason: reason.into(),
        }
    }

    /// Create an `InvalidConfig` error.
    pub(crate) fn invalid_config(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidConfig {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_failed_display() {
        let error = BuildError::CommandFailed {
            command: "cargo build".into(),
            exit_code: 1,
            stderr: "error: compilation failed".into(),
        };
        let msg = format!("{error}");
        assert!(msg.contains("cargo build"));
        assert!(msg.contains("exit code 1"));
        assert!(msg.contains("compilation failed"));
    }

    #[test]
    fn test_path_not_found_display() {
        let error = BuildError::path_not_found(PathBuf::from("Cargo.toml"), "workspace root");
        let msg = format!("{error}");
        assert!(msg.contains("Cargo.toml"));
        assert!(msg.contains("workspace root"));
    }

    #[test]
    fn test_invalid_platform_display() {
        let error = BuildError::invalid_platform("unsupported architecture");
        let msg = format!("{error}");
        assert!(msg.contains("unsupported architecture"));
    }

    #[test]
    fn test_invalid_config_display() {
        let error = BuildError::invalid_config("output_dir", "does not exist");
        let msg = format!("{error}");
        assert!(msg.contains("output_dir"));
        assert!(msg.contains("does not exist"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let build_err: BuildError = io_err.into();
        assert!(matches!(build_err, BuildError::Io(_)));
    }

    #[test]
    fn test_from_string() {
        let build_err: BuildError = "custom error".to_string().into();
        assert!(matches!(build_err, BuildError::Other(_)));
    }

    #[test]
    fn test_from_str() {
        let build_err: BuildError = "custom error".into();
        assert!(matches!(build_err, BuildError::Other(_)));
    }
}
