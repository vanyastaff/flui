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
//! arrive with their first consumer; version 1 has `android.permissions`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use cargo_metadata::{Metadata, Package, PackageId};
use serde::Deserialize;

use crate::build::error::{BuildError, BuildResult};

pub(crate) mod capability;
pub(crate) mod verify;

/// The newest `schema` this CLI reads.
pub(crate) const SCHEMA: u32 = 1;

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

/// What a crate needs from the Android manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidPart {
    /// Each `<uses-permission>`.
    #[serde(default)]
    permissions: Vec<AndroidPermission>,
}

/// Every declaration of the crates an application links, joined, with the
/// crates that ask for each.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Declarations {
    android_permissions: BTreeMap<AndroidPermission, BTreeSet<String>>,
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
        self.add_android(name, &declares.android);
        self.add_apple(name, &declares.apple)?;
        for (feature, part) in &declares.features {
            if features.contains(&feature.as_str()) {
                self.add_android(name, &part.android);
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

    fn add_android(&mut self, crate_name: &str, android: &AndroidPart) {
        for permission in &android.permissions {
            self.android_permissions
                .entry(permission.clone())
                .or_default()
                .insert(crate_name.to_owned());
        }
    }

    fn join(&mut self, other: Self) {
        for (permission, crates) in other.android_permissions {
            self.android_permissions
                .entry(permission)
                .or_default()
                .extend(crates);
        }
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

    /// A malformed table is refused naming the crate and what is wrong: an
    /// unknown key, a missing or newer schema, a feature the crate does not
    /// have, a permission that is not a name, a required-reason category Apple
    /// does not define, an accessed API without a reason or with a malformed
    /// one.
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
            ("schema = 2\n", "states schema 2"),
            ("schema = 0\n", "states schema 0"),
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
