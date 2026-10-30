//! `flui.toml`, the project file the CLI reads and `flui platform` rewrites.
//!
//! Only the keys a command consumes are modelled here, so what `save`
//! writes back is exactly what the README documents:
//!
//! ```toml
//! [app]
//! name = "my_app"
//! version = "0.1.0"
//! id = "com.example.my_app"
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
//!
//! # Optional: raise FLUI's floor for one environment (ADR-0244 §10).
//! [platforms.ios]
//! min = "15.0"
//! ```
//!
//! Unknown keys are ignored on load, so a file from an older version still
//! parses. There is deliberately no global (per-user) configuration file:
//! the CLI keeps no state about the user and has no telemetry to configure.

use crate::app_id::AppId;
use crate::build::app_floor::{AppFloors, PlatformsConfig};
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
    /// The application's floor per environment (ADR-0244 §10), when it
    /// raises FLUI's: `[platforms.<environment>] min`.
    #[serde(default, skip_serializing_if = "PlatformsConfig::is_empty")]
    pub(crate) platforms: PlatformsConfig,
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

        if let Some(id) = legacy_organization_id(&content) {
            return Err(CliError::InvalidConfig {
                file: path.display().to_string(),
                reason: format!(
                    "`[app] organization` is replaced by the application identifier; write \
                     `id = \"{id}\"` in its place"
                ),
            });
        }
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// The floors a build of this project uses: FLUI's, raised where
    /// `[platforms]` asks.
    ///
    /// # Errors
    /// [`CliError::AppFloor`] when a `min` is below FLUI's floor, is given
    /// for the web or Linux, or is not the value its environment takes.
    pub(crate) fn floors(&self) -> CliResult<AppFloors> {
        Ok(self.platforms.floors()?)
    }

    /// Write `flui.toml` into `dir`.
    pub(crate) fn save(&self, dir: &Path) -> CliResult<()> {
        let content = toml::to_string_pretty(self).context("failed to serialize configuration")?;
        std::fs::write(dir.join("flui.toml"), content).context("failed to write flui.toml")?;
        Ok(())
    }
}

/// The floors a build in `root` uses: those of the nearest `flui.toml` at or
/// above `root`, the one `flui run` finds the project by (a worker project's
/// host package sits below it), or FLUI's alone when there is none (an
/// in-repo example, a plain cargo package).
///
/// # Errors
/// When `flui.toml` cannot be read or parsed, or a `[platforms]` table is
/// refused ([`FluiConfig::floors`]).
pub(crate) fn project_floors(root: &Path) -> CliResult<AppFloors> {
    match root
        .ancestors()
        .map(|dir| dir.join("flui.toml"))
        .find(|manifest| manifest.is_file())
    {
        Some(manifest) => FluiConfig::load_from(&manifest)?.floors(),
        None => Ok(AppFloors::default()),
    }
}

/// Host/worker hot reload layout.
///
/// When present, `flui run` keeps the host process alive, rebuilds only the
/// worker `cdylib` on source changes, and relies on the
/// `flui_hot_reload::WorkerReloadHook` the host installs with
/// `AppConfig::with_dev_reload` to apply the reload.
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

/// The identifier a `flui.toml` written before `[app] id` existed would have
/// had (`organization`, then `name` with `-` as `_`); `None` for any other
/// file.
fn legacy_organization_id(content: &str) -> Option<String> {
    let table = content.parse::<toml::Table>().ok()?;
    let app = table.get("app")?.as_table()?;
    if app.contains_key("id") {
        return None;
    }
    let organization = app.get("organization")?.as_str()?;
    let name = app.get("name")?.as_str()?;
    Some(format!("{organization}.{}", name.replace('-', "_")))
}

/// Application metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    /// Application name (used as crate name).
    pub(crate) name: String,
    /// Application version (semver).
    pub(crate) version: String,
    /// The application identifier (reverse-DNS, `com.example.my_app`), from
    /// which every platform name is derived ([`AppId`]).
    pub(crate) id: AppId,
}

/// Build configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct BuildConfig {
    /// Platforms declared for this project, managed by `flui platform`.
    #[serde(default)]
    pub(crate) target_platforms: Vec<String>,
}

/// The environment variable the application's identifier reaches `flui-app`
/// through when it compiles (`AppIdentity::app_id`, which names the
/// application's data directory and its Apple log subsystem).
pub(crate) const APP_ID_ENV: &str = "FLUI_APP_ID";

/// Where cargo reads the project's `[env]`, relative to the project root.
pub(crate) const CARGO_CONFIG: &str = ".cargo/config.toml";

/// The extensionless name cargo still reads first: when it exists, cargo
/// ignores [`CARGO_CONFIG`].
const LEGACY_CARGO_CONFIG: &str = ".cargo/config";

/// `existing` (a `.cargo/config.toml`, or `None` when there is none) with
/// `[env] FLUI_APP_ID` set to `app_id`; `None` when it already says so.
///
/// Only that key changes: every other table, key, comment and its layout is
/// kept as written. A key in the inline-table form (`{ value = "…" }`) keeps
/// its form.
///
/// # Errors
///
/// [`CliError::InvalidConfig`] when `existing` is not TOML, or holds an `env`
/// that is not a table.
pub(crate) fn cargo_config_with_app_id(
    existing: Option<&str>,
    app_id: &str,
) -> CliResult<Option<String>> {
    let invalid = |reason: String| CliError::InvalidConfig {
        file: CARGO_CONFIG.into(),
        reason,
    };
    let mut document = existing
        .unwrap_or_default()
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| invalid(error.to_string()))?;
    let env = document
        .entry("env")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| invalid("`env` is not a table".into()))?;
    let current = env.get(APP_ID_ENV).and_then(|item| {
        item.as_str()
            .or_else(|| item.as_table_like()?.get("value")?.as_str())
    });
    if current == Some(app_id) {
        return Ok(None);
    }
    match env.get_mut(APP_ID_ENV) {
        Some(item) if item.is_table_like() => {
            let table = item.as_table_like_mut().expect("BUG: checked table-like");
            table.insert("value", toml_edit::value(app_id));
        }
        _ => {
            env.insert(APP_ID_ENV, toml_edit::value(app_id));
        }
    }
    Ok(Some(document.to_string()))
}

/// Keep the project's cargo configuration naming `config`'s application
/// identifier, so that every cargo invocation in the project, an editor's
/// included, builds the application with it; written only when the value
/// changed. The file is the one cargo reads: `.cargo/config` when it exists,
/// since cargo then ignores `.cargo/config.toml`, otherwise
/// `.cargo/config.toml`.
///
/// # Errors
///
/// The file cannot be read or written, or is not valid TOML.
pub(crate) fn sync_app_id_env(root: &Path, config: &FluiConfig) -> CliResult<()> {
    let legacy = root.join(LEGACY_CARGO_CONFIG);
    let path = if legacy.is_file() {
        legacy
    } else {
        root.join(CARGO_CONFIG)
    };
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(CliError::context(
                error,
                format!("failed to read {}", path.display()),
            ));
        }
    };
    let Some(updated) = cargo_config_with_app_id(existing.as_deref(), config.app.id.as_str())?
    else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }
    std::fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))
}

/// The nearest `flui.toml` at or above the current directory, with the
/// directory it is in; `None` outside a FLUI project.
///
/// # Errors
///
/// The current directory cannot be read, or the file found is not valid.
pub(crate) fn find_flui_config() -> CliResult<Option<(std::path::PathBuf, FluiConfig)>> {
    let mut dir = std::env::current_dir().context("could not read current directory")?;
    loop {
        let path = dir.join("flui.toml");
        if path.exists() {
            let config = FluiConfig::load_from(&path)?;
            return Ok(Some((dir, config)));
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

/// Before `flui build` or `flui run` compiles: bring the project's
/// `.cargo/config.toml` in line with its `flui.toml` identifier. Nothing to do
/// outside a FLUI project, such as an example in FLUI's own repository.
///
/// # Errors
///
/// As [`find_flui_config`] and [`sync_app_id_env`].
pub(crate) fn sync_app_id_env_for_current_project() -> CliResult<()> {
    match find_flui_config()? {
        Some((root, config)) => sync_app_id_env(&root, &config),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FluiConfig {
        FluiConfig {
            app: AppConfig {
                name: "my-app".into(),
                version: "0.1.0".into(),
                id: AppId::try_new("com.example.my_app").expect("an identifier"),
            },
            build: BuildConfig {
                target_platforms: vec!["macos".into()],
            },
            hot_reload: None,
            platforms: PlatformsConfig::default(),
        }
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
    fn the_cargo_env_key_is_rewritten_alone_and_only_when_it_changed() {
        let id = "com.example.my_app";
        let cases: [(&str, Option<&str>, Option<&str>); 6] = [
            (
                "no file",
                None,
                Some("[env]\nFLUI_APP_ID = \"com.example.my_app\"\n"),
            ),
            (
                "another table only",
                Some("[term]\nverbose = true\n"),
                Some("[term]\nverbose = true\n\n[env]\nFLUI_APP_ID = \"com.example.my_app\"\n"),
            ),
            (
                "a stale value beside another key",
                Some("[env]\nOTHER = \"1\" # mine\nFLUI_APP_ID = \"com.old.app\"\n"),
                Some("[env]\nOTHER = \"1\" # mine\nFLUI_APP_ID = \"com.example.my_app\"\n"),
            ),
            (
                "a stale value in the inline-table form",
                Some("[env]\nFLUI_APP_ID = { value = \"com.old.app\", force = true }\n"),
                Some("[env]\nFLUI_APP_ID = { value = \"com.example.my_app\", force = true }\n"),
            ),
            (
                "already in line",
                Some("[env]\nFLUI_APP_ID = \"com.example.my_app\"\n"),
                None,
            ),
            (
                "already in line, inline-table form",
                Some("[env]\nFLUI_APP_ID = { value = \"com.example.my_app\" }\n"),
                None,
            ),
        ];
        for (case, existing, expected) in cases {
            let updated = cargo_config_with_app_id(existing, id).expect(case);
            assert_eq!(updated.as_deref(), expected, "{case}");
        }
    }

    #[test]
    fn a_cargo_config_flui_cannot_edit_is_refused() {
        for existing in ["env = \"x\"\n", "[env\n"] {
            let refused = cargo_config_with_app_id(Some(existing), "com.example.app");
            assert!(
                matches!(refused, Err(CliError::InvalidConfig { .. })),
                "{existing:?}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_flui_toml_from_before_the_identifier_names_the_line_to_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("flui.toml");
        std::fs::write(
            &path,
            "[app]\nname = \"my-app\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n",
        )
        .expect("write flui.toml");
        let refused = FluiConfig::load_from(&path).expect_err("no `[app] id`");
        assert!(
            matches!(&refused, CliError::InvalidConfig { reason, .. }
                if reason.contains("id = \"com.example.my_app\"")),
            "{refused:?}"
        );

        std::fs::write(
            &path,
            "[app]\nname = \"my-app\"\nversion = \"0.1.0\"\nid = \"com.example.my_app\"\n",
        )
        .expect("write flui.toml");
        let loaded = FluiConfig::load_from(&path).expect("an identifier");
        assert_eq!(loaded.app.id.as_str(), "com.example.my_app");

        std::fs::write(
            &path,
            "[app]\nname = \"my-app\"\nversion = \"0.1.0\"\nid = \"com.my-company.app\"\n",
        )
        .expect("write flui.toml");
        assert!(
            FluiConfig::load_from(&path).is_err(),
            "a refused identifier"
        );
    }

    #[test]
    fn the_identifier_goes_into_the_cargo_configuration_cargo_reads() {
        for legacy in [false, true] {
            let dir = tempfile::tempdir().expect("temp dir");
            std::fs::create_dir(dir.path().join(".cargo")).expect("create .cargo");
            if legacy {
                std::fs::write(
                    dir.path().join(LEGACY_CARGO_CONFIG),
                    "[term]\nverbose = true\n",
                )
                .expect("write .cargo/config");
            }
            sync_app_id_env(dir.path(), &sample()).expect("synced");
            let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).ok();
            let (written, untouched) = if legacy {
                (LEGACY_CARGO_CONFIG, CARGO_CONFIG)
            } else {
                (CARGO_CONFIG, LEGACY_CARGO_CONFIG)
            };
            let text = read(written).expect("the file cargo reads is written");
            assert!(
                text.contains("FLUI_APP_ID = \"com.example.my_app\""),
                "{written}: {text}"
            );
            assert_eq!(
                read(untouched),
                None,
                "legacy {legacy}: {untouched} stays absent"
            );
        }
    }
}
