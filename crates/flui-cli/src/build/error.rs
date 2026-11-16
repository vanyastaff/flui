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

    /// The toolchain can build no standard library for the target
    #[error(
        "Rust target '{target}' has no standard library in {toolchain}; install with: \
         {install_cmd}, or build it from source with `[unstable] build-std` in \
         .cargo/config.toml and the `rust-src` component"
    )]
    TargetNotInstalled {
        /// Rust target triple (e.g., "aarch64-linux-android")
        target: String,
        /// The toolchain looked at: its release and sysroot
        toolchain: String,
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

    /// A 64-bit Android library with a loadable segment aligned below 16 KB,
    /// which a 16 KB-page device cannot map and Google Play refuses
    #[error(
        "{} has a loadable segment aligned to {alignment} bytes; 16 KB-page devices need 16384. \
         Link with NDK r28 or later, or add `-C link-arg=-Wl,-z,max-page-size=16384` to the \
         target's rustflags",
        .library.display()
    )]
    PageAlignment {
        /// The library
        library: PathBuf,
        /// The smallest `p_align` of its `PT_LOAD` segments
        alignment: u64,
    },

    /// The key an Android package would be signed with cannot sign it
    #[error("signing the APK: {0}")]
    AndroidSigning(#[from] crate::build::android_signing::AndroidSigningError),

    /// The application's version has no form a package can state
    #[error("flui.toml: [app] {0}")]
    AppVersion(#[from] crate::build::app_version::AppVersionError),

    /// The SDK has no Android platform the APK can be linked against
    #[error(
        "the APK declares targetSdk {needed} (FLUI's, from platform-support.toml), so it links \
         against the android.jar of API {needed} or later; {} has {}; install with: \
         sdkmanager \"platforms;android-{needed}\"",
        .platforms.display(),
        platform_levels(.installed)
    )]
    AndroidPlatformMissing {
        /// The lowest API level whose `android.jar` the APK can link against
        needed: u32,
        /// The API levels installed, highest first
        installed: Vec<u32>,
        /// The SDK's `platforms` directory
        platforms: PathBuf,
    },

    /// Gradle wrote an APK that is not signed, which no device installs
    #[error(
        "Gradle wrote the unsigned {}: the project's app/build.gradle.kts gives its release \
         build type no signing config",
        .apk.display()
    )]
    UnsignedApk {
        /// The APK Gradle wrote
        apk: PathBuf,
    },

    /// I/O error occurred
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// Other error with custom message
    #[error("{0}")]
    Other(String),
}

// Conversion from String for convenience
/// Installed Android API levels as a refusal lists them: `android-35,
/// android-34`, or `no platform` when there is none.
pub(crate) fn platform_levels(levels: &[u32]) -> String {
    if levels.is_empty() {
        return "no platform".to_owned();
    }
    levels
        .iter()
        .map(|level| format!("android-{level}"))
        .collect::<Vec<_>>()
        .join(", ")
}

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
