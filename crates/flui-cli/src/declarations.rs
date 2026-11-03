//! What the crates an application links ask of its platform manifests
//! (ADR-0307).
//!
//! Any crate of the application's graph, FLUI's own included, states what it
//! needs from the manifests in `[package.metadata.flui.declares]`; a Cargo
//! feature of the crate carries its own part under `features.<feature>`, so a
//! disabled feature removes both its code and its declarations. The CLI reads
//! the graph `cargo metadata` resolves for the target it builds, joins every
//! declaration, and writes the result into the manifests it generates
//! (`build::android_manifest`).
//!
//! The table is strict: an unknown key is an error naming the crate, and
//! `schema` names the version of the keys the crate uses, so a crate written
//! for a newer CLI is refused by name instead of being half-applied. Keys
//! arrive with their first consumer: version 1 has `android.permissions` and
//! `apple.privacy.accessed-apis`, version 2 adds `android.dex` and
//! `android.java`, the compiled Java classes a crate ships and their sources.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use cargo_metadata::{Metadata, Package, PackageId};
use serde::Deserialize;

use crate::build::error::{BuildError, BuildResult};

pub(crate) mod capability;
pub(crate) mod verify;

/// The newest `schema` this CLI reads.
pub(crate) const SCHEMA: u32 = 2;

/// The key under `[package.metadata.flui]` that holds a crate's
/// declarations.
const KEY: &str = "declares";

/// An Android permission a crate needs, such as
/// `android.permission.INTERNET`: two or more `.`-separated labels, each an
/// ASCII Java identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct AndroidPermission(String);

impl AndroidPermission {
    /// Parse `name`.
    ///
    /// # Errors
    ///
    /// Why `name` is not a permission name.
    pub(crate) fn try_new(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        let mut labels = name.split('.');
        let well_formed = name.split('.').count() >= 2
            && labels.all(|label| {
                label.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
        if well_formed {
            Ok(Self(name))
        } else {
            Err(format!(
                "`{name}` is not a permission name: two or more dot-separated labels of ASCII \
                 letters, digits and `_`, each starting with a letter or `_`"
            ))
        }
    }

    /// The permission's name, as the manifest's `<uses-permission>` states it.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AndroidPermission {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::try_new(name)
    }
}

impl Display for AndroidPermission {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One crate's `[package.metadata.flui.declares]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declares {
    /// The version of the keys the crate uses.
    schema: u32,
    /// What the crate needs from the Android manifest whatever its features.
    /// (Every key of [`Part`] is repeated here: `deny_unknown_fields` does
    /// not combine with `flatten`.)
    #[serde(default)]
    android: AndroidPart,
    /// What the crate needs from Apple's manifests whatever its features.
    #[serde(default)]
    apple: ApplePart,
    /// What each feature of the crate adds when it is enabled.
    #[serde(default)]
    features: BTreeMap<String, Part>,
}

/// The declarations of one crate, or of one of its features.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    #[serde(default)]
    android: AndroidPart,
    #[serde(default)]
    apple: ApplePart,
}

/// What a crate needs from Apple's manifests.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplePart {
    /// The privacy manifest (`PrivacyInfo.xcprivacy`).
    #[serde(default)]
    privacy: PrivacyPart,
}

/// What a crate puts in the privacy manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct PrivacyPart {
    /// Each required-reason API the crate's code calls, with its reasons.
    #[serde(default)]
    accessed_apis: Vec<AccessedApi>,
}

/// One `NSPrivacyAccessedAPITypes` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccessedApi {
    category: PrivacyCategory,
    reasons: Vec<PrivacyReason>,
}

/// A category of Apple's required-reason APIs: an API in one is refused at
/// upload unless the privacy manifest states why it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub(crate) enum PrivacyCategory {
    /// `stat`, `fstat`, `getattrlist`, file creation and modification dates.
    #[serde(rename = "NSPrivacyAccessedAPICategoryFileTimestamp")]
    FileTimestamp,
    /// `systemUptime`, `mach_absolute_time`.
    #[serde(rename = "NSPrivacyAccessedAPICategorySystemBootTime")]
    SystemBootTime,
    /// The free or total disk space.
    #[serde(rename = "NSPrivacyAccessedAPICategoryDiskSpace")]
    DiskSpace,
    /// The list of active keyboards.
    #[serde(rename = "NSPrivacyAccessedAPICategoryActiveKeyboards")]
    ActiveKeyboards,
    /// `NSUserDefaults`.
    #[serde(rename = "NSPrivacyAccessedAPICategoryUserDefaults")]
    UserDefaults,
}

impl PrivacyCategory {
    /// The category's key in the privacy manifest.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FileTimestamp => "NSPrivacyAccessedAPICategoryFileTimestamp",
            Self::SystemBootTime => "NSPrivacyAccessedAPICategorySystemBootTime",
            Self::DiskSpace => "NSPrivacyAccessedAPICategoryDiskSpace",
            Self::ActiveKeyboards => "NSPrivacyAccessedAPICategoryActiveKeyboards",
            Self::UserDefaults => "NSPrivacyAccessedAPICategoryUserDefaults",
        }
    }
}

/// An approved reason code, such as `C617.1`: four upper-case hexadecimal
/// digits, a `.` and a number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct PrivacyReason(String);

impl PrivacyReason {
    /// Parse `code`.
    ///
    /// # Errors
    ///
    /// Why `code` is not a reason code.
    pub(crate) fn try_new(code: impl Into<String>) -> Result<Self, String> {
        let code = code.into();
        let well_formed = code.split_once('.').is_some_and(|(id, number)| {
            id.len() == 4
                && id
                    .chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() && c.is_ascii_hexdigit())
                && !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
        });
        if well_formed {
            Ok(Self(code))
        } else {
            Err(format!(
                "`{code}` is not an approved-reason code such as `C617.1`: four upper-case \
                 hexadecimal digits, `.` and a number"
            ))
        }
    }

    /// The code, as the privacy manifest states it.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PrivacyReason {
    type Error = String;

    fn try_from(code: String) -> Result<Self, String> {
        Self::try_new(code)
    }
}

/// What a crate needs from the Android manifest and the APK.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidPart {
    /// Each `<uses-permission>`.
    #[serde(default)]
    permissions: Vec<AndroidPermission>,
    /// A dex the crate ships, which the APK carries (schema 2): the crate's
    /// compiled Java classes, which the Activity's class loader reads.
    #[serde(default)]
    dex: Option<CratePath>,
    /// The directory of the Java sources that dex is compiled from (schema
    /// 2), which a Gradle build compiles itself.
    #[serde(default)]
    java: Option<CratePath>,
}

/// A path inside a crate's directory, as its declarations name it:
/// `/`-separated, relative, never climbing out (`..`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct CratePath(String);

impl CratePath {
    /// Parse `path`.
    ///
    /// # Errors
    ///
    /// Why `path` is not a relative path inside the crate.
    pub(crate) fn try_new(path: impl Into<String>) -> Result<Self, String> {
        let path = path.into();
        let inside = !path.is_empty()
            && !path.starts_with('/')
            && !path.contains(['\\', ':'])
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if inside {
            Ok(Self(path))
        } else {
            Err(format!(
                "`{path}` is not a path inside the crate: relative, `/`-separated, without `.`, \
                 `..`, `\\` or `:`"
            ))
        }
    }

    /// The path under the crate directory `crate_dir`.
    #[must_use]
    pub(crate) fn under(&self, crate_dir: &Path) -> PathBuf {
        self.0
            .split('/')
            .fold(crate_dir.to_path_buf(), |path, part| path.join(part))
    }
}

impl TryFrom<String> for CratePath {
    type Error = String;

    fn try_from(path: String) -> Result<Self, String> {
        Self::try_new(path)
    }
}

impl Display for CratePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every declaration of the crates an application links, joined, with the
/// crates that ask for each.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Declarations {
    android_permissions: BTreeMap<AndroidPermission, BTreeSet<String>>,
    /// Each dex a crate ships, by crate and then path: the order of the APK's
    /// `classes.dex`, `classes2.dex`, ….
    android_dex: BTreeSet<(String, PathBuf)>,
    /// Each directory of Java sources, by crate and then path.
    android_java: BTreeSet<(String, PathBuf)>,
    apple_accessed_apis: BTreeMap<PrivacyCategory, BTreeMap<PrivacyReason, BTreeSet<String>>>,
}

impl Declarations {
    /// The declarations of the application at `app_dir` built for each of
    /// `targets` (Rust target triples): the crates `cargo metadata` resolves
    /// for that target with the features the build enables.
    ///
    /// # Errors
    ///
    /// The failure of `cargo metadata`, or [`BuildError::InvalidConfig`] for a
    /// crate whose declarations are malformed.
    pub(crate) fn collect(app_dir: &Path, targets: &[&str]) -> BuildResult<Self> {
        let mut joined = Self::default();
        for target in targets {
            let metadata = cargo_metadata::MetadataCommand::new()
                .current_dir(app_dir)
                .other_options(vec!["--filter-platform".to_owned(), (*target).to_owned()])
                .exec()
                .map_err(|error| {
                    BuildError::Other(format!("cargo metadata for {target}: {error}"))
                })?;
            joined.join(Self::of_graph(&metadata)?);
        }
        Ok(joined)
    }

    /// The declarations of every crate in `metadata`'s resolved graph, each
    /// with the features the graph enables for it.
    fn of_graph(metadata: &Metadata) -> BuildResult<Self> {
        let resolve = metadata.resolve.as_ref().ok_or_else(|| {
            BuildError::Other("cargo metadata resolved no dependency graph".to_owned())
        })?;
        let packages: BTreeMap<&PackageId, &Package> = metadata
            .packages
            .iter()
            .map(|package| (&package.id, package))
            .collect();
        let mut joined = Self::default();
        for node in &resolve.nodes {
            let package = packages.get(&node.id).ok_or_else(|| {
                BuildError::Other(format!("cargo metadata lists no package {}", node.id))
            })?;
            let features: Vec<&str> = node
                .features
                .iter()
                .map(|feature| feature.as_str())
                .collect();
            joined.add(package, &features)?;
        }
        Ok(joined)
    }

    /// What `package` alone declares, with `features` enabled.
    ///
    /// # Errors
    ///
    /// As [`Declarations::collect`], for this one crate's table.
    pub(crate) fn of_package(package: &Package, features: &[&str]) -> BuildResult<Self> {
        let mut declarations = Self::default();
        declarations.add(package, features)?;
        Ok(declarations)
    }

    /// Add what `package` declares, with `features` enabled.
    fn add(&mut self, package: &Package, features: &[&str]) -> BuildResult<()> {
        let Some(table) = package.metadata.get("flui").and_then(|flui| flui.get(KEY)) else {
            return Ok(());
        };
        let name = package.name.as_str();
        let refused = |reason: String| {
            BuildError::invalid_config(format!("{name}'s [package.metadata.flui.{KEY}]"), reason)
        };
        let declares = Declares::deserialize(table).map_err(|error| refused(error.to_string()))?;
        if declares.schema == 0 || declares.schema > SCHEMA {
            return Err(refused(format!(
                "states schema {}, and this flui-cli reads schema 1 to {SCHEMA}; update flui-cli",
                declares.schema
            )));
        }
        if let Some(unknown) = declares
            .features
            .keys()
            .find(|feature| !package.features.contains_key(feature.as_str()))
        {
            return Err(refused(format!(
                "declares for feature `{unknown}`, which {name} does not have"
            )));
        }
        self.add_android(package, declares.schema, &declares.android)?;
        self.add_apple(name, &declares.apple)?;
        for (feature, part) in &declares.features {
            if features.contains(&feature.as_str()) {
                self.add_android(package, declares.schema, &part.android)?;
                self.add_apple(name, &part.apple)?;
            }
        }
        Ok(())
    }

    fn add_apple(&mut self, crate_name: &str, apple: &ApplePart) -> BuildResult<()> {
        for api in &apple.privacy.accessed_apis {
            if api.reasons.is_empty() {
                return Err(BuildError::invalid_config(
                    format!("{crate_name}'s [package.metadata.flui.{KEY}]"),
                    format!(
                        "declares {} with no reasons; Apple refuses an accessed API without one",
                        api.category.as_str()
                    ),
                ));
            }
            let reasons = self.apple_accessed_apis.entry(api.category).or_default();
            for reason in &api.reasons {
                reasons
                    .entry(reason.clone())
                    .or_default()
                    .insert(crate_name.to_owned());
            }
        }
        Ok(())
    }

    /// Add the Android part of `package`'s table, which states `schema`.
    fn add_android(
        &mut self,
        package: &Package,
        schema: u32,
        android: &AndroidPart,
    ) -> BuildResult<()> {
        let crate_name = package.name.as_str();
        for permission in &android.permissions {
            self.android_permissions
                .entry(permission.clone())
                .or_default()
                .insert(crate_name.to_owned());
        }
        let refused = |reason: String| {
            BuildError::invalid_config(
                format!("{crate_name}'s [package.metadata.flui.{KEY}]"),
                reason,
            )
        };
        let crate_dir = package
            .manifest_path
            .parent()
            .map(|dir| dir.as_std_path().to_path_buf())
            .unwrap_or_default();
        for (key, path, is_dir) in [("dex", &android.dex, false), ("java", &android.java, true)] {
            let Some(path) = path else { continue };
            if schema < 2 {
                return Err(refused(format!(
                    "declares android.{key}, which schema 2 introduced; state `schema = 2`"
                )));
            }
            let resolved = path.under(&crate_dir);
            let present = if is_dir {
                resolved.is_dir()
            } else {
                resolved.is_file()
            };
            if !present {
                return Err(refused(format!(
                    "declares android.{key} = \"{path}\", and {} is no {}",
                    resolved.display(),
                    if is_dir { "directory" } else { "file" }
                )));
            }
            let joined = if is_dir {
                &mut self.android_java
            } else {
                &mut self.android_dex
            };
            joined.insert((crate_name.to_owned(), resolved));
        }
        Ok(())
    }

    fn join(&mut self, other: Self) {
        for (permission, crates) in other.android_permissions {
            self.android_permissions
                .entry(permission)
                .or_default()
                .extend(crates);
        }
        self.android_dex.extend(other.android_dex);
        self.android_java.extend(other.android_java);
        for (category, reasons) in other.apple_accessed_apis {
            let joined = self.apple_accessed_apis.entry(category).or_default();
            for (reason, crates) in reasons {
                joined.entry(reason).or_default().extend(crates);
            }
        }
    }

    /// Every Android permission declared, in order, with the crates that ask
    /// for it.
    pub(crate) fn android_permissions(
        &self,
    ) -> impl Iterator<Item = (&AndroidPermission, &BTreeSet<String>)> {
        self.android_permissions.iter()
    }

    /// Every dex the linked crates ship, in the order the APK numbers them
    /// (`classes.dex`, `classes2.dex`, …), with the crate that ships it.
    pub(crate) fn android_dex(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.android_dex
            .iter()
            .map(|(crate_name, path)| (crate_name.as_str(), path.as_path()))
    }

    /// Every directory of Java sources the linked crates ship, in order, with
    /// the crate that ships it.
    pub(crate) fn android_java(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.android_java
            .iter()
            .map(|(crate_name, path)| (crate_name.as_str(), path.as_path()))
    }

    /// Every required-reason API category declared, in order, with each of
    /// its reasons and the crates that give it.
    pub(crate) fn apple_accessed_apis(
        &self,
    ) -> impl Iterator<Item = (PrivacyCategory, &BTreeMap<PrivacyReason, BTreeSet<String>>)> {
        self.apple_accessed_apis
            .iter()
            .map(|(category, reasons)| (*category, reasons))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{AndroidPermission, Declarations, PrivacyReason};

    /// A fixture project: `members` written as `(directory, Cargo.toml)`
    /// under one temporary directory, with an empty `src/lib.rs` each.
    fn fixture(members: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for (name, manifest) in members {
            let crate_dir = dir.path().join(name);
            std::fs::create_dir_all(crate_dir.join("src")).expect("the crate directory");
            std::fs::write(crate_dir.join("Cargo.toml"), manifest).expect("the manifest");
            std::fs::write(crate_dir.join("src").join("lib.rs"), "").expect("the source");
        }
        dir
    }

    /// A crate's manifest; `rest` follows its `[package]` table.
    fn manifest(name: &str, rest: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{rest}")
    }

    fn permissions(declarations: &Declarations) -> Vec<(String, Vec<String>)> {
        declarations
            .android_permissions()
            .map(|(permission, crates)| {
                (
                    permission.as_str().to_owned(),
                    crates.iter().cloned().collect(),
                )
            })
            .collect()
    }

    type Reasons = Vec<(String, Vec<String>)>;

    fn accessed_apis(declarations: &Declarations) -> Vec<(&'static str, Reasons)> {
        declarations
            .apple_accessed_apis()
            .map(|(category, reasons)| {
                (
                    category.as_str(),
                    reasons
                        .iter()
                        .map(|(reason, crates)| {
                            (reason.as_str().to_owned(), crates.iter().cloned().collect())
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn collect(app: &Path, target: &str) -> Result<Declarations, String> {
        Declarations::collect(app, &[target]).map_err(|error| error.to_string())
    }

    /// A crate's own declarations reach the application through any depth
    /// of dependency; a feature's part counts only when the graph enables the
    /// feature; a dependency only another target links contributes nothing;
    /// each permission lists every crate that asks for it.
    #[test]
    fn declarations_are_joined_over_the_graph_the_target_links() {
        let project = fixture(&[
            (
                "app",
                &manifest(
                    "app",
                    "[workspace]\n[dependencies]\nmiddle = { path = \"../middle\" }\n\
                     http_lib = { path = \"../http_lib\", features = [\"http\"] }\n\
                     quiet = { path = \"../quiet\" }\n\
                     [target.'cfg(target_os = \"linux\")'.dependencies]\n\
                     linux_only = { path = \"../linux_only\" }\n",
                ),
            ),
            (
                "middle",
                &manifest("middle", "[dependencies]\nleaf = { path = \"../leaf\" }\n"),
            ),
            (
                "leaf",
                &manifest(
                    "leaf",
                    "[package.metadata.flui.declares]\nschema = 1\n\
                     [package.metadata.flui.declares.android]\n\
                     permissions = [\"android.permission.VIBRATE\"]\n\
                     [[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
                     category = \"NSPrivacyAccessedAPICategoryFileTimestamp\"\n\
                     reasons = [\"C617.1\"]\n",
                ),
            ),
            (
                "http_lib",
                &manifest(
                    "http_lib",
                    "[features]\nhttp = []\ncamera = []\n\
                     [package.metadata.flui.declares]\nschema = 1\n\
                     [package.metadata.flui.declares.android]\n\
                     permissions = [\"android.permission.VIBRATE\"]\n\
                     [package.metadata.flui.declares.features.http.android]\n\
                     permissions = [\"android.permission.INTERNET\"]\n\
                     [[package.metadata.flui.declares.features.http.apple.privacy.accessed-apis]]\n\
                     category = \"NSPrivacyAccessedAPICategoryFileTimestamp\"\n\
                     reasons = [\"0A2A.1\", \"C617.1\"]\n\
                     [package.metadata.flui.declares.features.camera.android]\n\
                     permissions = [\"android.permission.CAMERA\"]\n\
                     [[package.metadata.flui.declares.features.camera.apple.privacy.accessed-apis]]\n\
                     category = \"NSPrivacyAccessedAPICategoryUserDefaults\"\n\
                     reasons = [\"CA92.1\"]\n",
                ),
            ),
            ("quiet", &manifest("quiet", "")),
            (
                "linux_only",
                &manifest(
                    "linux_only",
                    "[package.metadata.flui.declares]\nschema = 1\n\
                     [package.metadata.flui.declares.android]\n\
                     permissions = [\"android.permission.READ_CONTACTS\"]\n",
                ),
            ),
        ]);
        let app = project.path().join("app");
        let android = collect(&app, "aarch64-linux-android").expect("declarations");
        assert_eq!(
            permissions(&android),
            [
                (
                    "android.permission.INTERNET".to_owned(),
                    vec!["http_lib".to_owned()]
                ),
                (
                    "android.permission.VIBRATE".to_owned(),
                    vec!["http_lib".to_owned(), "leaf".to_owned()]
                ),
            ]
        );
        let ios = collect(&app, "aarch64-apple-ios").expect("declarations");
        assert_eq!(
            accessed_apis(&ios),
            [(
                "NSPrivacyAccessedAPICategoryFileTimestamp",
                vec![
                    ("0A2A.1".to_owned(), vec!["http_lib".to_owned()]),
                    (
                        "C617.1".to_owned(),
                        vec!["http_lib".to_owned(), "leaf".to_owned()]
                    ),
                ]
            )]
        );
        let linux = collect(&app, "x86_64-unknown-linux-gnu").expect("declarations");
        assert!(
            permissions(&linux)
                .iter()
                .any(|(permission, _)| permission == "android.permission.READ_CONTACTS"),
            "a dependency the target links contributes: {linux:?}"
        );
    }

    /// Each crate's dex and Java sources reach the APK resolved under that
    /// crate's own directory, in crate order (the order of `classes.dex`,
    /// `classes2.dex`, …); a feature's dex counts only when the graph enables
    /// the feature.
    #[test]
    fn a_crates_dex_and_java_sources_resolve_under_the_crate() {
        let project = fixture(&[
            (
                "app",
                &manifest(
                    "app",
                    "[workspace]\n[dependencies]\nzeta = { path = \"../zeta\" }\n\
                     alpha = { path = \"../alpha\" }\n",
                ),
            ),
            (
                "zeta",
                &manifest(
                    "zeta",
                    "[package.metadata.flui.declares]\nschema = 2\n\
                     [package.metadata.flui.declares.android]\n\
                     dex = \"android/classes.dex\"\njava = \"android/java\"\n",
                ),
            ),
            (
                "alpha",
                &manifest(
                    "alpha",
                    "[features]\nextra = []\n\
                     [package.metadata.flui.declares]\nschema = 2\n\
                     [package.metadata.flui.declares.android]\n\
                     dex = \"android/classes.dex\"\n\
                     [package.metadata.flui.declares.features.extra.android]\n\
                     dex = \"android/missing.dex\"\n",
                ),
            ),
        ]);
        for (path, directory) in [
            ("zeta/android/classes.dex", false),
            ("zeta/android/java", true),
            ("alpha/android/classes.dex", false),
        ] {
            let path = project.path().join(path);
            if directory {
                std::fs::create_dir_all(&path).expect("the directory");
            } else {
                std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
                std::fs::write(&path, b"dex\n035\0").expect("the dex");
            }
        }
        let android =
            collect(&project.path().join("app"), "aarch64-linux-android").expect("declarations");
        let relative = |entries: Vec<(&str, &Path)>| -> Vec<(String, std::path::PathBuf)> {
            entries
                .into_iter()
                .map(|(crate_name, path)| {
                    let path = path.canonicalize().expect("a declared path exists");
                    let root = project.path().canonicalize().expect("the project");
                    (
                        crate_name.to_owned(),
                        path.strip_prefix(root)
                            .expect("under the project")
                            .to_path_buf(),
                    )
                })
                .collect()
        };
        assert_eq!(
            relative(android.android_dex().collect()),
            [
                (
                    "alpha".to_owned(),
                    Path::new("alpha/android/classes.dex").to_path_buf()
                ),
                (
                    "zeta".to_owned(),
                    Path::new("zeta/android/classes.dex").to_path_buf()
                ),
            ]
        );
        assert_eq!(
            relative(android.android_java().collect()),
            [(
                "zeta".to_owned(),
                Path::new("zeta/android/java").to_path_buf()
            )]
        );
    }

    /// A malformed table is refused naming the crate and what is wrong: an
    /// unknown key, a missing or newer schema, a key newer than the schema
    /// the crate states, a path outside the crate or naming nothing, a feature
    /// the crate does not have, a permission that is not a name, a
    /// required-reason category Apple does not define, an accessed API without
    /// a reason or with a malformed one.
    #[test]
    fn a_malformed_declaration_is_refused_by_crate() {
        let cases = [
            (
                "schema = 1\n[package.metadata.flui.declares.android]\npermission = [\"a.B\"]\n",
                "unknown field `permission`",
            ),
            (
                "schema = 1\n[package.metadata.flui.declares.ios]\nkeys = []\n",
                "unknown field `ios`",
            ),
            (
                "[package.metadata.flui.declares.android]\npermissions = [\"a.B\"]\n",
                "missing field `schema`",
            ),
            ("schema = 3\n", "states schema 3"),
            ("schema = 0\n", "states schema 0"),
            (
                "schema = 1\n[package.metadata.flui.declares.android]\ndex = \"src/lib.rs\"\n",
                "declares android.dex, which schema 2 introduced",
            ),
            (
                "schema = 2\n[package.metadata.flui.declares.android]\n\
                 dex = \"../lib/src/lib.rs\"\n",
                "is not a path inside the crate",
            ),
            (
                "schema = 2\n[package.metadata.flui.declares.android]\ndex = \"classes.dex\"\n",
                "classes.dex is no file",
            ),
            (
                "schema = 2\n[package.metadata.flui.declares.android]\njava = \"src/lib.rs\"\n",
                "is no directory",
            ),
            (
                "schema = 1\n[package.metadata.flui.declares.features.camera.android]\n\
                 permissions = [\"a.B\"]\n",
                "feature `camera`, which lib does not have",
            ),
            (
                "schema = 1\n[package.metadata.flui.declares.android]\n\
                 permissions = [\"INTERNET\"]\n",
                "`INTERNET` is not a permission name",
            ),
            (
                "schema = 1\n[package.metadata.flui.declares.android]\n\
                 permissions = [\"a.b\\\"/><x a=\\\"\"]\n",
                "is not a permission name",
            ),
            (
                "schema = 1\n[[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
                 category = \"NSPrivacyAccessedAPICategoryLocation\"\nreasons = [\"C617.1\"]\n",
                "unknown variant `NSPrivacyAccessedAPICategoryLocation`",
            ),
            (
                "schema = 1\n[[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
                 category = \"NSPrivacyAccessedAPICategoryDiskSpace\"\nreasons = []\n",
                "NSPrivacyAccessedAPICategoryDiskSpace with no reasons",
            ),
            (
                "schema = 1\n[[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
                 category = \"NSPrivacyAccessedAPICategoryDiskSpace\"\nreasons = [\"e174.1\"]\n",
                "`e174.1` is not an approved-reason code",
            ),
            (
                "schema = 1\n[package.metadata.flui.declares.apple.privacy]\naccessed_apis = []\n",
                "unknown field `accessed_apis`",
            ),
        ];
        let mut failures = Vec::new();
        for (table, expected) in cases {
            let project = fixture(&[(
                "lib",
                &manifest(
                    "lib",
                    &format!("[workspace]\n[package.metadata.flui.declares]\n{table}"),
                ),
            )]);
            match collect(&project.path().join("lib"), "aarch64-linux-android") {
                Err(error) if error.contains(expected) && error.contains("lib's") => {}
                other => failures.push(format!("{table:?}: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn a_permission_is_a_dotted_java_name() {
        for (name, accepted) in [
            ("android.permission.INTERNET", true),
            ("com.example.permission.MAPS_RECEIVE", true),
            ("_a.b2", true),
            ("INTERNET", false),
            ("android..INTERNET", false),
            ("android.permission.", false),
            ("2d.permission", false),
            ("android.permission.INTER NET", false),
            ("android.permission.\"x", false),
        ] {
            assert_eq!(AndroidPermission::try_new(name).is_ok(), accepted, "{name}");
        }
    }

    #[test]
    fn a_privacy_reason_is_an_approved_reason_code() {
        for (code, accepted) in [
            ("C617.1", true),
            ("35F9.1", true),
            ("CA92.12", true),
            ("c617.1", false),
            ("C617", false),
            ("C617.", false),
            ("C61.1", false),
            ("C6177.1", false),
            ("G617.1", false),
            ("C617.1a", false),
        ] {
            assert_eq!(PrivacyReason::try_new(code).is_ok(), accepted, "{code}");
        }
    }
}
