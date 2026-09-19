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
    /// Which cargo package or example to compile.
    pub target: BuildUnit,
    /// Build profile (debug or release)
    pub profile: Profile,
    /// Cargo features to enable
    pub features: Vec<String>,
    /// Output directory for build artifacts
    pub output_dir: PathBuf,
    /// Application bundle metadata, when the target stages a platform bundle.
    ///
    /// `None` keeps the backend's plain-artifact behaviour (a bare executable
    /// copy). On macOS a `Some` stages a `.app` whose `Info.plist` names the
    /// application — the bundle a double-clickable, foreground-activatable app
    /// needs, and which a bare Mach-O cannot substitute for.
    pub bundle: Option<AppBundle>,
}

/// Identity an application bundle is staged under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBundle {
    /// Human-readable application name (the `.app` stem and `CFBundleName`).
    pub name: String,
    /// Reverse-DNS bundle identifier (`CFBundleIdentifier`).
    pub identifier: String,
}

impl AppBundle {
    /// Create bundle metadata from a display name and an org id.
    ///
    /// The identifier is `{identifier_prefix}.{slug}` with the name slugged to
    /// lowercase alphanumerics — the shape a bundle id may legally take, so a
    /// name like `My App` does not produce an invalid plist value.
    #[must_use]
    pub fn new(name: &str, identifier_prefix: &str) -> Self {
        let slug: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let slug = slug.trim_matches('-').to_string();
        Self {
            name: name.to_string(),
            identifier: format!("{identifier_prefix}.{slug}"),
        }
    }
}

/// What cargo should compile within the workspace.
///
/// A desktop build produces an *executable*, and which executable differs by
/// call site: a generated application project has one binary package, while the
/// FLUI source tree is a workspace whose runnable entry points are examples.
/// This is the knob that keeps `DesktopBuilder` from hard-coding one of them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum BuildUnit {
    /// Build the current package's default binary (`cargo build` in
    /// `workspace_root`).
    #[default]
    DefaultBinary,
    /// Build the named workspace package's binary (`cargo build -p NAME`).
    Package(String),
    /// Build the named example of the current package
    /// (`cargo build --example NAME`).
    Example(String),
}

impl BuildUnit {
    /// The cargo arguments that select this target.
    #[must_use]
    pub fn cargo_args(&self) -> Vec<String> {
        match self {
            Self::DefaultBinary => Vec::new(),
            Self::Package(name) => vec!["-p".to_string(), name.clone()],
            Self::Example(name) => vec!["--example".to_string(), name.clone()],
        }
    }
}

/// Platform to build for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Platform {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    /// Path to a compiled executable, when the target produces one.
    ///
    /// Desktop builds produce an executable; mobile/web builds produce
    /// libraries consumed by a platform bundle step and leave this `None`.
    pub executable: Option<PathBuf>,
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
#[expect(async_fn_in_trait)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_target_default_is_the_current_packages_binary() {
        assert_eq!(BuildUnit::default(), BuildUnit::DefaultBinary);
        assert!(BuildUnit::DefaultBinary.cargo_args().is_empty());
    }

    #[test]
    fn build_target_maps_to_its_cargo_selector() {
        assert_eq!(
            BuildUnit::Package("my-app".to_string()).cargo_args(),
            vec!["-p".to_string(), "my-app".to_string()]
        );
        assert_eq!(
            BuildUnit::Example("material_demo".to_string()).cargo_args(),
            vec!["--example".to_string(), "material_demo".to_string()]
        );
    }
}
