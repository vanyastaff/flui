use anyhow::Result;
use std::path::PathBuf;

/// Build context containing configuration and paths
#[derive(Debug, Clone)]
pub struct BuilderContext {
    pub workspace_root: PathBuf,
    pub platform: Platform,
    pub profile: Profile,
    pub features: Vec<String>,
    pub output_dir: PathBuf,
}

/// Platform to build for
#[derive(Debug, Clone)]
pub enum Platform {
    Android {
        targets: Vec<String>,
    },
    Web {
        target: String,
    },
    Desktop {
        target: Option<String>,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
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

/// Platform-specific builder trait
pub trait PlatformBuilder: Send + Sync {
    /// Platform name
    fn platform_name(&self) -> &str;

    /// Validate environment (check tools, SDK, etc.)
    fn validate_environment(&self) -> Result<()>;

    /// Build Rust libraries
    fn build_rust(&self, ctx: &BuilderContext) -> Result<BuildArtifacts>;

    /// Build platform-specific artifacts (APK, WASM, etc.)
    fn build_platform(&self, ctx: &BuilderContext, artifacts: &BuildArtifacts) -> Result<FinalArtifacts>;

    /// Clean build artifacts
    fn clean(&self, ctx: &BuilderContext) -> Result<()>;
}
