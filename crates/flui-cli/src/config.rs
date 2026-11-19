//! `flui.toml`, the project file the CLI reads and `flui platform` rewrites.
//!
//! Only the keys a command consumes are modelled here, so what `save`
//! writes back is exactly what the README documents:
//!
//! ```toml
//! [app]
//! name = "my_app"                  # a crate-like name, checked when the file loads
//! display_name = "My App"          # optional: the name the application is shown by
//! version = "0.1.0"
//! build = 12                       # optional: the store build number; a release of a pre-release needs it
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
//!
//! # Optional: the key a release APK is signed with; its passwords come from
//! # FLUI_ANDROID_KEYSTORE_PASSWORD (and FLUI_ANDROID_KEY_PASSWORD).
//! [signing.android]
//! keystore = "keys/upload.jks"
//! alias = "upload"
//!
//! # Optional: the path a web build is served from, its page's `<base href>`.
//! [web]
//! base_href = "/apps/demo/"
//! description = "A FLUI application."
//! theme_color = "#1a73e8"
//! # page = "platforms/web/index.html"   # the project's own page template
//! ```
//!
//! Unknown keys are ignored on load, so a file from an older version still
//! parses. There is deliberately no global (per-user) configuration file:
//! the CLI keeps no state about the user and has no telemetry to configure.

use crate::app_id::AppId;
use crate::build::android_signing::AndroidReleaseKey;
use crate::build::app_floor::{AppFloors, PlatformsConfig};
use crate::build::app_version::{AppVersion, BuildNumber};
use crate::build::web_page::{BaseHref, ThemeColor, WebPage};
use crate::error::{CliError, CliResult, ResultExt};
use crate::types::{DisplayName, ProjectName};
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
    /// The keys a release package is signed with: `[signing.<environment>]`.
    #[serde(default, skip_serializing_if = "SigningConfig::is_empty")]
    pub(crate) signing: SigningConfig,
    /// What a web build's page states: `[web] base_href`.
    #[serde(default, skip_serializing_if = "WebConfig::is_default")]
    pub(crate) web: WebConfig,
    /// The files the application ships: `[assets] directories`.
    #[serde(default, skip_serializing_if = "AssetsConfig::is_empty")]
    pub(crate) assets: AssetsConfig,
}

/// `[assets]`: the directories whose files every package carries, each
/// read back by its path from the project root (ADR-0384 §3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetsConfig {
    /// Directories relative to the project root (`assets`): every regular
    /// file under each, at any depth, is an asset keyed by its path from
    /// the root (`assets/images/logo.png`).
    #[serde(default)]
    pub(crate) directories: Vec<String>,
}

impl AssetsConfig {
    fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }
}

/// `[signing]`: the key each environment's release packages are signed
/// with, when the application names one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct SigningConfig {
    /// `[signing.android]`: the release key of the APK.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) android: Option<AndroidReleaseKey>,
}

impl SigningConfig {
    fn is_empty(&self) -> bool {
        self.android.is_none()
    }
}

/// The web build's page.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct WebConfig {
    /// The path the build is served from (`/apps/demo/`), which the page
    /// names as its `<base href>`; the site's root when absent.
    #[serde(default)]
    pub(crate) base_href: BaseHref,
    /// The page's `description` and the manifest's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    /// The browser chrome's colour (`theme-color`), a CSS colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) theme_color: Option<ThemeColor>,
    /// The project's page template, relative to the project
    /// (`platforms/web/index.html`); the generated page when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<std::path::PathBuf>,
}

impl WebConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a web build of `project` describes (none: an in-repo example or a
/// plain package) states in its page and manifest.
#[must_use]
pub(crate) fn web_page(project: Option<&Project>) -> WebPage {
    project.map_or_else(WebPage::default, |Project { config, dir }| WebPage {
        title: Some(config.app.shown_name().to_owned()),
        description: config.web.description.clone(),
        theme_color: config.web.theme_color.clone(),
        base_href: config.web.base_href.clone(),
        template: config.web.page.clone(),
        project: Some(dir.clone()),
    })
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

        let content = read_bounded(path)?;

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

/// The most bytes a `flui.toml` may hold. A project file is a few hundred
/// bytes; one past this bound is refused before it is decoded, so a file that
/// is not a project file (or never ends) is never read into memory whole.
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// The text of the `flui.toml` at `path`, read only up to
/// [`MAX_CONFIG_BYTES`].
///
/// # Errors
///
/// When the file cannot be read, holds more than [`MAX_CONFIG_BYTES`], or is
/// not UTF-8.
fn read_bounded(path: &Path) -> CliResult<String> {
    use std::io::Read as _;

    let file =
        std::fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(CliError::InvalidConfig {
            file: path.display().to_string(),
            reason: format!(
                "holds more than {MAX_CONFIG_BYTES} bytes, which no project file needs"
            ),
        });
    }
    String::from_utf8(bytes).map_err(|_| CliError::InvalidConfig {
        file: path.display().to_string(),
        reason: "is not UTF-8 text".to_owned(),
    })
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
    floors_of(nearest_config(root)?.as_ref())
}

/// The floors of the project `config` describes; FLUI's alone with none.
///
/// # Errors
/// A `[platforms]` table is refused ([`FluiConfig::floors`]).
pub(crate) fn floors_of(config: Option<&FluiConfig>) -> CliResult<AppFloors> {
    config.map_or_else(|| Ok(AppFloors::default()), FluiConfig::floors)
}

/// The nearest `flui.toml` at or above `root`, the one `flui run` finds the
/// project by; `None` when there is none (an in-repo example, a plain cargo
/// package).
///
/// # Errors
/// When `flui.toml` cannot be read or parsed.
pub(crate) fn nearest_config(root: &Path) -> CliResult<Option<FluiConfig>> {
    Ok(nearest_project(root)?.map(|project| project.config))
}

/// The project the nearest `flui.toml` at or above `root` describes: the
/// file's contents and the directory it is in, which the project's own
/// paths are relative to.
pub(crate) struct Project {
    pub(crate) config: FluiConfig,
    pub(crate) dir: std::path::PathBuf,
}

/// The nearest directory at or above `root` that holds `flui.toml`: the
/// project's root, which its own paths are relative to.
pub(crate) fn nearest_project_dir(root: &Path) -> Option<&Path> {
    root.ancestors().find(|dir| dir.join("flui.toml").is_file())
}

/// The nearest project at or above `root` ([`nearest_config`]).
///
/// # Errors
/// When `flui.toml` cannot be read or parsed.
pub(crate) fn nearest_project(root: &Path) -> CliResult<Option<Project>> {
    let Some(dir) = nearest_project_dir(root) else {
        return Ok(None);
    };
    Ok(Some(Project {
        config: FluiConfig::load_from(&dir.join("flui.toml"))?,
        dir: dir.to_path_buf(),
    }))
}

/// Host/worker hot reload layout.
///
/// When present, `flui run` keeps the host process alive, rebuilds only the
/// worker `cdylib` on source changes, and relies on the
/// `flui_hot_reload::WorkerReloadHook` the host installs with
/// `AppConfig::dev_reload` to apply the reload.
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
    /// The host feature that installs the reload hook. When set, `flui run`
    /// builds and runs the host with it and with the host's default features
    /// off, so a host whose default build links the logic crate loads the
    /// worker library instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reload_feature: Option<String>,
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
    /// Application name: the crate's name, and the name each package and
    /// generated project states. It is a [`ProjectName`], checked when the
    /// file loads, so a quote, a `$`, a slash or a space never reaches the
    /// Kotlin, XML, JSON or HTML it is written into.
    pub(crate) name: ProjectName,
    /// The name the application is shown by, free text (`"My Notes"`); the
    /// [`name`](Self::name) when absent ([`AppConfig::shown_name`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<DisplayName>,
    /// The application's version, SemVer, from which every package's version
    /// is derived ([`AppVersion`]).
    pub(crate) version: AppVersion,
    /// The build number both stores compare (`versionCode`,
    /// `CFBundleVersion`), when the project declares one; a release build of
    /// a pre-release needs it ([`AppVersion::package_numbers`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) build: Option<BuildNumber>,
    /// The application identifier (reverse-DNS, `com.example.my_app`), from
    /// which every platform name is derived ([`AppId`]).
    pub(crate) id: AppId,
}

impl AppConfig {
    /// The name the application is shown by: `[app] display_name`, else
    /// `[app] name`.
    #[must_use]
    pub(crate) fn shown_name(&self) -> &str {
        self.display_name
            .as_ref()
            .map_or(self.name.as_str(), DisplayName::as_str)
    }
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
                name: ProjectName::new("my-app").expect("a project name"),
                display_name: None,
                version: AppVersion::try_new("0.1.0").expect("a SemVer version"),
                build: None,
                id: AppId::try_new("com.example.my_app").expect("an identifier"),
            },
            build: BuildConfig {
                target_platforms: vec!["macos".into()],
            },
            hot_reload: None,
            platforms: PlatformsConfig::default(),
            signing: SigningConfig::default(),
            web: WebConfig::default(),
            assets: AssetsConfig::default(),
        }
    }

    #[test]
    fn a_web_base_path_is_read_and_a_bad_one_refused() {
        let with = |web: &str| {
            format!("[app]\nname = \"a\"\nversion = \"0.1.0\"\nid = \"com.example.a\"\n{web}")
        };
        let read = toml::from_str::<FluiConfig>(&with("[web]\nbase_href = \"/apps/demo/\"\n"))
            .expect("a base path");
        assert_eq!(read.web.base_href.to_string(), "/apps/demo/");
        let absent = toml::from_str::<FluiConfig>(&with("")).expect("no web table");
        assert_eq!(absent.web.base_href, BaseHref::root());
        assert!(
            toml::from_str::<FluiConfig>(&with("[web]\nbase_href = \"apps\"\n")).is_err(),
            "a base path that is not an absolute directory"
        );
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

    /// What the project file says is checked when it loads, before any
    /// command writes a value of it into a generated file: a name that is
    /// not a project name (a quote that would close the Kotlin string of
    /// `settings.gradle.kts`, a `$` Kotlin reads as a template, a space, a
    /// slash) is refused naming the name, and a file past the size bound is
    /// refused before it is decoded.
    #[test]
    fn a_project_file_is_checked_when_it_loads() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("flui.toml");
        let with_name = |name: &str| {
            format!("[app]\nname = {name}\nversion = \"0.1.0\"\nid = \"com.example.notes\"\n")
        };
        for (name, accepted) in [
            ("\"notes\"", true),
            ("\"notes-app_2\"", true),
            ("'my\"app'", false),
            ("\"my$app\"", false),
            ("\"my app\"", false),
            ("\"../app\"", false),
            ("\"\"", false),
        ] {
            std::fs::write(&path, with_name(name)).expect("write flui.toml");
            let loaded = FluiConfig::load_from(&path);
            assert_eq!(loaded.is_ok(), accepted, "{name}: {loaded:?}");
        }
        // The shown name is free text within its bound; the version is one
        // every packager takes; the build number is one both stores take.
        let with_app =
            |lines: &str| format!("[app]\nname = \"notes\"\nid = \"com.example.notes\"\n{lines}\n");
        let too_long = format!(
            "version = \"1.2.3\"\ndisplay_name = \"{}\"",
            "a".repeat(DisplayName::MAX_CHARS + 1)
        );
        for (lines, accepted) in [
            (
                "version = \"1.2.3\"\ndisplay_name = \"My \\\"R&D\\\" Notes\"",
                true,
            ),
            ("version = \"1.2.3\"\ndisplay_name = \"\"", false),
            ("version = \"1.2.3\"\ndisplay_name = \"a\\u0001\"", false),
            ("version = \"1.2.3\"\ndisplay_name = \"a\\uFFFF\"", false),
            (too_long.as_str(), false),
            ("version = \"1.200.0\"", false),
            ("version = \"0.0.0\"", false),
            ("version = \"2.0.0-beta.1\"\nbuild = 41", true),
            ("version = \"1.2.3\"\nbuild = 0", false),
            ("version = \"1.2.3\"\nbuild = 2100000001", false),
            ("version = \"1.2.3\"\nbuild = -1", false),
        ] {
            std::fs::write(&path, with_app(lines)).expect("write flui.toml");
            let loaded = FluiConfig::load_from(&path);
            assert_eq!(loaded.is_ok(), accepted, "{lines}: {loaded:?}");
        }
        std::fs::write(
            &path,
            with_app("version = \"1.2.3\"\ndisplay_name = \"My Notes\""),
        )
        .expect("write flui.toml");
        let loaded = FluiConfig::load_from(&path).expect("a display name");
        assert_eq!(loaded.app.shown_name(), "My Notes");
        assert_eq!(sample().app.shown_name(), "my-app");
        let mut oversized = with_name("\"notes\"");
        oversized.push_str(&"#".repeat(MAX_CONFIG_BYTES));
        std::fs::write(&path, oversized).expect("write flui.toml");
        let refused = FluiConfig::load_from(&path).expect_err("past the size bound");
        assert!(
            matches!(&refused, CliError::InvalidConfig { reason, .. }
                if reason.contains("more than 1048576 bytes")),
            "{refused:?}"
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
