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
//! `apple.privacy.accessed-apis`, version 2 adds `android.dex`,
//! `android.java` and `android.proguard`: the compiled Java classes a crate
//! ships, their sources and the R8 rules that keep what native code reaches,
//! and version 3 adds `url-schemes`, `associated-domains` and `file-types`:
//! what the application opens, which the operating system has to know before
//! it hands the application a link or a file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use cargo_metadata::{Metadata, Package, PackageId};
use serde::Deserialize;

use crate::build::error::{BuildError, BuildResult};
pub(crate) use opening::{AssociatedDomain, FileType, UrlScheme};

pub(crate) mod capability;
pub(crate) mod opening;
pub(crate) mod verify;

/// The newest `schema` this CLI reads.
pub(crate) const SCHEMA: u32 = 3;

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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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
    /// The application's own URL schemes (schema 3).
    #[serde(default)]
    url_schemes: Vec<UrlScheme>,
    /// The web domains whose links open the application (schema 3).
    #[serde(default)]
    associated_domains: Vec<AssociatedDomain>,
    /// The file types the application opens (schema 3).
    #[serde(default)]
    file_types: Vec<FileType>,
    /// What each feature of the crate adds when it is enabled.
    #[serde(default)]
    features: BTreeMap<String, Part>,
}

/// The declarations of one crate, or of one of its features.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Part {
    #[serde(default)]
    android: AndroidPart,
    #[serde(default)]
    apple: ApplePart,
    #[serde(default)]
    url_schemes: Vec<UrlScheme>,
    #[serde(default)]
    associated_domains: Vec<AssociatedDomain>,
    #[serde(default)]
    file_types: Vec<FileType>,
}

/// What a crate's table, or one feature's part of it, says the application
/// opens.
#[derive(Clone, Copy)]
struct Opening<'a> {
    url_schemes: &'a [UrlScheme],
    associated_domains: &'a [AssociatedDomain],
    file_types: &'a [FileType],
}

impl<'a> Opening<'a> {
    fn of_declares(declares: &'a Declares) -> Self {
        Self {
            url_schemes: &declares.url_schemes,
            associated_domains: &declares.associated_domains,
            file_types: &declares.file_types,
        }
    }

    fn of_part(part: &'a Part) -> Self {
        Self {
            url_schemes: &part.url_schemes,
            associated_domains: &part.associated_domains,
            file_types: &part.file_types,
        }
    }

    fn is_empty(self) -> bool {
        self.url_schemes.is_empty()
            && self.associated_domains.is_empty()
            && self.file_types.is_empty()
    }
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
    /// The R8 rules that keep what native code reaches in those classes
    /// (schema 2), which a minified Gradle release applies.
    #[serde(default)]
    proguard: Option<CratePath>,
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
    /// Each R8 rules file, by crate and then path.
    android_proguard: BTreeSet<(String, PathBuf)>,
    apple_accessed_apis: BTreeMap<PrivacyCategory, BTreeMap<PrivacyReason, BTreeSet<String>>>,
    url_schemes: BTreeMap<UrlScheme, BTreeSet<String>>,
    associated_domains: BTreeMap<AssociatedDomain, BTreeSet<String>>,
    file_types: BTreeMap<FileType, BTreeSet<String>>,
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
        Self::collect_with_features(app_dir, targets, &[])
    }

    /// [`Declarations::collect`] for a build that also turns on `features`
    /// (each as `cargo --features` takes it, such as `flui/driver`): the
    /// parts those features enable count.
    ///
    /// # Errors
    ///
    /// As [`Declarations::collect`].
    pub(crate) fn collect_with_features(
        app_dir: &Path,
        targets: &[&str],
        features: &[String],
    ) -> BuildResult<Self> {
        let mut joined = Self::default();
        for target in targets {
            let mut options = vec!["--filter-platform".to_owned(), (*target).to_owned()];
            if !features.is_empty() {
                options.extend(["--features".to_owned(), features.join(",")]);
            }
            let metadata = cargo_metadata::MetadataCommand::new()
                .current_dir(app_dir)
                .other_options(options)
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
        self.add_opening(name, declares.schema, Opening::of_declares(&declares))?;
        for (feature, part) in &declares.features {
            if features.contains(&feature.as_str()) {
                self.add_android(package, declares.schema, &part.android)?;
                self.add_apple(name, &part.apple)?;
                self.add_opening(name, declares.schema, Opening::of_part(part))?;
            }
        }
        Ok(())
    }

    /// Add what `crate_name`'s table, which states `schema`, says the
    /// application opens. Two crates giving one extension two types is
    /// refused: the system can open a file by one type only.
    fn add_opening(
        &mut self,
        crate_name: &str,
        schema: u32,
        opening: Opening<'_>,
    ) -> BuildResult<()> {
        if opening.is_empty() {
            return Ok(());
        }
        let refused = |reason: String| {
            BuildError::invalid_config(
                format!("{crate_name}'s [package.metadata.flui.{KEY}]"),
                reason,
            )
        };
        if schema < 3 {
            return Err(refused(
                "declares what the application opens (`url-schemes`, `associated-domains`, \
                 `file-types`), which schema 3 introduced; state `schema = 3`"
                    .to_owned(),
            ));
        }
        for file_type in opening.file_types {
            if let Some((other, crates)) = self.file_types.iter().find(|(other, _)| {
                other.extension() == file_type.extension() && *other != file_type
            }) {
                return Err(refused(format!(
                    "declares `.{}` as {}, and {} declares it as {}",
                    file_type.extension(),
                    file_type.mime(),
                    crates.iter().cloned().collect::<Vec<_>>().join(", "),
                    other.mime()
                )));
            }
            self.file_types
                .entry(file_type.clone())
                .or_default()
                .insert(crate_name.to_owned());
        }
        for scheme in opening.url_schemes {
            self.url_schemes
                .entry(scheme.clone())
                .or_default()
                .insert(crate_name.to_owned());
        }
        for domain in opening.associated_domains {
            self.associated_domains
                .entry(domain.clone())
                .or_default()
                .insert(crate_name.to_owned());
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
            .map_or_default(|dir| dir.as_std_path().to_path_buf());
        let parts = [
            ("dex", &android.dex, false, &mut self.android_dex),
            ("java", &android.java, true, &mut self.android_java),
            (
                "proguard",
                &android.proguard,
                false,
                &mut self.android_proguard,
            ),
        ];
        for (key, path, is_dir, joined) in parts {
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
        self.android_proguard.extend(other.android_proguard);
        for (category, reasons) in other.apple_accessed_apis {
            let joined = self.apple_accessed_apis.entry(category).or_default();
            for (reason, crates) in reasons {
                joined.entry(reason).or_default().extend(crates);
            }
        }
        for (scheme, crates) in other.url_schemes {
            self.url_schemes.entry(scheme).or_default().extend(crates);
        }
        for (domain, crates) in other.associated_domains {
            self.associated_domains
                .entry(domain)
                .or_default()
                .extend(crates);
        }
        for (file_type, crates) in other.file_types {
            self.file_types.entry(file_type).or_default().extend(crates);
        }
    }

    /// What one crate's `[package.metadata.flui.declares]`, given as TOML,
    /// declares of the parts no crate directory resolves: what the
    /// application opens and its accessed APIs.
    #[cfg(test)]
    pub(crate) fn of_table(crate_name: &str, table: &str) -> Self {
        let declares: Declares =
            toml::from_str(table).expect("BUG: a test's declarations table parses");
        let mut declarations = Self::default();
        declarations
            .add_apple(crate_name, &declares.apple)
            .expect("BUG: a test's accessed APIs are well formed");
        declarations
            .add_opening(crate_name, declares.schema, Opening::of_declares(&declares))
            .expect("BUG: a test's opened types are well formed");
        declarations
    }

    /// Every URL scheme the application opens, in order, with the crates
    /// that declare it.
    pub(crate) fn url_schemes(&self) -> impl Iterator<Item = (&UrlScheme, &BTreeSet<String>)> {
        self.url_schemes.iter()
    }

    /// Every web domain whose links open the application, in order, with the
    /// crates that declare it.
    pub(crate) fn associated_domains(
        &self,
    ) -> impl Iterator<Item = (&AssociatedDomain, &BTreeSet<String>)> {
        self.associated_domains.iter()
    }

    /// Every file type the application opens, in order of extension, with
    /// the crates that declare it.
    pub(crate) fn file_types(&self) -> impl Iterator<Item = (&FileType, &BTreeSet<String>)> {
        self.file_types.iter()
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

    /// Every R8 rules file the linked crates ship, in order, with the crate
    /// that ships it.
    pub(crate) fn android_proguard(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.android_proguard
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

    /// A feature the build turns on beyond the defaults, as `flui run --agent`
    /// turns on `flui/driver`, brings in what it enables, named by the
    /// application's feature or by the dependency's.
    #[test]
    fn what_the_application_opens_is_joined_over_the_graph() {
        let project = fixture(&[
            (
                "app",
                &manifest(
                    "app",
                    "[workspace]\n[dependencies]\nviewer = { path = \"../viewer\", features = \
                     [\"notes\"] }\n\
                     [package.metadata.flui.declares]\nschema = 3\n\
                     url-schemes = [\"Notes\", \"notes-beta\"]\n\
                     associated-domains = [\"notes.example.com\"]\n",
                ),
            ),
            (
                "viewer",
                &manifest(
                    "viewer",
                    "[features]\nnotes = []\nsketch = []\n\
                     [package.metadata.flui.declares]\nschema = 3\nurl-schemes = [\"notes\"]\n\
                     [[package.metadata.flui.declares.features.notes.file-types]]\n\
                     extension = \"note\"\nmime = \"application/x-notes-note\"\nname = \"Note\"\n\
                     [[package.metadata.flui.declares.features.sketch.file-types]]\n\
                     extension = \"sketch\"\nmime = \"image/x-sketch\"\nname = \"Sketch\"\n",
                ),
            ),
        ]);
        let declarations =
            collect(&project.path().join("app"), "aarch64-linux-android").expect("declarations");
        let schemes: Vec<(String, Vec<String>)> = declarations
            .url_schemes()
            .map(|(scheme, crates)| (scheme.to_string(), crates.iter().cloned().collect()))
            .collect();
        assert_eq!(
            schemes,
            [
                (
                    "notes".to_owned(),
                    vec!["app".to_owned(), "viewer".to_owned()]
                ),
                ("notes-beta".to_owned(), vec!["app".to_owned()]),
            ],
            "a scheme compares without regard to case"
        );
        let domains: Vec<String> = declarations
            .associated_domains()
            .map(|(domain, _)| domain.to_string())
            .collect();
        assert_eq!(domains, ["notes.example.com"]);
        let file_types: Vec<(&str, &str)> = declarations
            .file_types()
            .map(|(file_type, _)| (file_type.extension(), file_type.mime()))
            .collect();
        assert_eq!(
            file_types,
            [("note", "application/x-notes-note")],
            "only the enabled feature's file type"
        );
    }

    #[test]
    fn the_features_a_build_turns_on_count() {
        let project = fixture(&[
            (
                "app",
                &manifest(
                    "app",
                    "[workspace]\n[features]\nextra = [\"lib/extra\"]\n\
                     [dependencies]\nlib = { path = \"../lib\" }\n",
                ),
            ),
            (
                "lib",
                &manifest(
                    "lib",
                    "[features]\nextra = []\n\
                     [package.metadata.flui.declares]\nschema = 1\n\
                     [package.metadata.flui.declares.features.extra.android]\n\
                     permissions = [\"android.permission.INTERNET\"]\n",
                ),
            ),
        ]);
        let app = project.path().join("app");
        let target = ["aarch64-linux-android"];
        let internet = vec![(
            "android.permission.INTERNET".to_owned(),
            vec!["lib".to_owned()],
        )];
        let defaults = Declarations::collect(&app, &target).expect("declarations");
        assert_eq!(permissions(&defaults), []);
        for features in [["extra".to_owned()], ["lib/extra".to_owned()]] {
            let on = Declarations::collect_with_features(&app, &target, &features)
                .expect("declarations");
            assert_eq!(permissions(&on), internet, "{features:?}");
        }
    }

    /// Each crate's dex, Java sources and R8 rules reach the build resolved
    /// under that crate's own directory, in crate order (the order of
    /// `classes.dex`, `classes2.dex`, …); a feature's dex counts only when
    /// the graph enables the feature.
    #[test]
    fn a_crates_android_code_resolves_under_the_crate() {
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
                     dex = \"android/classes.dex\"\njava = \"android/java\"\n\
                     proguard = \"android/proguard-rules.pro\"\n",
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
            ("zeta/android/proguard-rules.pro", false),
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
        assert_eq!(
            relative(android.android_proguard().collect()),
            [(
                "zeta".to_owned(),
                Path::new("zeta/android/proguard-rules.pro").to_path_buf()
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
            ("schema = 4\n", "states schema 4"),
            (
                "schema = 2\nurl-schemes = [\"notes\"]\n",
                "which schema 3 introduced",
            ),
            (
                "schema = 3\nurl-schemes = [\"https\"]\n",
                "not the application's own scheme",
            ),
            (
                "schema = 3\nassociated-domains = [\"https://example.com\"]\n",
                "is not a domain",
            ),
            (
                "schema = 3\nurl_schemes = [\"notes\"]\n",
                "unknown field `url_schemes`",
            ),
            (
                "schema = 3\n[[package.metadata.flui.declares.file-types]]\nextension = \"note\"\n\
                 mime = \"text/plain\"\n\
                 name = \"Note\"\n[[package.metadata.flui.declares.file-types]]\n\
                 extension = \"note\"\nmime = \"text/x-note\"\n\
                 name = \"Note\"\n",
                "declares `.note` as text/x-note, and lib declares it as text/plain",
            ),
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
