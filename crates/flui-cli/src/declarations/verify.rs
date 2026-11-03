//! A manifest FLUI does not write, read back and compared with what the
//! linked crates declare: ADR-0307's verify mode, run by `flui doctor`.
//!
//! Two kinds are read, told apart by their root element: an Android
//! `AndroidManifest.xml` (its `uses-permission` entries) and an Apple
//! `PrivacyInfo.xcprivacy` in XML property-list form (its
//! `NSPrivacyAccessedAPITypes`). Each declared entry the file lacks is a
//! [`Missing`], carrying the line to add and the crates that need it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::Declarations;

/// The Android targets a manifest serves: every ABI, so that a permission any
/// of them links is listed.
const ANDROID_TARGETS: [&str; 4] = [
    "aarch64-linux-android",
    "armv7-linux-androideabi",
    "x86_64-linux-android",
    "i686-linux-android",
];

/// The Apple target whose graph a privacy manifest declares for.
const APPLE_TARGETS: [&str; 1] = ["aarch64-apple-ios"];

/// The `android:` attribute namespace.
const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";

/// The manifest merger's `tools:` attribute namespace.
const TOOLS_NS: &str = "http://schemas.android.com/tools";

/// What a manifest is, by its root element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestKind {
    /// An `AndroidManifest.xml`.
    Android,
    /// An Apple `PrivacyInfo.xcprivacy`.
    ApplePrivacy,
}

impl ManifestKind {
    /// The targets whose graphs the manifest has to cover.
    #[must_use]
    pub(crate) fn targets(self) -> &'static [&'static str] {
        match self {
            Self::Android => &ANDROID_TARGETS,
            Self::ApplePrivacy => &APPLE_TARGETS,
        }
    }
}

/// What a manifest declares, of the entries crates can declare.
#[derive(Debug)]
pub(crate) struct Manifest {
    kind: ManifestKind,
    android_permissions: BTreeSet<String>,
    accessed_apis: BTreeMap<String, BTreeSet<String>>,
}

/// A declared entry the manifest lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Missing {
    /// The entry, such as `android.permission.INTERNET` or
    /// `NSPrivacyAccessedAPICategoryFileTimestamp C617.1`.
    pub(crate) entry: String,
    /// What to add to the manifest for it.
    pub(crate) line: String,
    /// The crates that declare it.
    pub(crate) crates: Vec<String>,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.entry, self.crates.join(", "))
    }
}

impl Manifest {
    /// Read `bytes`, an `AndroidManifest.xml` or an XML `PrivacyInfo.xcprivacy`.
    ///
    /// # Errors
    ///
    /// Why `bytes` is neither: a binary property list, text that is not
    /// UTF-8 or not XML, another root element, a malformed entry.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.starts_with(b"bplist") {
            return Err(
                "is a binary property list; convert it with `plutil -convert xml1 <file>` first"
                    .to_owned(),
            );
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "is not UTF-8 text".to_owned())?;
        // A property list names its DTD; nothing in either kind defines
        // entities, so allowing the declaration expands nothing.
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        };
        let document = roxmltree::Document::parse_with_options(text, options)
            .map_err(|error| format!("is not XML: {error}"))?;
        let root = document.root_element();
        match root.tag_name().name() {
            "manifest" => Ok(Self::android(root)),
            "plist" => Self::apple_privacy(root),
            other => Err(format!(
                "has root element <{other}>, neither an AndroidManifest.xml <manifest> nor a \
                 PrivacyInfo.xcprivacy <plist>"
            )),
        }
    }

    fn android(root: roxmltree::Node<'_, '_>) -> Self {
        let android_permissions = root
            .children()
            .filter(|node| {
                matches!(
                    node.tag_name().name(),
                    "uses-permission" | "uses-permission-sdk-23"
                )
            })
            // `tools:node="remove"` takes the permission out of the merged
            // manifest: the file states it does not want it.
            .filter(|node| node.attribute((TOOLS_NS, "node")) != Some("remove"))
            .filter_map(|node| node.attribute((ANDROID_NS, "name")))
            .map(str::to_owned)
            .collect();
        Self {
            kind: ManifestKind::Android,
            android_permissions,
            accessed_apis: BTreeMap::new(),
        }
    }

    fn apple_privacy(root: roxmltree::Node<'_, '_>) -> Result<Self, String> {
        let top = elements(root)
            .find(|node| node.has_tag_name("dict"))
            .ok_or_else(|| "is a property list without a top-level <dict>".to_owned())?;
        let mut accessed_apis: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if let Some(types) = dict_value(top, "NSPrivacyAccessedAPITypes")? {
            if !types.has_tag_name("array") {
                return Err("states NSPrivacyAccessedAPITypes, but not as an <array>".to_owned());
            }
            for entry in elements(types) {
                if !entry.has_tag_name("dict") {
                    return Err("lists an accessed API that is not a <dict>".to_owned());
                }
                let category = dict_value(entry, "NSPrivacyAccessedAPIType")?
                    .filter(|value| value.has_tag_name("string"))
                    .and_then(|value| value.text())
                    .ok_or_else(|| {
                        "lists an accessed API without a NSPrivacyAccessedAPIType string".to_owned()
                    })?;
                let reasons = accessed_apis.entry(category.trim().to_owned()).or_default();
                let listed = dict_value(entry, "NSPrivacyAccessedAPITypeReasons")?
                    .filter(|value| value.has_tag_name("array"))
                    .ok_or_else(|| {
                        format!("lists {category} without a NSPrivacyAccessedAPITypeReasons array")
                    })?;
                for reason in elements(listed) {
                    let code = reason
                        .has_tag_name("string")
                        .then(|| reason.text())
                        .flatten()
                        .ok_or_else(|| {
                            format!("lists a reason of {category} that is not a string")
                        })?;
                    reasons.insert(code.trim().to_owned());
                }
            }
        }
        Ok(Self {
            kind: ManifestKind::ApplePrivacy,
            android_permissions: BTreeSet::new(),
            accessed_apis,
        })
    }

    /// What the manifest is.
    #[must_use]
    pub(crate) fn kind(&self) -> ManifestKind {
        self.kind
    }

    /// Each entry of `declarations` for this kind of manifest that it lacks,
    /// in order.
    #[must_use]
    pub(crate) fn missing(&self, declarations: &Declarations) -> Vec<Missing> {
        match self.kind {
            ManifestKind::Android => declarations
                .android_permissions()
                .filter(|(permission, _)| !self.android_permissions.contains(permission.as_str()))
                .map(|(permission, crates)| Missing {
                    entry: permission.as_str().to_owned(),
                    line: format!("<uses-permission android:name=\"{permission}\" />"),
                    crates: crates.iter().cloned().collect(),
                })
                .collect(),
            ManifestKind::ApplePrivacy => {
                let mut missing = Vec::new();
                for (category, reasons) in declarations.apple_accessed_apis() {
                    let present = self.accessed_apis.get(category.as_str());
                    for (reason, crates) in reasons {
                        if present.is_some_and(|present| present.contains(reason.as_str())) {
                            continue;
                        }
                        missing.push(Missing {
                            entry: format!("{} {}", category.as_str(), reason.as_str()),
                            line: format!(
                                "NSPrivacyAccessedAPITypes: {} with reason {}",
                                category.as_str(),
                                reason.as_str()
                            ),
                            crates: crates.iter().cloned().collect(),
                        });
                    }
                }
                missing
            }
        }
    }
}

/// The element children of `node`.
fn elements<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    node.children().filter(roxmltree::Node::is_element)
}

/// The value element under `key` in a property-list `<dict>`, whose children
/// alternate `<key>` and value.
fn dict_value<'a, 'input>(
    dict: roxmltree::Node<'a, 'input>,
    key: &str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, String> {
    let mut children = elements(dict);
    while let Some(node) = children.next() {
        if !node.has_tag_name("key") {
            return Err(format!(
                "has a <dict> entry <{}> where a <key> belongs",
                node.tag_name().name()
            ));
        }
        let value = children
            .next()
            .ok_or_else(|| format!("has a <dict> key `{}` without a value", text(node)))?;
        if text(node) == key {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn text<'a>(node: roxmltree::Node<'a, '_>) -> &'a str {
    node.text().map_or("", str::trim)
}
