use std::path::PathBuf;

use crate::build::error::BuildError;

/// Build context containing configuration and paths.
///
/// Use [`BuilderContextBuilder`](crate::build::BuilderContextBuilder) to construct instances.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub(crate) struct BuilderContext {
    /// Root directory of the workspace
    pub(crate) workspace_root: PathBuf,
    /// Target platform to build for
    pub(crate) platform: Platform,
    /// Which cargo package or example to compile.
    pub(crate) target: BuildUnit,
    /// Build profile (debug or release)
    pub(crate) profile: Profile,
    /// Output directory for build artifacts
    pub(crate) output_dir: PathBuf,
    /// Application bundle metadata, when the target stages a platform bundle.
    ///
    /// `None` keeps the backend's plain-artifact behaviour (a bare executable
    /// copy). On macOS a `Some` stages a `.app` whose `Info.plist` names the
    /// application — the bundle a double-clickable, foreground-activatable app
    /// needs, and which a bare Mach-O cannot substitute for.
    pub(crate) bundle: Option<AppBundle>,
}

/// Identity an application bundle is staged under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppBundle {
    /// Human-readable application name (the `.app` stem and `CFBundleName`).
    pub(crate) name: String,
    /// Reverse-DNS bundle identifier (`CFBundleIdentifier`).
    pub(crate) identifier: String,
    /// Full application SemVer. Apple bundles store its numeric core separately.
    pub(crate) version: String,
}

impl AppBundle {
    /// Create bundle metadata from a display name and an org id.
    ///
    /// The identifier is `{identifier_prefix}.{slug}` with the name slugged to
    /// lowercase alphanumerics — the shape a bundle id may legally take, so a
    /// name like `My App` does not produce an invalid plist value.
    #[must_use]
    pub(crate) fn new(name: &str, identifier_prefix: &str) -> Self {
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
            version: "0.1.0".into(),
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
pub(crate) enum BuildUnit {
    /// Build the current package's default binary (`cargo build` in `workspace_root`).
    #[default]
    DefaultBinary,
    /// Build the named workspace package's binary.
    Package(String),
    /// Build the named example of the current package
    /// (`cargo build --example NAME`).
    Example(String),
    /// Build a static library for iOS XCFramework delivery.
    Library {
        /// Workspace package name, or current/default package selection.
        package: Option<String>,
    },
}

/// Platform to build for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    /// Android platform with target architectures
    Android {
        /// Target architectures (e.g., "aarch64-linux-android")
        targets: Vec<String>,
    },
    /// iOS platform with target architectures
    Ios {
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
    pub(crate) fn name(&self) -> &str {
        match self {
            Platform::Android { .. } => "android",
            Platform::Ios { .. } => "ios",
            Platform::Web { .. } => "web",
            Platform::Desktop { .. } => "desktop",
        }
    }
}

/// Build profile (debug or release)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Profile {
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
    pub(crate) fn cargo_flag(&self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

    /// Returns the profile name as a string
    #[must_use]
    pub(crate) fn as_str(&self) -> &'static str {
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
            "ios" => Ok(Platform::Ios {
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
pub(crate) struct BuildArtifacts {
    /// Paths to compiled Rust libraries (.a, .so, .dll, .dylib, .wasm)
    pub(crate) rust_libs: Vec<PathBuf>,
    /// Path to a compiled executable, when the target produces one.
    ///
    /// Desktop builds and executable iOS examples produce an executable. Other mobile/web builds produce
    /// libraries consumed by a platform bundle step and leave this `None`.
    pub(crate) executable: Option<PathBuf>,
    /// Platform-specific metadata (JSON)
    pub(crate) metadata: serde_json::Value,
}

/// Delivered file or bundle directory after a platform-specific build.
#[derive(Debug)]
pub(crate) struct FinalArtifacts {
    /// Path to the delivered artifact: a file (APK, WASM, executable, etc.) or
    /// a bundle directory (`.app` or `.xcframework`). iOS library delivery
    /// without a consumer Xcode project returns an XCFramework directory.
    pub(crate) app_binary: PathBuf,
    /// File size, or the sum of contained file sizes for a bundle, in bytes.
    pub(crate) size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_target_default_is_the_current_packages_binary() {
        assert_eq!(BuildUnit::default(), BuildUnit::DefaultBinary);
    }
}
