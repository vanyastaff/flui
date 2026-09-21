//! Configuration management for FLUI projects and CLI.
//!
//! These configuration types are part of the public API but are not yet
//! used by all commands. They are intended for future enhancements.
#![expect(
    dead_code,
    reason = "config types prepared for future command integration"
)]
//!
//! This module provides configuration types following Rust API Guidelines:
//!
//! - **C-COMMON-TRAITS**: All types implement Debug, Clone
//! - **C-SERDE**: Data structures implement Serialize/Deserialize
//! - **C-DEFAULT**: Types with sensible defaults implement Default
//! - **C-STRUCT-PRIVATE**: Internal state is encapsulated where appropriate
//!
//! # Configuration Files
//!
//! - **Project config** (`flui.toml`): Project-specific settings
//!
//! There is deliberately no global (per-user) configuration file: the CLI
//! keeps no state about the user and has no telemetry to configure.
//!
//! # Examples
//!
//! ```ignore
//! use flui_cli::config::FluiConfig;
//!
//! // Load project configuration
//! let config = FluiConfig::load()?;
//! println!("Project: {}", config.app.name);
//! ```

use crate::error::{CliError, CliResult, ResultExt};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ============================================================================
// Project Configuration (flui.toml)
// ============================================================================

/// FLUI project configuration.
///
/// This struct represents the contents of `flui.toml` at the project root.
///
/// # File Format
///
/// ```toml
/// [app]
/// name = "my-app"
/// version = "0.1.0"
/// organization = "com.example"
///
/// [build]
/// target_platforms = ["windows", "linux", "macos"]
///
/// [assets]
/// directories = ["assets"]
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FluiConfig {
    /// Application metadata.
    pub app: AppConfig,
    /// Build configuration.
    #[serde(default)]
    pub build: BuildConfig,
    /// Asset configuration.
    #[serde(default)]
    pub assets: AssetsConfig,
    /// Custom font families.
    #[serde(default)]
    pub fonts: Vec<FontFamily>,
    /// Flutter-parity host/worker hot reload (optional).
    #[serde(default)]
    pub hot_reload: Option<HotReloadConfig>,
}

impl FluiConfig {
    /// Load configuration from `flui.toml` in the current directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `flui.toml` doesn't exist (not a FLUI project)
    /// - File cannot be read
    /// - TOML parsing fails
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let config = FluiConfig::load()?;
    /// println!("Building: {}", config.app.name);
    /// ```
    pub fn load() -> CliResult<Self> {
        Self::load_from(Path::new("flui.toml"))
    }

    /// Load configuration from a specific path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file doesn't exist or cannot be parsed.
    pub fn load_from(path: &Path) -> CliResult<Self> {
        if !path.exists() {
            return Err(CliError::NotFluiProject {
                reason: format!("{} not found", path.display()),
            });
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Save configuration to `flui.toml` in the specified directory.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file writing fails.
    pub fn save(&self, dir: &Path) -> CliResult<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(dir.join("flui.toml"), content).context("Failed to write flui.toml")?;

        Ok(())
    }

    /// Create a new configuration with the given app settings.
    pub fn new(app: AppConfig) -> Self {
        Self {
            app,
            build: BuildConfig::default(),
            assets: AssetsConfig::default(),
            fonts: Vec::new(),
            hot_reload: None,
        }
    }
}

/// Host/worker hot reload layout (Flutter-parity).
///
/// When present, `flui run` keeps the host process alive, rebuilds only the
/// worker `cdylib` on source changes, and relies on `flui_hot_reload::WorkerReloadDriver` in
/// the host runner to apply `flui_hot_reload::HotReloadTier::HotReload`.
///
/// ```toml
/// [hot_reload]
/// host_package = "my-app-host"
/// worker_package = "my-app-logic"
/// worker_lib = "my_app_logic"
/// logic_watch = "logic/src"
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotReloadConfig {
    /// Host binary crate (`cargo run -p …`).
    pub host_package: String,
    /// Reloadable worker crate (`cargo build -p …`, `cdylib`).
    pub worker_package: String,
    /// `cdylib` library name (artifact: `lib{name}.so` / `{name}.dll`).
    pub worker_lib: String,
    /// Directory to watch for UI changes, relative to this `flui.toml`.
    pub logic_watch: String,
    /// Optional shared types crate — changes trigger a full host restart.
    #[serde(default)]
    pub types_watch: Option<String>,
}

/// Application metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Application name (used as crate name).
    pub name: String,
    /// Application version (semver).
    pub version: String,
    /// Organization identifier (reverse domain notation).
    pub organization: String,
}

impl AppConfig {
    /// Create a new app configuration.
    pub fn new(name: impl Into<String>, organization: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: "0.1.0".to_string(),
            organization: organization.into(),
        }
    }

    /// Get the full application ID.
    ///
    /// Combines organization and name into a single identifier
    /// suitable for mobile platforms.
    pub fn app_id(&self) -> String {
        let sanitized_name = self.name.replace('-', "_");
        format!("{}.{}", self.organization, sanitized_name)
    }
}

/// Build configuration.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Target platforms for this project.
    #[serde(default)]
    pub target_platforms: Vec<String>,
    /// Enable link-time optimization.
    #[serde(default)]
    pub lto: bool,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u8,
    /// Debug build settings.
    #[serde(default)]
    pub debug: Option<BuildModeConfig>,
    /// Profile build settings.
    #[serde(default)]
    pub profile: Option<BuildModeConfig>,
    /// Release build settings.
    #[serde(default)]
    pub release: Option<BuildModeConfig>,
}

/// Build mode specific configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BuildModeConfig {
    /// Enable incremental compilation.
    #[serde(default)]
    pub incremental: bool,
    /// Enable hot reload.
    #[serde(default)]
    pub hot_reload: bool,
    /// Include debug symbols.
    #[serde(default)]
    pub debug: bool,
    /// Override optimization level.
    #[serde(default)]
    pub opt_level: Option<u8>,
    /// Strip symbols from binary.
    #[serde(default)]
    pub strip: bool,
    /// Enable LTO for this mode.
    #[serde(default)]
    pub lto: bool,
}

/// Asset configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AssetsConfig {
    /// Directories containing assets.
    #[serde(default)]
    pub directories: Vec<String>,
    /// Bundle assets into the binary.
    #[serde(default)]
    pub bundle: bool,
}

/// Font family configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontFamily {
    /// Font family name.
    pub family: String,
    /// Font files in this family.
    pub fonts: Vec<FontAsset>,
}

/// Individual font asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontAsset {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Path to font file.
    pub asset: String,
    /// Font weight (100-900).
    pub weight: u16,
    /// Font style ("normal", "italic").
    pub style: String,
}

impl FontAsset {
    /// Create a new font asset.
    pub fn new(asset: impl Into<String>, weight: u16, style: impl Into<String>) -> Self {
        Self {
            asset: asset.into(),
            weight,
            style: style.into(),
        }
    }

    /// Create a normal weight font.
    pub fn normal(asset: impl Into<String>) -> Self {
        Self::new(asset, 400, "normal")
    }

    /// Create a bold font.
    pub fn bold(asset: impl Into<String>) -> Self {
        Self::new(asset, 700, "normal")
    }
}

fn default_opt_level() -> u8 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_app_id() {
        let app = AppConfig::new("my-app", "com.example");
        assert_eq!(app.app_id(), "com.example.my_app");
    }

    #[test]
    fn font_asset_constructors() {
        let normal = FontAsset::normal("fonts/Regular.ttf");
        assert_eq!(normal.weight, 400);
        assert_eq!(normal.style, "normal");

        let bold = FontAsset::bold("fonts/Bold.ttf");
        assert_eq!(bold.weight, 700);
    }

    #[test]
    fn flui_config_serialize() {
        let config = FluiConfig::new(AppConfig::new("test", "com.test"));
        let toml = toml::to_string(&config).unwrap();
        assert!(toml.contains("name = \"test\""));
    }
}
