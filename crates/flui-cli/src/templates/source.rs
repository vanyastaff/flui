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
        let mut required = vec!["flui-app", "flui-view", "flui-widgets"];
        if hot_reload {
            required.extend(["flui-hot-reload", "flui-types"]);
        }
        for name in required {
            let member = format!("crates/{name}");
            let members = manifest["workspace"]
                .get("members")
                .and_then(toml::Value::as_array);
            if !members
                .is_some_and(|members| members.iter().any(|value| value.as_str() == Some(&member)))
            {
                return Err(CliError::Missing(format!(
                    "FLUI source '{}' does not declare workspace member '{member}'",
                    root.display()
                )));
            }
            let crate_manifest = read_manifest(&root.join(&member).join("Cargo.toml"))?;
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
    pub fn dependency(&self, name: &str, features: &[&str]) -> String {
        let version = env!("CARGO_PKG_VERSION");
        if matches!(self, Self::Registry) && features.is_empty() {
            return toml::Value::String(version.into()).to_string();
        }
        let mut dependency = toml::Table::new();
        match self {
            Self::Registry => {
                dependency.insert("version".into(), version.into());
            }
            Self::Local(source) => {
                let path = if name == "flui" {
                    Path::new(&source.root).to_path_buf()
                } else {
                    Path::new(&source.root).join("crates").join(name)
                };
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
}

fn read_manifest(path: &Path) -> CliResult<toml::Table> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Cannot read FLUI source manifest '{}'; use a complete FLUI checkout",
            path.display()
        )
    })?;
    toml::from_str(&text)
        .with_context(|| format!("Invalid FLUI source manifest '{}'", path.display()))
}

fn require_utf8(path: &Path) -> CliResult<&str> {
    path.to_str().ok_or_else(|| {
        CliError::Missing("FLUI source path must be UTF-8 to appear in Cargo.toml; move the checkout to a UTF-8 path".into())
    })
}
