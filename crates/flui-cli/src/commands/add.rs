//! `flui add <capability>`: link a capability crate into the application
//! (ADR-0154 §8, ADR-0307 §5).
//!
//! With the facade, the capability is the facade feature that enables its
//! crate, and `add` enables it in `Cargo.toml`. Without the facade, or for a
//! crate the facade does not carry, the capability is a crate name and `add`
//! runs `cargo add`, from the FLUI checkout the application's other FLUI
//! dependencies come from when it has the crate. Either way the crate must
//! state its plugin in `[package.metadata.flui.capability]`; `add` then shows
//! the line that registers it and what it brings to the manifests, which the
//! crate declares itself. A change that turns out not to link a capability
//! crate is undone.
//!
//! `--json` emits one `add.done` event.

use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, Package};
use serde::Serialize;

use crate::declarations::Declarations;
use crate::declarations::capability::{PluginPath, plugin_of, plugin_of_manifest};
use crate::error::{CliError, CliResult, ResultExt};
use crate::ui;

/// The facade's package name.
const FACADE: &str = "flui";

/// What `flui add <name>` links.
#[derive(Debug)]
enum Target {
    /// A facade feature that enables one crate.
    Feature {
        /// The facade's key in `[dependencies]` (its name, or a rename).
        key: String,
        feature: String,
        crate_name: String,
        enabled: bool,
    },
    /// A crate added directly.
    Crate {
        crate_name: String,
        present: bool,
        /// The crate's directory in the application's FLUI checkout.
        path: Option<PathBuf>,
    },
}

impl Target {
    fn crate_name(&self) -> &str {
        match self {
            Self::Feature { crate_name, .. } | Self::Crate { crate_name, .. } => crate_name,
        }
    }

    fn linked(&self) -> bool {
        match self {
            Self::Feature { enabled, .. } => *enabled,
            Self::Crate { present, .. } => *present,
        }
    }

    /// The path code names the crate by.
    fn prefix(&self) -> String {
        match self {
            Self::Feature { key, feature, .. } => {
                format!("{}::services::{feature}", key.replace('-', "_"))
            }
            Self::Crate { crate_name, .. } => crate_name.replace('-', "_"),
        }
    }
}

/// The `add.done` event.
#[derive(Debug, Serialize)]
struct Added<'a> {
    capability: &'a str,
    #[serde(rename = "crate")]
    crate_name: &'a str,
    feature: Option<&'a str>,
    /// Whether `Cargo.toml` changed; `false` when the capability was linked.
    changed: bool,
    snippet: String,
    android_permissions: Vec<String>,
    apple_accessed_apis: Vec<AccessedApi>,
}

#[derive(Debug, Serialize)]
struct AccessedApi {
    category: &'static str,
    reason: String,
}

/// Execute `flui add <name>`.
///
/// # Errors
///
/// Not a Cargo project, an unknown capability, a crate without a capability
/// table, a `Cargo.toml` in a shape `flui` cannot edit, a failing `cargo`.
pub(crate) fn execute(name: &str) -> CliResult<()> {
    let dir = std::env::current_dir().context("could not read current directory")?;
    let manifest = dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(CliError::NotFluiProject {
            reason: "Cargo.toml not found".into(),
        });
    }
    let before = metadata(&dir)?;
    let root = before
        .root_package()
        .ok_or_else(|| CliError::InvalidConfig {
            file: "Cargo.toml".into(),
            reason: "is a virtual workspace; run `flui add` in the application's directory".into(),
        })?;
    let target = resolve(&before, root, name)?;
    let lock = before.workspace_root.join("Cargo.lock");
    let snapshot = Snapshot::take(&[manifest.as_path(), lock.as_std_path()])?;
    let changed = !target.linked();
    let added = apply(&target, &manifest, &dir).and_then(|()| describe(&dir, &target));
    let (plugin, declarations) = match added {
        Ok(added) => added,
        Err(error) => {
            if changed {
                snapshot.restore()?;
            }
            return Err(error);
        }
    };
    let snippet = format!(".plugin({}::{plugin}::default())", target.prefix());
    report(&Added {
        capability: name,
        crate_name: target.crate_name(),
        feature: match &target {
            Target::Feature { feature, .. } => Some(feature),
            Target::Crate { .. } => None,
        },
        changed,
        snippet,
        android_permissions: declarations
            .android_permissions()
            .map(|(permission, _)| permission.to_string())
            .collect(),
        apple_accessed_apis: declarations
            .apple_accessed_apis()
            .flat_map(|(category, reasons)| {
                reasons.keys().map(move |reason| AccessedApi {
                    category: category.as_str(),
                    reason: reason.as_str().to_owned(),
                })
            })
            .collect(),
    });
    Ok(())
}

fn metadata(dir: &Path) -> CliResult<Metadata> {
    MetadataCommand::new()
        .current_dir(dir)
        .exec()
        .context("cargo metadata failed")
}

/// What `name` means for the application `root` of `metadata`.
fn resolve(metadata: &Metadata, root: &Package, name: &str) -> CliResult<Target> {
    let direct = |crate_name: &str| {
        root.dependencies
            .iter()
            .any(|dependency| dependency.name == crate_name)
    };
    let facade = root.dependencies.iter().find(|dependency| {
        dependency.name == FACADE
            && dependency.kind == DependencyKind::Normal
            && dependency.target.is_none()
    });
    let Some(facade) = facade else {
        return Ok(Target::Crate {
            crate_name: name.to_owned(),
            present: direct(name),
            path: checkout_crate(root, name),
        });
    };
    let facade_package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == FACADE)
        .ok_or_else(|| CliError::Missing(format!("cargo metadata lists no `{FACADE}` package")))?;
    if let Some(crate_name) = feature_crate(facade_package, name) {
        return Ok(Target::Feature {
            key: facade.rename.clone().unwrap_or_else(|| FACADE.to_owned()),
            feature: name.to_owned(),
            crate_name,
            enabled: facade.features.iter().any(|feature| feature == name)
                || (facade.uses_default_features
                    && facade_package
                        .features
                        .get("default")
                        .is_some_and(|default| default.iter().any(|feature| feature == name))),
        });
    }
    if direct(name) {
        return Ok(Target::Crate {
            crate_name: name.to_owned(),
            present: true,
            path: None,
        });
    }
    let offered = offered(facade_package);
    Err(CliError::Usage(if offered.is_empty() {
        format!("`{FACADE}` has no capability feature `{name}`")
    } else {
        format!(
            "`{FACADE}` has no capability feature `{name}`; it offers {}",
            offered.join(", ")
        )
    }))
}

/// The crate the facade's feature `feature` enables, when it enables one
/// optional dependency and nothing else.
fn feature_crate(facade: &Package, feature: &str) -> Option<String> {
    let [enabled] = facade.features.get(feature)?.as_slice() else {
        return None;
    };
    let dependency = enabled.strip_prefix("dep:")?;
    facade
        .dependencies
        .iter()
        .find(|candidate| {
            candidate.optional
                && candidate.rename.as_deref().unwrap_or(&candidate.name) == dependency
        })
        .map(|candidate| candidate.name.clone())
}

/// The facade features whose crate, at a path the facade names, states a
/// plugin: what `flui add` can link. A crate from a registry or git is
/// unread until it is linked, so it is not listed.
fn offered(facade: &Package) -> Vec<String> {
    facade
        .features
        .keys()
        .filter(|feature| {
            feature_crate(facade, feature)
                .and_then(|crate_name| {
                    facade
                        .dependencies
                        .iter()
                        .find(|dependency| dependency.name == crate_name)
                })
                .and_then(|dependency| dependency.path.as_ref())
                .and_then(|path| std::fs::read_to_string(path.join("Cargo.toml")).ok())
                .is_some_and(|manifest| matches!(plugin_of_manifest(&manifest), Ok(Some(_))))
        })
        .cloned()
        .collect()
}

/// The directory of `crate_name` in the workspace of a FLUI checkout the
/// application already depends on by path.
fn checkout_crate(root: &Package, crate_name: &str) -> Option<PathBuf> {
    root.dependencies
        .iter()
        .filter(|dependency| dependency.name.starts_with(FACADE))
        .filter_map(|dependency| dependency.path.as_ref())
        .find_map(|path| {
            let workspace = MetadataCommand::new()
                .manifest_path(path.join("Cargo.toml"))
                .no_deps()
                .exec()
                .ok()?;
            workspace
                .workspace_packages()
                .into_iter()
                .find(|package| package.name.as_str() == crate_name)
                .and_then(|package| package.manifest_path.parent())
                .map(|dir| dir.as_std_path().to_path_buf())
        })
}

/// Link `target` into the application at `dir`.
fn apply(target: &Target, manifest: &Path, dir: &Path) -> CliResult<()> {
    if target.linked() {
        return Ok(());
    }
    match target {
        Target::Feature { key, feature, .. } => {
            let text = std::fs::read_to_string(manifest)?;
            let edited =
                enable_feature(&text, key, feature).map_err(|reason| CliError::InvalidConfig {
                    file: "Cargo.toml".into(),
                    reason,
                })?;
            std::fs::write(manifest, edited)?;
            Ok(())
        }
        Target::Crate {
            crate_name, path, ..
        } => {
            let mut command = Command::new("cargo");
            command.current_dir(dir).arg("add");
            match path {
                Some(path) => command.arg("--path").arg(path),
                None => command.arg(crate_name),
            };
            let status = command.status().context("could not run cargo add")?;
            if status.success() {
                Ok(())
            } else {
                Err(CliError::CommandFailed {
                    context: format!("cargo add {crate_name}"),
                    exit_code: status.code(),
                })
            }
        }
    }
}

/// `manifest` with `feature` among the features of `[dependencies] key`,
/// the rest of the file as it was.
///
/// # Errors
///
/// Why the dependency is not in a shape this edit takes.
fn enable_feature(manifest: &str, key: &str, feature: &str) -> Result<String, String> {
    use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};
    let mut document = manifest
        .parse::<DocumentMut>()
        .map_err(|error| format!("is not TOML: {error}"))?;
    let entry = document
        .get_mut("dependencies")
        .and_then(Item::as_table_like_mut)
        .and_then(|dependencies| dependencies.get_mut(key))
        .filter(|entry| !entry.is_none())
        .ok_or_else(|| format!("has no `{key}` in [dependencies]"))?;
    let push = |features: &mut Array| {
        if !features.iter().any(|value| value.as_str() == Some(feature)) {
            features.push(feature);
        }
    };
    match entry {
        Item::Value(Value::String(version)) => {
            let mut table = InlineTable::new();
            table.insert("version", Value::from(version.value().as_str()));
            let mut features = Array::new();
            features.push(feature);
            table.insert("features", Value::Array(features));
            // The comment after the version stays after the table.
            let decor = version.decor().clone();
            let mut value = Value::InlineTable(table);
            *value.decor_mut() = decor;
            *entry = Item::Value(value);
        }
        Item::Value(Value::InlineTable(table)) => {
            if !table.contains_key("features") {
                table.insert("features", Value::Array(Array::new()));
                // A new key is spaced like the ones before it.
                table.fmt();
            }
            let Some(Value::Array(features)) = table.get_mut("features") else {
                return Err(format!("states `{key}`'s features, but not as an array"));
            };
            push(features);
        }
        Item::Table(table) => {
            let features = table
                .entry("features")
                .or_insert_with(|| Item::Value(Value::Array(Array::new())));
            let Some(features) = features.as_array_mut() else {
                return Err(format!("states `{key}`'s features, but not as an array"));
            };
            push(features);
        }
        _ => {
            return Err(format!(
                "states `{key}` in a shape `flui add` does not edit"
            ));
        }
    }
    Ok(document.to_string())
}

/// What the linked crate states: its plugin and its declarations, with the
/// features the graph now enables for it.
fn describe(dir: &Path, target: &Target) -> CliResult<(PluginPath, Declarations)> {
    let metadata = metadata(dir)?;
    let crate_name = target.crate_name();
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| CliError::Missing("cargo metadata resolved no graph".into()))?;
    let (package, node) = resolve
        .nodes
        .iter()
        .find_map(|node| {
            let package = metadata
                .packages
                .iter()
                .find(|package| package.id == node.id)?;
            (package.name.as_str() == crate_name).then_some((package, node))
        })
        .ok_or_else(|| {
            CliError::Missing(format!("`{crate_name}` is not in the application's graph"))
        })?;
    let refused = |reason: String| CliError::InvalidConfig {
        file: format!("{crate_name}'s Cargo.toml"),
        reason,
    };
    let plugin = plugin_of(&package.metadata)
        .map_err(|reason| refused(format!("[package.metadata.flui.capability]: {reason}")))?
        .ok_or_else(|| {
            refused(
                "states no [package.metadata.flui.capability] plugin, so it is not a \
                 capability crate"
                    .into(),
            )
        })?;
    let features: Vec<&str> = node
        .features
        .iter()
        .map(|feature| feature.as_str())
        .collect();
    let declarations =
        Declarations::of_package(package, &features).map_err(|error| refused(error.to_string()))?;
    Ok((plugin, declarations))
}

fn report(added: &Added<'_>) {
    ui::emit("add.done", added);
    let what = match added.feature {
        Some(feature) => format!("`{feature}` ({})", added.crate_name),
        None => format!("`{}`", added.crate_name),
    };
    let _ = if added.changed {
        ui::success(format!("Added {what}"))
    } else {
        ui::info(format!("{what} is already linked"))
    };
    let mut body = format!("Register it on the application:\n  {}", added.snippet);
    if !added.android_permissions.is_empty() || !added.apple_accessed_apis.is_empty() {
        body.push_str("\n\n`flui build` declares what it needs:");
        for permission in &added.android_permissions {
            body.push_str("\n  Android: ");
            body.push_str(permission);
        }
        for api in &added.apple_accessed_apis {
            body.push_str("\n  Apple privacy: ");
            body.push_str(api.category);
            body.push(' ');
            body.push_str(&api.reason);
        }
    }
    let _ = ui::note("Capability", body);
}

/// Files as they were before `add` changed them.
struct Snapshot(Vec<(PathBuf, Option<Vec<u8>>)>);

impl Snapshot {
    fn take(paths: &[&Path]) -> CliResult<Self> {
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let contents = match std::fs::read(path) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            files.push((path.to_path_buf(), contents));
        }
        Ok(Self(files))
    }

    fn restore(&self) -> CliResult<()> {
        for (path, contents) in &self.0 {
            match contents {
                Some(contents) => std::fs::write(path, contents)?,
                None => match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::enable_feature;

    /// The feature joins the dependency in whichever shape it is written,
    /// once, and the rest of the file is kept byte for byte.
    #[test]
    fn a_feature_joins_the_facade_dependency_in_any_shape() {
        for (manifest, expected) in [
            (
                "[dependencies]\nflui = \"0.2\" # the facade\nother = \"1\"\n",
                Ok(
                    "[dependencies]\nflui = { version = \"0.2\", features = [\"system\"] } # the facade\nother = \"1\"\n",
                ),
            ),
            (
                "[dependencies]\nflui = { path = \"../flui\", features = [\"material\"] }\n",
                Ok(
                    "[dependencies]\nflui = { path = \"../flui\", features = [\"material\", \"system\"] }\n",
                ),
            ),
            (
                "[dependencies]\nflui = { path = \"../flui\" }\n",
                Ok("[dependencies]\nflui = { path = \"../flui\", features = [\"system\"] }\n"),
            ),
            (
                "[dependencies]\nflui = { path = \"../flui\", features = [\"system\"] }\n",
                Ok("[dependencies]\nflui = { path = \"../flui\", features = [\"system\"] }\n"),
            ),
            (
                "[dependencies.flui]\npath = \"../flui\"\n",
                Ok("[dependencies.flui]\npath = \"../flui\"\nfeatures = [\"system\"]\n"),
            ),
            ("[dependencies]\nother = \"1\"\n", Err("has no `flui`")),
            (
                "[dependencies]\nflui = { path = \"../flui\", features = \"system\" }\n",
                Err("not as an array"),
            ),
        ] {
            let edited = enable_feature(manifest, "flui", "system");
            match (&edited, expected) {
                (Ok(edited), Ok(expected)) => assert_eq!(edited, expected, "{manifest}"),
                (Err(error), Err(expected)) => {
                    assert!(error.contains(expected), "{manifest:?}: {error}");
                }
                _ => panic!("{manifest:?}: {edited:?}"),
            }
        }
    }
}
