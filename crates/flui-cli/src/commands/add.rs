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

use cargo_metadata::{Dependency, DependencyKind, Metadata, MetadataCommand, Package};
use serde::Serialize;

use crate::declarations::Declarations;
use crate::declarations::capability::{PluginPath, plugin_of, plugin_of_manifest};
use crate::declarations::verify::ManifestKind;
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
        /// The crate's key in the facade's `[dependencies]`.
        crate_key: String,
        crate_name: String,
        enabled: bool,
    },
    /// A crate added directly.
    Crate {
        /// The crate's key in the application's `[dependencies]`: its
        /// rename when the application already links it under one, else its
        /// name, which `cargo add` gives it.
        key: String,
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
            Self::Crate { key, .. } => key.replace('-', "_"),
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
    let before = metadata(&dir, None)?;
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
    let Described {
        plugin,
        android,
        apple,
    } = match added {
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
        android_permissions: android
            .android_permissions()
            .map(|(permission, _)| permission.to_string())
            .collect(),
        apple_accessed_apis: apple
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

/// The graph `cargo metadata` resolves at `dir`, for every platform or for
/// the target triple `platform`.
fn metadata(dir: &Path, platform: Option<&str>) -> CliResult<Metadata> {
    let mut command = MetadataCommand::new();
    command.current_dir(dir);
    if let Some(triple) = platform {
        command.other_options(vec!["--filter-platform".to_owned(), triple.to_owned()]);
    }
    command.exec().context("cargo metadata failed")
}

/// What `name` means for the application `root` of `metadata`.
fn resolve(metadata: &Metadata, root: &Package, name: &str) -> CliResult<Target> {
    // The application's normal dependency on the package `crate_name`, by
    // whatever key it names it.
    let direct = |crate_name: &str| {
        root.dependencies.iter().find(|dependency| {
            dependency.name == crate_name && dependency.kind == DependencyKind::Normal
        })
    };
    let facade = root.dependencies.iter().find(|dependency| {
        dependency.name == FACADE
            && dependency.kind == DependencyKind::Normal
            && dependency.target.is_none()
    });
    let Some(facade) = facade else {
        let linked = direct(name);
        return Ok(Target::Crate {
            key: linked.map_or(name, key_of).to_owned(),
            crate_name: name.to_owned(),
            present: linked.is_some(),
            path: checkout_crate(root, name),
        });
    };
    let key = facade.rename.clone().unwrap_or_else(|| FACADE.to_owned());
    let facade_package = dependency_package(metadata, root, &key)?.ok_or_else(|| {
        CliError::Missing(format!(
            "`{key}` resolves to no package of cargo metadata's graph"
        ))
    })?;
    if let Some(dependency) = feature_crate(facade_package, name) {
        return Ok(Target::Feature {
            key,
            feature: name.to_owned(),
            crate_key: key_of(dependency).to_owned(),
            crate_name: dependency.name.clone(),
            enabled: facade.features.iter().any(|feature| feature == name)
                || (facade.uses_default_features
                    && facade_package
                        .features
                        .get("default")
                        .is_some_and(|default| default.iter().any(|feature| feature == name))),
        });
    }
    if let Some(linked) = direct(name) {
        return Ok(Target::Crate {
            key: key_of(linked).to_owned(),
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

/// The key `dependency` has in its `[dependencies]`: its rename, or its
/// package's name.
fn key_of(dependency: &Dependency) -> &str {
    dependency.rename.as_deref().unwrap_or(&dependency.name)
}

/// The package `from`'s normal dependency of key `key` resolves to in
/// `metadata`'s graph: the package of `from`'s edge for it, so another
/// package of the same name elsewhere in the graph is not mistaken for it.
/// An edge counts for a declaration of that key whose target table it was
/// resolved for, so a graph of one platform (`--filter-platform`) finds the
/// package that platform's table names; `None` when that graph has no edge
/// for the key.
///
/// # Errors
///
/// `from` declares no normal dependency of key `key`, or two of its edges
/// in this graph match one.
fn dependency_package<'m>(
    metadata: &'m Metadata,
    from: &Package,
    key: &str,
) -> CliResult<Option<&'m Package>> {
    let unresolved = |what: &str| {
        CliError::Missing(format!(
            "`{}`'s dependency `{key}` resolves to {what} of cargo metadata's graph",
            from.name
        ))
    };
    let declared: Vec<&Dependency> = from
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == DependencyKind::Normal && key_of(dependency) == key)
        .collect();
    let [first, ..] = declared.as_slice() else {
        return Err(unresolved("no declaration"));
    };
    // Cargo names an edge by the crate name code uses: a rename, which the
    // key is, else the library's name, which no rename of `from` takes.
    let renames: Vec<String> = from
        .dependencies
        .iter()
        .filter_map(|dependency| dependency.rename.as_ref())
        .map(|rename| rename.replace('-', "_"))
        .collect();
    let names_this_dependency = |edge: &str| match &first.rename {
        Some(rename) => edge == rename.replace('-', "_"),
        None => !renames.iter().any(|rename| rename == edge),
    };
    let node = metadata
        .resolve
        .as_ref()
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == from.id))
        .ok_or_else(|| unresolved("no node"))?;
    let mut packages = node
        .deps
        .iter()
        .filter(|edge| {
            names_this_dependency(&edge.name)
                && edge.dep_kinds.iter().any(|kind| {
                    kind.kind == DependencyKind::Normal
                        && declared
                            .iter()
                            .any(|dependency| dependency.target == kind.target)
                })
        })
        .filter_map(|edge| {
            metadata
                .packages
                .iter()
                .find(|package| package.id == edge.pkg)
        })
        .filter(|package| {
            declared
                .iter()
                .any(|dependency| dependency.name == package.name.as_str())
        });
    match (packages.next(), packages.next()) {
        (Some(_), Some(_)) => Err(unresolved("several packages")),
        (package, _) => Ok(package),
    }
}

/// The capability crate `target` names in `metadata`'s graph, through the
/// application's dependency edge, or the facade's for a facade feature;
/// `None` when that graph does not link it.
fn linked_package<'m>(
    metadata: &'m Metadata,
    application: &Package,
    target: &Target,
) -> CliResult<Option<&'m Package>> {
    match target {
        Target::Feature { key, crate_key, .. } => {
            match dependency_package(metadata, application, key)? {
                Some(facade) => dependency_package(metadata, facade, crate_key),
                None => Ok(None),
            }
        }
        Target::Crate { key, .. } => dependency_package(metadata, application, key),
    }
}

/// The facade's dependency its feature `feature` enables, when it enables
/// one optional dependency and nothing else.
fn feature_crate<'p>(facade: &'p Package, feature: &str) -> Option<&'p Dependency> {
    let [enabled] = facade.features.get(feature)?.as_slice() else {
        return None;
    };
    let dependency = enabled.strip_prefix("dep:")?;
    facade
        .dependencies
        .iter()
        .find(|candidate| candidate.optional && key_of(candidate) == dependency)
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

/// What [`describe`] reads of the linked crate.
struct Described {
    plugin: PluginPath,
    /// Its declarations in the application's Android build.
    android: Declarations,
    /// Its declarations in the application's iOS build.
    apple: Declarations,
}

/// What the linked crate states: its plugin, and its declarations for each
/// platform whose manifests the application's build writes, from that
/// platform's build, whatever the host: what a feature only the iOS build
/// turns on declares for Android is not the Android build's. Each target's
/// graph resolves the crate through its own edges, so an application that
/// links one version for Android and another for iOS reads each; the host's
/// graph counts for the plugin of a crate only the desktop links.
fn describe(dir: &Path, target: &Target) -> CliResult<Described> {
    let crate_name = target.crate_name();
    let refused = |reason: String| CliError::InvalidConfig {
        file: format!("{crate_name}'s Cargo.toml"),
        reason,
    };
    let host = crate::build::DesktopBuilder::detect_host_target(dir)?;
    let platforms = ManifestKind::Android
        .targets()
        .iter()
        .map(|triple| (*triple, Some(ManifestKind::Android)))
        .chain(
            ManifestKind::ApplePrivacy
                .targets()
                .iter()
                .map(|triple| (*triple, Some(ManifestKind::ApplePrivacy))),
        )
        .chain([(host.as_str(), None)]);
    let mut plugin: Option<PluginPath> = None;
    let mut android = Declarations::default();
    let mut apple = Declarations::default();
    for (triple, kind) in platforms {
        let metadata = metadata(dir, Some(triple))?;
        let application = metadata.root_package().ok_or_else(|| {
            CliError::Missing(
                "cargo metadata names no package in the application's directory".into(),
            )
        })?;
        let Some(package) = linked_package(&metadata, application, target)? else {
            continue;
        };
        let stated = plugin_of(&package.metadata)
            .map_err(|reason| refused(format!("[package.metadata.flui.capability]: {reason}")))?
            .ok_or_else(|| {
                refused(
                    "states no [package.metadata.flui.capability] plugin, so it is not a \
                     capability crate"
                        .into(),
                )
            })?;
        match &plugin {
            Some(earlier) if *earlier != stated => {
                return Err(refused(format!(
                    "is linked as {} {} for {triple}, whose plugin `{stated}` is not the \
                     `{earlier}` another target's build links",
                    package.name, package.version
                )));
            }
            _ => plugin = Some(stated),
        }
        let declarations =
            Declarations::of_linked_crate(dir, &metadata, application, package, &[triple])?;
        match kind {
            Some(ManifestKind::Android) => android.join(declarations)?,
            Some(_) => apple.join(declarations)?,
            None => {}
        }
    }
    let plugin = plugin.ok_or_else(|| {
        CliError::Missing(format!(
            "no build of the application, for Android, iOS or {host}, links `{crate_name}`"
        ))
    })?;
    Ok(Described {
        plugin,
        android,
        apple,
    })
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
