//! `[package.metadata.flui.capability]`: how an application registers a
//! capability crate (ADR-0307 §5), read by `flui add`.
//!
//! The one key, `plugin`, is the path of the crate's `Plugin` type from the
//! crate root, such as `SystemPlugin` or `services::LocationPlugin`. The type
//! implements `Default`; a test of the crate itself checks that the declared
//! path names it, so the declaration cannot go stale.

use std::fmt::{self, Display, Formatter};

use serde::Deserialize;

/// The capability table, strict like the declarations.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityTable {
    plugin: PluginPath,
}

/// A path from a crate root to its `Plugin` type: identifiers joined by `::`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct PluginPath(String);

impl PluginPath {
    /// Parse `path`.
    ///
    /// # Errors
    ///
    /// Why `path` is not a path of Rust identifiers, such as a `::` prefix,
    /// generics or a call.
    pub(crate) fn try_new(path: impl Into<String>) -> Result<Self, String> {
        let path = path.into();
        let identifier = |segment: &str| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                && segment != "_"
        };
        if path.split("::").all(identifier) {
            Ok(Self(path))
        } else {
            Err(format!(
                "`{path}` is not a type path from the crate root such as `SystemPlugin` or \
                 `services::LocationPlugin`"
            ))
        }
    }
}

impl TryFrom<String> for PluginPath {
    type Error = String;

    fn try_from(path: String) -> Result<Self, String> {
        Self::try_new(path)
    }
}

impl Display for PluginPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The plugin a crate's `[package.metadata]` declares, given as the JSON
/// `cargo metadata` reports, or `None` when it has no capability table.
///
/// # Errors
///
/// Why the table is malformed: an unknown key, a missing or malformed
/// `plugin`.
pub(crate) fn plugin_of(metadata: &serde_json::Value) -> Result<Option<PluginPath>, String> {
    let Some(table) = metadata.get("flui").and_then(|flui| flui.get("capability")) else {
        return Ok(None);
    };
    CapabilityTable::deserialize(table)
        .map(|table| Some(table.plugin))
        .map_err(|error| error.to_string())
}

/// As [`plugin_of`], for a crate's `Cargo.toml` read from disk.
///
/// # Errors
///
/// As [`plugin_of`], and when `manifest` is not TOML.
pub(crate) fn plugin_of_manifest(manifest: &str) -> Result<Option<PluginPath>, String> {
    let manifest: toml::Table = manifest.parse().map_err(|error| format!("{error}"))?;
    let Some(metadata) = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
    else {
        return Ok(None);
    };
    let metadata = serde_json::to_value(metadata).map_err(|error| error.to_string())?;
    plugin_of(&metadata)
}

#[cfg(test)]
mod tests {
    use super::{PluginPath, plugin_of_manifest};

    #[test]
    fn a_plugin_is_a_type_path_from_the_crate_root() {
        for (path, accepted) in [
            ("SystemPlugin", true),
            ("services::LocationPlugin", true),
            ("_Private2", true),
            ("SystemPlugin::new()", false),
            ("::SystemPlugin", false),
            ("Plugin<T>", false),
            ("a::::B", false),
            ("2D", false),
            ("", false),
            ("_", false),
        ] {
            assert_eq!(PluginPath::try_new(path).is_ok(), accepted, "{path}");
        }
        for (manifest, expected) in [
            ("[package]\nname = \"a\"\n", Ok(None)),
            (
                "[package.metadata.flui.capability]\nplugin = \"SystemPlugin\"\n",
                Ok(Some("SystemPlugin")),
            ),
            (
                "[package.metadata.flui.capability]\nplugin = \"P\"\nname = \"x\"\n",
                Err("unknown field `name`"),
            ),
            (
                "[package.metadata.flui.capability]\n",
                Err("missing field `plugin`"),
            ),
        ] {
            let read = plugin_of_manifest(manifest);
            match (&read, expected) {
                (Ok(plugin), Ok(expected)) => {
                    assert_eq!(
                        plugin.as_ref().map(ToString::to_string).as_deref(),
                        expected,
                        "{manifest}"
                    );
                }
                (Err(error), Err(expected)) => assert!(error.contains(expected), "{error}"),
                _ => panic!("{manifest:?}: {read:?}"),
            }
        }
    }
}
