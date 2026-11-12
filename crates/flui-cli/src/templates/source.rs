//! Validated framework dependency sources shared by all project templates.

use crate::error::{CliError, CliResult, ResultExt};
use std::path::Path;

/// The dependency location selected for a generated project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DependencySource {
    /// Resolve the CLI's framework version from crates.io.
    #[default]
    Registry,
    /// Resolve dependencies from a validated source checkout.
    Local(LocalSource),
}

/// A canonical, UTF-8 FLUI checkout root, constructed only through validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSource {
    root: String,
}

/// A framework package a generated project names in its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkPackage {
    /// The facade, `flui`, at the checkout root.
    Facade,
    /// The official package `flui-hot-reload`, which an application adds
    /// itself: the facade names no reload package (ADR-0311 §6).
    HotReload,
}

impl FrameworkPackage {
    /// The package name, which is also its dependency key.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Facade => "flui",
            Self::HotReload => "flui-hot-reload",
        }
    }

    /// Its directory in a checkout, relative to the root; `None` for the root.
    const fn member(self) -> Option<&'static str> {
        match self {
            Self::Facade => None,
            Self::HotReload => Some("packages/flui-hot-reload"),
        }
    }
}

impl DependencySource {
    /// Resolve and validate the selected checkout before writing any output.
    pub fn resolve(local: Option<&Path>, hot_reload: bool) -> CliResult<Self> {
        let Some(path) = local else {
            return Ok(Self::Registry);
        };
        require_utf8(path)?;
        let root = path.canonicalize().with_context(|| {
            format!(
                "Cannot open FLUI source '{}'; use --local=PATH with a FLUI checkout root",
                path.display()
            )
        })?;
        let root_text = require_utf8(&root)?;
        let manifest = read_manifest(&root.join("Cargo.toml"))?;
        if manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            != Some("flui")
            || manifest
                .get("workspace")
                .and_then(toml::Value::as_table)
                .is_none()
        {
            return Err(CliError::Missing(format!(
                "'{}' is not a FLUI workspace root; use --local=PATH with the checkout containing crates/",
                root.display()
            )));
        }
        let mut required = vec![
            ("crates/flui-app", "flui-app"),
            ("crates/flui-view", "flui-view"),
            ("crates/flui-widgets", "flui-widgets"),
        ];
        if hot_reload {
            let package = FrameworkPackage::HotReload;
            required.push((
                package
                    .member()
                    .expect("BUG: an official package is a workspace member"),
                package.name(),
            ));
        }
        for (member, name) in required {
            let members = manifest["workspace"]
                .get("members")
                .and_then(toml::Value::as_array);
            if !members
                .is_some_and(|members| members.iter().any(|value| value.as_str() == Some(member)))
            {
                return Err(CliError::Missing(format!(
                    "FLUI source '{}' does not declare workspace member '{member}'",
                    root.display()
                )));
            }
            let crate_manifest = read_manifest(&root.join(member).join("Cargo.toml"))?;
            if crate_manifest
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(toml::Value::as_str)
                != Some(name)
            {
                return Err(CliError::Missing(format!(
                    "FLUI source manifest '{member}/Cargo.toml' must declare package '{name}'"
                )));
            }
        }
        Ok(Self::Local(LocalSource {
            root: root_text.into(),
        }))
    }

    /// Serialize a Cargo dependency value, preserving paths and feature names.
    pub fn dependency(&self, package: FrameworkPackage, features: &[&str]) -> String {
        render(FRAMEWORK_ON_CRATES_IO, self, package, features)
    }
}

/// Where a generated project takes FLUI from when `--local` is not given.
///
/// The framework is not on crates.io yet. Until it is, the dependency is the
/// git tag of this CLI's own release (`v<version>`), which the release
/// workflow creates for every `v*` tag, so `flui create` produces a project
/// that builds on the day `cargo install flui-cli` works. Flip it to `true`
/// in the CLI release that follows the framework's first publication;
/// both shapes are pinned by the tests below.
const FRAMEWORK_ON_CRATES_IO: bool = false;

/// `"<version>"` (or `{ version, features }`) when the framework is on
/// crates.io, `{ git = "<repository>", tag = "v<version>" }` until then,
/// `{ path }` for `--local`.
fn render(
    on_crates_io: bool,
    source: &DependencySource,
    package: FrameworkPackage,
    features: &[&str],
) -> String {
    let version = env!("CARGO_PKG_VERSION");
    if matches!(source, DependencySource::Registry) && on_crates_io && features.is_empty() {
        return toml::Value::String(version.into()).to_string();
    }
    let mut dependency = toml::Table::new();
    match source {
        DependencySource::Registry if on_crates_io => {
            dependency.insert("version".into(), version.into());
        }
        DependencySource::Registry => {
            dependency.insert("git".into(), env!("CARGO_PKG_REPOSITORY").into());
            dependency.insert("tag".into(), format!("v{version}").into());
        }
        DependencySource::Local(source) => {
            let root = Path::new(&source.root);
            let path = package
                .member()
                .map_or_else(|| root.to_path_buf(), |member| root.join(member));
            dependency.insert(
                "path".into(),
                path.to_str()
                    .expect("BUG: validated UTF-8 root and crate name produce a UTF-8 path")
                    .into(),
            );
        }
    }
    if !features.is_empty() {
        dependency.insert(
            "features".into(),
            toml::Value::Array(
                features
                    .iter()
                    .map(|feature| toml::Value::String((*feature).into()))
                    .collect(),
            ),
        );
    }
    toml::Value::Table(dependency).to_string()
}

fn read_manifest(path: &Path) -> CliResult<toml::Table> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Cannot read FLUI source manifest '{}'; use a complete FLUI checkout",
            path.display()
        )
    })?;
    toml::from_str(&text)
        .with_context(|| format!("invalid FLUI source manifest '{}'", path.display()))
}

fn require_utf8(path: &Path) -> CliResult<&str> {
    path.to_str().ok_or_else(|| {
        CliError::Missing("FLUI source path must be UTF-8 to appear in Cargo.toml; move the checkout to a UTF-8 path".into())
    })
}
