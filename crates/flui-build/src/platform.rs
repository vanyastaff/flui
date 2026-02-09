use std::path::PathBuf;

use crate::error::{BuildError, BuildResult};

/// Private module to seal the `PlatformBuilder` trait.
///
/// This prevents external implementations of `PlatformBuilder`,
/// allowing us to add methods to the trait in the future without
/// breaking changes.
pub(crate) mod private {
    pub trait Sealed {}
}

/// Build context containing configuration and paths.
///
/// Use [`BuilderContextBuilder`](crate::BuilderContextBuilder) to construct instances.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BuilderContext {
    /// Root directory of the workspace
    pub workspace_root: PathBuf,
    /// Target platform to build for
    pub platform: Platform,
    /// Build profile (debug or release)
    pub profile: Profile,
    /// Cargo features to enable
    pub features: Vec<String>,
    /// Output directory for build artifacts
    pub output_dir: PathBuf,
}

/// Platform to build for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Android platform with target architectures
    Android {
        /// Target architectures (e.g., "aarch64-linux-android")
        targets: Vec<String>,
    },
    /// iOS platform with target architectures
    IOS {
        /// Target architectures (e.g., "aarch64-apple-ios", "x86_64-apple-ios")
        targets: Vec<String>,
    },
    /// Web/WASM platform
    Web {
        /// Target identifier (e.g., "web")
        target: String,
    },
    /// Desktop platform (Windows, macOS, Linux)
    Desktop {
        /// Optional target triple (auto-detected if None)
        target: Option<String>,
    },
}

impl Platform {
    /// Returns the platform name as a string
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Platform::Android { .. } => "android",
            Platform::IOS { .. } => "ios",
            Platform::Web { .. } => "web",
            Platform::Desktop { .. } => "desktop",
        }
    }
}

/// Build profile (debug or release)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Profile {
    /// Debug profile (default) - faster compilation, includes debug symbols
    #[default]
    Debug,
    /// Release profile - optimized, slower compilation
    Release,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl Profile {
    /// Returns the cargo flag for this profile
    ///
    /// Returns `None` for Debug (default), `Some("--release")` for Release
    #[must_use]
    pub fn cargo_flag(&self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

    /// Returns the profile name as a string
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

impl From<&str> for Profile {
    fn from(s: &str) -> Self {
        match s {
            "release" => Profile::Release,
            _ => Profile::Debug,
        }
    }
}

impl TryFrom<&str> for Platform {
    type Error = BuildError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "android" => Ok(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            }),
            "ios" => Ok(Platform::IOS {
                targets: vec!["aarch64-apple-ios".to_string()],
            }),
            "web" => Ok(Platform::Web {
                target: "web".to_string(),
            }),
            "desktop" => Ok(Platform::Desktop { target: None }),
            other => Err(BuildError::InvalidPlatform {
                reason: format!("unknown platform '{other}', expected: android, ios, web, desktop"),
            }),
        }
    }
}

/// Build artifacts produced by Rust compilation
#[derive(Debug)]
pub struct BuildArtifacts {
    /// Paths to compiled Rust libraries (.so, .dll, .dylib, .wasm)
    pub rust_libs: Vec<PathBuf>,
    /// Platform-specific metadata (JSON)
    pub metadata: serde_json::Value,
}

/// Final artifacts after platform-specific build
#[derive(Debug)]
pub struct FinalArtifacts {
    /// Path to the final application binary (APK, WASM, executable, etc.)
    pub app_binary: PathBuf,
    /// Size of the final artifact in bytes
    pub size_bytes: u64,
}

/// Platform-specific builder trait.
///
/// This trait is sealed and cannot be implemented outside of `flui_build`.
/// Only the built-in builders (`AndroidBuilder`, `WebBuilder`, `DesktopBuilder`)
/// implement this trait.
///
/// # Sealed Trait
///
/// This trait is sealed using the [sealed trait pattern](https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed).
/// External crates cannot implement this trait, which allows us to add methods
/// in the future without breaking changes.
// Sealed trait — only implemented within this crate, so Send bounds on futures are guaranteed.
#[allow(async_fn_in_trait)]
pub trait PlatformBuilder: private::Sealed + Send + Sync {
    /// Platform name
    fn platform_name(&self) -> &str;

    /// Validate environment (check tools, SDK, etc.)
    fn validate_environment(&self) -> BuildResult<()>;

    /// Build Rust libraries
    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts>;

    /// Build platform-specific artifacts (APK, WASM, etc.)
    async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts>;

    /// Clean build artifacts
    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()>;
}
