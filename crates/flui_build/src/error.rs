/// Custom error types for the flui_build crate.
///
/// This module provides type-safe error handling with detailed error messages
/// and context. All build errors are represented by the `BuildError` enum.
///
/// # Example
///
/// ```rust
/// use flui_build::{BuildError, BuildResult};
/// use std::path::PathBuf;
///
/// fn check_tool() -> BuildResult<()> {
///     Err(BuildError::ToolNotFound {
///         tool: "cargo-ndk".to_string(),
///         install_hint: "cargo install cargo-ndk".to_string(),
///     })
/// }
///
/// match check_tool() {
///     Err(BuildError::ToolNotFound { tool, install_hint }) => {
///         println!("{} not found. Install with: {}", tool, install_hint);
///     }
///     _ => {}
/// }
/// ```
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

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
/// ```rust
/// use flui_build::BuildError;
///
/// let error = BuildError::ToolNotFound {
///     tool: "wasm-pack".to_string(),
///     install_hint: "cargo install wasm-pack".to_string(),
/// };
///
/// println!("{}", error);
/// // Output: wasm-pack not found. Install with: cargo install wasm-pack
/// ```
///
/// ## Command Failed
///
/// ```rust
/// use flui_build::BuildError;
///
/// let error = BuildError::CommandFailed {
///     command: "cargo build".to_string(),
///     exit_code: 1,
///     stderr: "error: could not compile".to_string(),
/// };
///
/// println!("{}", error);
/// // Output: Command 'cargo build' failed with exit code 1
/// //         Stderr: error: could not compile
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum BuildError {
    /// Required tool not found (cargo, wasm-pack, gradle, etc.)
    ToolNotFound {
        /// Name of the missing tool
        tool: String,
        /// Installation instructions
        install_hint: String,
    },

    /// Platform target not installed
    TargetNotInstalled {
        /// Rust target triple (e.g., "aarch64-linux-android")
        target: String,
        /// Command to install the target
        install_cmd: String,
    },

    /// Environment variable missing or invalid
    EnvVarError {
        /// Environment variable name (e.g., "ANDROID_HOME")
        var: String,
        /// Error description
        reason: String,
    },

    /// Build command failed
    CommandFailed {
        /// Command that was executed
        command: String,
        /// Exit code from the command
        exit_code: i32,
        /// Error output from stderr
        stderr: String,
    },

    /// File or directory not found
    PathNotFound {
        /// Path that was not found
        path: PathBuf,
        /// Context explaining what was being looked for
        context: String,
    },

    /// Invalid platform configuration
    InvalidPlatform {
        /// Error description
        reason: String,
    },

    /// Invalid build configuration
    InvalidConfig {
        /// Field name that is invalid
        field: String,
        /// Error description
        reason: String,
    },

    /// I/O error occurred
    Io(std::io::Error),

    /// Other error with custom message
    Other(String),
}

impl Display for BuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolNotFound { tool, install_hint } => {
                write!(f, "{} not found. Install with: {}", tool, install_hint)
            }
            Self::TargetNotInstalled { target, install_cmd } => write!(
                f,
                "Rust target '{}' not installed. Install with: {}",
                target, install_cmd
            ),
            Self::EnvVarError { var, reason } => {
                write!(f, "Environment variable {} error: {}", var, reason)
            }
            Self::CommandFailed {
                command,
                exit_code,
                stderr,
            } => write!(
                f,
                "Command '{}' failed with exit code {}\nStderr: {}",
                command, exit_code, stderr
            ),
            Self::PathNotFound { path, context } => {
                write!(f, "Path not found: {} ({})", path.display(), context)
            }
            Self::InvalidPlatform { reason } => write!(f, "Invalid platform: {}", reason),
            Self::InvalidConfig { field, reason } => {
                write!(f, "Invalid config for '{}': {}", field, reason)
            }
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl Error for BuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

// Conversion from std::io::Error
impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
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
///
/// # Examples
///
/// ```rust
/// use flui_build::{BuildResult, BuildError};
///
/// fn check_environment() -> BuildResult<()> {
///     // Check environment...
///     Ok(())
/// }
///
/// fn build_project() -> BuildResult<String> {
///     check_environment()?;
///     Ok("Success".to_string())
/// }
/// ```
pub type BuildResult<T> = Result<T, BuildError>;

// Helper functions for creating common errors
impl BuildError {
    /// Create a ToolNotFound error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::tool_not_found("cargo-ndk", "cargo install cargo-ndk");
    /// ```
    pub fn tool_not_found(tool: impl Into<String>, install_hint: impl Into<String>) -> Self {
        Self::ToolNotFound {
            tool: tool.into(),
            install_hint: install_hint.into(),
        }
    }

    /// Create a TargetNotInstalled error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::target_not_installed(
    ///     "aarch64-linux-android",
    ///     "rustup target add aarch64-linux-android"
    /// );
    /// ```
    pub fn target_not_installed(
        target: impl Into<String>,
        install_cmd: impl Into<String>,
    ) -> Self {
        Self::TargetNotInstalled {
            target: target.into(),
            install_cmd: install_cmd.into(),
        }
    }

    /// Create an EnvVarError.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::env_var_error("ANDROID_HOME", "not set");
    /// ```
    pub fn env_var_error(var: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::EnvVarError {
            var: var.into(),
            reason: reason.into(),
        }
    }

    /// Create a CommandFailed error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::command_failed("cargo build", 1, "compilation error");
    /// ```
    pub fn command_failed(
        command: impl Into<String>,
        exit_code: i32,
        stderr: impl Into<String>,
    ) -> Self {
        Self::CommandFailed {
            command: command.into(),
            exit_code,
            stderr: stderr.into(),
        }
    }

    /// Create a PathNotFound error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    /// use std::path::PathBuf;
    ///
    /// let error = BuildError::path_not_found(
    ///     PathBuf::from("Cargo.toml"),
    ///     "looking for workspace root"
    /// );
    /// ```
    pub fn path_not_found(path: PathBuf, context: impl Into<String>) -> Self {
        Self::PathNotFound {
            path,
            context: context.into(),
        }
    }

    /// Create an InvalidPlatform error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::invalid_platform("unsupported target architecture");
    /// ```
    pub fn invalid_platform(reason: impl Into<String>) -> Self {
        Self::InvalidPlatform {
            reason: reason.into(),
        }
    }

    /// Create an InvalidConfig error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::BuildError;
    ///
    /// let error = BuildError::invalid_config("output_dir", "path does not exist");
    /// ```
    pub fn invalid_config(field: impl Into<String>, reason: impl Into<String>) -> Self {
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
    fn test_tool_not_found_display() {
        let error = BuildError::tool_not_found("cargo-ndk", "cargo install cargo-ndk");
        let msg = format!("{}", error);
        assert!(msg.contains("cargo-ndk"));
        assert!(msg.contains("cargo install"));
    }

    #[test]
    fn test_target_not_installed_display() {
        let error = BuildError::target_not_installed(
            "aarch64-linux-android",
            "rustup target add aarch64-linux-android",
        );
        let msg = format!("{}", error);
        assert!(msg.contains("aarch64-linux-android"));
        assert!(msg.contains("rustup target add"));
    }

    #[test]
    fn test_env_var_error_display() {
        let error = BuildError::env_var_error("ANDROID_HOME", "not set");
        let msg = format!("{}", error);
        assert!(msg.contains("ANDROID_HOME"));
        assert!(msg.contains("not set"));
    }

    #[test]
    fn test_command_failed_display() {
        let error = BuildError::command_failed("cargo build", 1, "error: compilation failed");
        let msg = format!("{}", error);
        assert!(msg.contains("cargo build"));
        assert!(msg.contains("exit code 1"));
        assert!(msg.contains("compilation failed"));
    }

    #[test]
    fn test_path_not_found_display() {
        let error = BuildError::path_not_found(PathBuf::from("Cargo.toml"), "workspace root");
        let msg = format!("{}", error);
        assert!(msg.contains("Cargo.toml"));
        assert!(msg.contains("workspace root"));
    }

    #[test]
    fn test_invalid_platform_display() {
        let error = BuildError::invalid_platform("unsupported architecture");
        let msg = format!("{}", error);
        assert!(msg.contains("unsupported architecture"));
    }

    #[test]
    fn test_invalid_config_display() {
        let error = BuildError::invalid_config("output_dir", "does not exist");
        let msg = format!("{}", error);
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

    #[test]
    fn test_build_result() {
        fn success() -> BuildResult<i32> {
            Ok(42)
        }

        fn failure() -> BuildResult<i32> {
            Err(BuildError::tool_not_found("test", "install test"))
        }

        assert!(success().is_ok());
        assert!(failure().is_err());
    }
}
