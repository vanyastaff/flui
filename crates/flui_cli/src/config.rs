//! Configuration management for FLUI projects and CLI.
//!
//! These configuration types are part of the public API but are not yet
//! used by all commands. They are intended for future enhancements.
#![allow(dead_code)]
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
//! - **Global config** (`~/.flui/config.toml`): User-wide settings
//!
//! # Examples
//!
//! ```ignore
//! use flui_cli::config::{FluiConfig, GlobalConfig};
//!
//! // Load project configuration
//! let config = FluiConfig::load()?;
//! println!("Project: {}", config.app.name);
//!
//! // Load global configuration (with defaults if not present)
//! let global = GlobalConfig::load()?;
//! ```

use crate::error::{CliError, CliResult, OptionExt, ResultExt};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
        }
    }
}

/// Application metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
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

// ============================================================================
// Global Configuration (~/.flui/config.toml)
// ============================================================================

/// Global FLUI CLI configuration.
///
/// Stored at `~/.flui/config.toml` (or platform-specific config directory).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// SDK settings.
    #[serde(default)]
    pub sdk: SdkConfig,
    /// Default build settings.
    #[serde(default)]
    pub build: GlobalBuildConfig,
    /// DevTools settings.
    #[serde(default)]
    pub devtools: DevToolsConfig,
    /// Telemetry settings.
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

impl GlobalConfig {
    /// Load global configuration.
    ///
    /// Returns default configuration if the file doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be parsed.
    pub fn load() -> CliResult<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content =
            std::fs::read_to_string(&config_path).context("Failed to read global config")?;

        toml::from_str(&content).context("Failed to parse global config")
    }

    /// Save global configuration.
    ///
    /// Creates the config directory if it doesn't exist.
    pub fn save(&self) -> CliResult<()> {
        let config_path = Self::config_path()?;

        if let Some(config_dir) = config_path.parent() {
            std::fs::create_dir_all(config_dir).context("Failed to create config directory")?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(&config_path, content).context("Failed to write global config")?;

        Ok(())
    }

    /// Get the global config file path.
    ///
    /// Returns `~/.flui/config.toml` on Unix-like systems.
    pub fn config_path() -> CliResult<PathBuf> {
        let home = dirs::home_dir().ok_or_context("Could not find home directory")?;

        Ok(home.join(".flui").join("config.toml"))
    }

    /// Get the FLUI data directory.
    ///
    /// Returns `~/.flui/` on Unix-like systems.
    pub fn data_dir() -> CliResult<PathBuf> {
        let home = dirs::home_dir().ok_or_context("Could not find home directory")?;

        Ok(home.join(".flui"))
    }
}

/// SDK configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdkConfig {
    /// Update channel (stable, beta, dev).
    #[serde(default = "default_channel")]
    pub channel: String,
    /// Custom SDK path.
    pub path: Option<PathBuf>,
}

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            channel: default_channel(),
            path: None,
        }
    }
}

/// Global build configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalBuildConfig {
    /// Number of parallel jobs.
    #[serde(default = "default_jobs")]
    pub jobs: usize,
}

impl Default for GlobalBuildConfig {
    fn default() -> Self {
        Self {
            jobs: default_jobs(),
        }
    }
}

/// DevTools configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevToolsConfig {
    /// Default port for DevTools server.
    #[serde(default = "default_devtools_port")]
    pub port: u16,
    /// Auto-launch DevTools in browser.
    #[serde(default = "default_auto_launch")]
    pub auto_launch: bool,
}

impl Default for DevToolsConfig {
    fn default() -> Self {
        Self {
            port: default_devtools_port(),
            auto_launch: default_auto_launch(),
        }
    }
}

/// Telemetry configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable anonymous usage telemetry.
    #[serde(default)]
    pub enabled: bool,
}

// Default value functions

fn default_channel() -> String {
    "stable".to_string()
}

fn default_jobs() -> usize {
    // Use std::thread::available_parallelism when available
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn default_devtools_port() -> u16 {
    9100
}

fn default_auto_launch() -> bool {
    true
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
    fn global_config_default() {
        let config = GlobalConfig::default();
        assert_eq!(config.sdk.channel, "stable");
        assert_eq!(config.devtools.port, 9100);
    }

    #[test]
    fn flui_config_serialize() {
        let config = FluiConfig::new(AppConfig::new("test", "com.test"));
        let toml = toml::to_string(&config).unwrap();
        assert!(toml.contains("name = \"test\""));
    }
}
