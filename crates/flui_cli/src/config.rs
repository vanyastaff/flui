use crate::error::{CliError, CliResult, ResultExt};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// FLUI project configuration (flui.toml)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FluiConfig {
    pub app: AppConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub assets: AssetsConfig,
    #[serde(default)]
    pub fonts: Vec<FontFamily>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub version: String,
    pub organization: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildConfig {
    #[serde(default)]
    pub target_platforms: Vec<String>,
    #[serde(default)]
    pub lto: bool,
    #[serde(default = "default_opt_level")]
    pub opt_level: u8,
    #[serde(default)]
    pub debug: Option<BuildModeConfig>,
    #[serde(default)]
    pub profile: Option<BuildModeConfig>,
    #[serde(default)]
    pub release: Option<BuildModeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildModeConfig {
    #[serde(default)]
    pub incremental: bool,
    #[serde(default)]
    pub hot_reload: bool,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub opt_level: Option<u8>,
    #[serde(default)]
    pub strip: bool,
    #[serde(default)]
    pub lto: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetsConfig {
    #[serde(default)]
    pub directories: Vec<String>,
    #[serde(default)]
    pub bundle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontFamily {
    pub family: String,
    pub fonts: Vec<FontAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontAsset {
    pub asset: String,
    pub weight: u16,
    pub style: String,
}

fn default_opt_level() -> u8 {
    3
}

impl FluiConfig {
    /// Load configuration from flui.toml
    #[allow(dead_code)]
    pub fn load() -> CliResult<Self> {
        let config_path = Path::new("flui.toml");
        if !config_path.exists() {
            return Err(CliError::NotFluiProject {
                reason: "flui.toml not found".to_string(),
            });
        }

        let content = std::fs::read_to_string(config_path).context("Failed to read flui.toml")?;

        let config: FluiConfig = toml::from_str(&content).context("Failed to parse flui.toml")?;

        Ok(config)
    }

    /// Save configuration to flui.toml
    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> CliResult<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(path.join("flui.toml"), content).context("Failed to write flui.toml")?;

        Ok(())
    }
}

/// Global FLUI CLI configuration (~/.flui/config.toml)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(dead_code)]
pub struct GlobalConfig {
    #[serde(default)]
    pub sdk: SdkConfig,
    #[serde(default)]
    pub build: GlobalBuildConfig,
    #[serde(default)]
    pub devtools: DevToolsConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SdkConfig {
    #[serde(default = "default_channel")]
    pub channel: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct GlobalBuildConfig {
    #[serde(default = "default_jobs")]
    pub jobs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DevToolsConfig {
    #[serde(default = "default_devtools_port")]
    pub port: u16,
    #[serde(default)]
    pub auto_launch: bool,
}

impl Default for DevToolsConfig {
    fn default() -> Self {
        Self {
            port: default_devtools_port(),
            auto_launch: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[allow(dead_code)]
fn default_channel() -> String {
    "stable".to_string()
}

#[allow(dead_code)]
fn default_jobs() -> usize {
    num_cpus::get()
}

#[allow(dead_code)]
fn default_devtools_port() -> u16 {
    9100
}

impl GlobalConfig {
    /// Load global configuration
    #[allow(dead_code)]
    pub fn load() -> CliResult<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            // Return default config if not found
            return Ok(Self::default());
        }

        let content =
            std::fs::read_to_string(&config_path).context("Failed to read global config")?;

        let config: GlobalConfig =
            toml::from_str(&content).context("Failed to parse global config")?;

        Ok(config)
    }

    /// Save global configuration
    #[allow(dead_code)]
    pub fn save(&self) -> CliResult<()> {
        let config_path = Self::config_path()?;
        let config_dir = config_path.parent().unwrap();

        std::fs::create_dir_all(config_dir).context("Failed to create config directory")?;

        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(&config_path, content).context("Failed to write global config")?;

        Ok(())
    }

    /// Get global config path (~/.flui/config.toml)
    #[allow(dead_code)]
    fn config_path() -> CliResult<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| CliError::WithContext {
            message: "Could not find home directory".to_string(),
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Home directory not found",
            )),
        })?;

        Ok(home.join(".flui").join("config.toml"))
    }
}

// Add num_cpus to Cargo.toml dependencies
// For now, use a simple fallback
mod num_cpus {
    #[allow(dead_code)]
    pub fn get() -> usize {
        4 // Default value
    }
}
