//! `flui.toml`, the project file the CLI reads and `flui platform` rewrites.
//!
//! Only the keys a command consumes are modelled here, so what `save`
//! writes back is exactly what the README documents:
//!
//! ```toml
//! [app]
//! name = "my_app"
//! version = "0.1.0"
//! organization = "com.example"
//!
//! [build]
//! target_platforms = ["macos", "ios", "web"]
//!
//! # Written by `flui create --hot-reload`; makes `flui run` use the worker host.
//! [hot_reload]
//! host_package = "my_app-host"
//! worker_package = "my_app-logic"
//! worker_lib = "my_app_logic"
//! logic_watch = "logic/src"
//! ```
//!
//! Unknown keys are ignored on load, so a file from an older version still
//! parses. There is deliberately no global (per-user) configuration file:
//! the CLI keeps no state about the user and has no telemetry to configure.

use crate::error::{CliError, CliResult, ResultExt};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The contents of `flui.toml` at the project root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FluiConfig {
    /// Application metadata.
    pub(crate) app: AppConfig,
    /// Build configuration.
    #[serde(default)]
    pub(crate) build: BuildConfig,
    /// Host/worker hot reload layout, when the project was created with
    /// `--hot-reload`.
    #[serde(default)]
    pub(crate) hot_reload: Option<HotReloadConfig>,
}

impl FluiConfig {
    /// Load `flui.toml` from the current directory.
    pub(crate) fn load() -> CliResult<Self> {
        Self::load_from(Path::new("flui.toml"))
    }

    /// Load the file at `path`; a missing file means "not a FLUI project".
    pub(crate) fn load_from(path: &Path) -> CliResult<Self> {
        if !path.exists() {
            return Err(CliError::NotFluiProject {
                reason: format!("{} not found", path.display()),
            });
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Write `flui.toml` into `dir`.
    pub(crate) fn save(&self, dir: &Path) -> CliResult<()> {
        let content = toml::to_string_pretty(self).context("failed to serialize configuration")?;
        std::fs::write(dir.join("flui.toml"), content).context("failed to write flui.toml")?;
        Ok(())
    }
}

/// Host/worker hot reload layout.
///
/// When present, `flui run` keeps the host process alive, rebuilds only the
/// worker `cdylib` on source changes, and relies on
/// `flui_hot_reload::WorkerReloadDriver` in the host runner to apply the
/// reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HotReloadConfig {
    /// Host binary crate (`cargo run -p …`).
    pub(crate) host_package: String,
    /// Reloadable worker crate (`cargo build -p …`, `cdylib`).
    pub(crate) worker_package: String,
    /// `cdylib` library name (artifact: `lib{name}.so` / `{name}.dll`).
    pub(crate) worker_lib: String,
    /// Directory to watch for UI changes, relative to this `flui.toml`.
    pub(crate) logic_watch: String,
    /// Optional shared types crate; changes trigger a full host restart.
    #[serde(default)]
    pub(crate) types_watch: Option<String>,
}

/// Application metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    /// Application name (used as crate name).
    pub(crate) name: String,
    /// Application version (semver).
    pub(crate) version: String,
    /// Organization identifier (reverse domain notation).
    pub(crate) organization: String,
}

impl AppConfig {
    /// The organization and name combined into the identifier mobile
    /// platforms use (Android package name, iOS bundle identifier).
    pub(crate) fn app_id(&self) -> String {
        let sanitized_name = self.name.replace('-', "_");
        format!("{}.{}", self.organization, sanitized_name)
    }
}

/// Build configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct BuildConfig {
    /// Platforms declared for this project, managed by `flui platform`.
    #[serde(default)]
    pub(crate) target_platforms: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FluiConfig {
        FluiConfig {
            app: AppConfig {
                name: "my-app".into(),
                version: "0.1.0".into(),
                organization: "com.example".into(),
            },
            build: BuildConfig {
                target_platforms: vec!["macos".into()],
            },
            hot_reload: None,
        }
    }

    #[test]
    fn app_id_uses_underscores() {
        assert_eq!(sample().app.app_id(), "com.example.my_app");
    }

    #[test]
    fn round_trips_and_writes_only_the_documented_keys() {
        let toml = toml::to_string_pretty(&sample()).unwrap();
        assert!(toml.contains("name = \"my-app\""));
        assert!(toml.contains("target_platforms = [\"macos\"]"));
        assert!(
            !toml.contains("hot_reload"),
            "an absent section is not written; got:\n{toml}"
        );
        assert_eq!(toml::from_str::<FluiConfig>(&toml).unwrap(), sample());
    }

    #[test]
    fn unknown_keys_from_older_files_are_ignored() {
        let text = "[app]\nname = \"a\"\nversion = \"0.1.0\"\norganization = \"com.a\"\n\n[assets]\ndirectories = [\"assets\"]\n";
        let config: FluiConfig = toml::from_str(text).unwrap();
        assert_eq!(config.app.name, "a");
    }
}
