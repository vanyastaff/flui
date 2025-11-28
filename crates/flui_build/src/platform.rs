use anyhow::Result;
use std::path::PathBuf;

/// Private module to seal the PlatformBuilder trait.
///
/// This prevents external implementations of PlatformBuilder,
/// allowing us to add methods to the trait in the future without
/// breaking changes.
pub(crate) mod private {
    pub trait Sealed {}
}

/// Build context containing configuration and paths
#[derive(Debug, Clone, PartialEq)]
pub struct BuilderContext {
    pub workspace_root: PathBuf,
    pub platform: Platform,
    pub profile: Profile,
    pub features: Vec<String>,
    pub output_dir: PathBuf,
}

/// Platform to build for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Platform {
    Android { targets: Vec<String> },
    Web { target: String },
    Desktop { target: Option<String> },
}

impl Platform {
    pub fn name(&self) -> &str {
        match self {
            Platform::Android { .. } => "android",
            Platform::Web { .. } => "web",
            Platform::Desktop { .. } => "desktop",
        }
    }
}

/// Build profile (debug or release)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Profile {
    #[default]
    Debug,
    Release,
}

impl Profile {
    pub fn cargo_flag(&self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

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

impl From<&str> for Platform {
    fn from(s: &str) -> Self {
        match s {
            "android" => Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            },
            "web" => Platform::Web {
                target: "web".to_string(),
            },
            "desktop" => Platform::Desktop { target: None },
            _ => Platform::Desktop { target: None },
        }
    }
}

/// Build artifacts produced by Rust compilation
#[derive(Debug)]
pub struct BuildArtifacts {
    pub rust_libs: Vec<PathBuf>,
    pub metadata: serde_json::Value,
}

/// Final artifacts after platform-specific build
#[derive(Debug)]
pub struct FinalArtifacts {
    pub app_binary: PathBuf,
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
pub trait PlatformBuilder: private::Sealed + Send + Sync {
    /// Platform name
    fn platform_name(&self) -> &str;

    /// Validate environment (check tools, SDK, etc.)
    fn validate_environment(&self) -> Result<()>;

    /// Build Rust libraries
    fn build_rust(&self, ctx: &BuilderContext) -> Result<BuildArtifacts>;

    /// Build platform-specific artifacts (APK, WASM, etc.)
    fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> Result<FinalArtifacts>;

    /// Clean build artifacts
    fn clean(&self, ctx: &BuilderContext) -> Result<()>;
}
