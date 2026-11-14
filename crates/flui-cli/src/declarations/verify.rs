//! A manifest FLUI does not write, read back and compared with what the
//! linked crates declare: ADR-0307's verify mode, run by `flui doctor`.
//!
//! Three kinds are read, told apart by their root element and keys: an
//! Android `AndroidManifest.xml` (its `uses-permission` entries and the
//! intent filters of what the application opens), an Apple
//! `PrivacyInfo.xcprivacy` (its `NSPrivacyAccessedAPITypes`) and an Apple
//! `Info.plist` (its URL schemes and document types), each property list in
//! XML form. Each declared entry the file lacks is a [`Missing`], carrying
//! the line to add and the crates that need it.

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

/// What a manifest is, by its root element and keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestKind {
    /// An `AndroidManifest.xml`.
    Android,
    /// An Apple `PrivacyInfo.xcprivacy`.
    ApplePrivacy,
    /// An Apple bundle's `Info.plist`: a property list naming a
    /// `CFBundleIdentifier`.
    AppleInfo,
}

impl ManifestKind {
    /// The targets whose graphs the manifest has to cover.
    #[must_use]
    pub(crate) fn targets(self) -> &'static [&'static str] {
        match self {
            Self::Android => &ANDROID_TARGETS,
            Self::ApplePrivacy | Self::AppleInfo => &APPLE_TARGETS,
        }
    }
}

/// What a manifest says the application opens.
#[derive(Debug, Default)]
struct Opens {
    /// URL schemes, in lower case.
    url_schemes: BTreeSet<String>,
    /// Web domains whose `https` links are verified to open it, in lower
    /// case.
    associated_domains: BTreeSet<String>,
    /// For Android, the MIME types it opens; for Apple, the extensions its
    /// document types open, in lower case.
    file_types: BTreeSet<String>,
}

/// What a manifest declares, of the entries crates can declare.
#[derive(Debug)]
pub(crate) struct Manifest {
    kind: ManifestKind,
    android_permissions: BTreeSet<String>,
    accessed_apis: BTreeMap<String, BTreeSet<String>>,
    opens: Opens,
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
    /// Read `bytes`: an `AndroidManifest.xml`, or an XML
    /// `PrivacyInfo.xcprivacy` or `Info.plist`.
    ///
    /// # Errors
    ///
    /// Why `bytes` is none of them: a binary property list, text that is not
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
            "plist" => Self::apple(root),
            other => Err(format!(
                "has root element <{other}>, neither an AndroidManifest.xml <manifest> nor a \
                 PrivacyInfo.xcprivacy or Info.plist <plist>"
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
            opens: android_opens(root),
        }
    }

    fn apple(root: roxmltree::Node<'_, '_>) -> Result<Self, String> {
        let top = elements(root)
            .find(|node| node.has_tag_name("dict"))
            .ok_or_else(|| "is a property list without a top-level <dict>".to_owned())?;
        if dict_value(top, "CFBundleIdentifier")?.is_some() {
            return Ok(Self {
                kind: ManifestKind::AppleInfo,
                android_permissions: BTreeSet::new(),
                accessed_apis: BTreeMap::new(),
                opens: apple_info_opens(top)?,
            });
        }
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
            opens: Opens::default(),
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
            ManifestKind::Android => {
                let mut missing: Vec<Missing> = declarations
                    .android_permissions()
                    .filter(|(permission, _)| {
                        !self.android_permissions.contains(permission.as_str())
                    })
                    .map(|(permission, crates)| Missing {
                        entry: permission.as_str().to_owned(),
                        line: format!("<uses-permission android:name=\"{permission}\" />"),
                        crates: crates.iter().cloned().collect(),
                    })
                    .collect();
                missing.extend(self.android_opens_missing(declarations));
                missing
            }
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
            ManifestKind::AppleInfo => self.apple_info_missing(declarations),
        }
    }

    /// The intent filters an Android manifest lacks for what the
    /// application opens.
    fn android_opens_missing(&self, declarations: &Declarations) -> Vec<Missing> {
        let crates = |crates: &BTreeSet<String>| crates.iter().cloned().collect::<Vec<String>>();
        let mut missing = Vec::new();
        for (scheme, declared) in declarations.url_schemes() {
            if !self.opens.url_schemes.contains(scheme.as_str()) {
                missing.push(Missing {
                    entry: format!("URL scheme {scheme}"),
                    line: format!(
                        "an <intent-filter> with the VIEW action, the DEFAULT and BROWSABLE \
                         categories and <data android:scheme=\"{scheme}\" />"
                    ),
                    crates: crates(declared),
                });
            }
        }
        for (domain, declared) in declarations.associated_domains() {
            let domain = domain.to_string();
            if !self.opens.associated_domains.contains(&domain) {
                missing.push(Missing {
                    line: format!(
                        "an <intent-filter android:autoVerify=\"true\"> with the VIEW action, the \
                         DEFAULT and BROWSABLE categories, <data android:scheme=\"https\" /> and \
                         <data android:host=\"{domain}\" />"
                    ),
                    entry: format!("associated domain {domain}"),
                    crates: crates(declared),
                });
            }
        }
        for (file_type, declared) in declarations.file_types() {
            if !self.opens.file_types.contains(file_type.mime()) {
                missing.push(Missing {
                    entry: format!(
                        "file type .{} ({})",
                        file_type.extension(),
                        file_type.mime()
                    ),
                    line: format!(
                        "an <intent-filter> with the VIEW action, the DEFAULT category and \
                         <data android:mimeType=\"{}\" />",
                        file_type.mime()
                    ),
                    crates: crates(declared),
                });
            }
        }
        missing
    }

    /// The keys an `Info.plist` lacks for what the application opens. Its
    /// associated domains are an entitlement, which the property list does
    /// not carry.
    fn apple_info_missing(&self, declarations: &Declarations) -> Vec<Missing> {
        let crates = |crates: &BTreeSet<String>| crates.iter().cloned().collect::<Vec<String>>();
        let mut missing = Vec::new();
        for (scheme, declared) in declarations.url_schemes() {
            if !self.opens.url_schemes.contains(scheme.as_str()) {
                missing.push(Missing {
                    entry: format!("URL scheme {scheme}"),
                    line: format!("{scheme} in a CFBundleURLTypes entry's CFBundleURLSchemes"),
                    crates: crates(declared),
                });
            }
        }
        for (file_type, declared) in declarations.file_types() {
            let extension = file_type.extension().to_ascii_lowercase();
            if !self.opens.file_types.contains(&extension) {
                missing.push(Missing {
                    entry: format!(
                        "file type .{} ({})",
                        file_type.extension(),
                        file_type.mime()
                    ),
                    line: format!(
                        "a UTExportedTypeDeclarations or UTImportedTypeDeclarations type whose \
                         public.filename-extension is {}, listed in a CFBundleDocumentTypes \
                         entry's LSItemContentTypes",
                        file_type.extension()
                    ),
                    crates: crates(declared),
                });
            }
        }
        missing
    }
}

/// What the intent filters of an Android manifest's components open: a
/// filter with the `VIEW` action opens its MIME types, with the `BROWSABLE`
/// category too its schemes, and verified (`android:autoVerify="true"`) its
/// `https` hosts. A filter's `<data>` elements combine, as Android reads
/// them.
fn android_opens(root: roxmltree::Node<'_, '_>) -> Opens {
    const VIEW: &str = "android.intent.action.VIEW";
    const BROWSABLE: &str = "android.intent.category.BROWSABLE";
    let mut opens = Opens::default();
    for filter in root
        .descendants()
        .filter(|node| node.has_tag_name("intent-filter"))
        // A filter the merger removes is not in the merged manifest.
        .filter(|node| node.attribute((TOOLS_NS, "node")) != Some("remove"))
    {
        let named = |tag: &str, name: &str| {
            elements(filter).any(|node| {
                node.has_tag_name(tag) && node.attribute((ANDROID_NS, "name")) == Some(name)
            })
        };
        if !named("action", VIEW) {
            continue;
        }
        let data: Vec<roxmltree::Node<'_, '_>> = elements(filter)
            .filter(|node| node.has_tag_name("data"))
            .collect();
        let values = |attribute: &str| -> Vec<String> {
            data.iter()
                .filter_map(|node| node.attribute((ANDROID_NS, attribute)))
                .map(str::to_ascii_lowercase)
                .collect()
        };
        let schemes = values("scheme");
        opens.file_types.extend(
            data.iter()
                .filter_map(|node| node.attribute((ANDROID_NS, "mimeType")))
                .map(str::to_owned),
        );
        if !named("category", BROWSABLE) {
            continue;
        }
        if filter.attribute((ANDROID_NS, "autoVerify")) == Some("true")
            && schemes.iter().any(|scheme| scheme == "https")
        {
            opens.associated_domains.extend(values("host"));
        }
        opens.url_schemes.extend(schemes);
    }
    opens
}

/// What an `Info.plist`'s top-level `<dict>` opens: the schemes of its
/// `CFBundleURLTypes`, and the extensions of the types its
/// `CFBundleDocumentTypes` list in `LSItemContentTypes`, declared in its
/// `UTExportedTypeDeclarations` or `UTImportedTypeDeclarations`, or named in
/// a document type's `CFBundleTypeExtensions`.
fn apple_info_opens(top: roxmltree::Node<'_, '_>) -> Result<Opens, String> {
    let mut opens = Opens::default();
    for url_type in array_dicts(top, "CFBundleURLTypes")? {
        opens.url_schemes.extend(
            strings(url_type, "CFBundleURLSchemes")?
                .iter()
                .map(|scheme| scheme.to_ascii_lowercase()),
        );
    }
    let mut extensions_by_type: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in ["UTExportedTypeDeclarations", "UTImportedTypeDeclarations"] {
        for declaration in array_dicts(top, key)? {
            let Some(identifier) = dict_value(declaration, "UTTypeIdentifier")?
                .filter(|value| value.has_tag_name("string"))
                .map(text)
            else {
                continue;
            };
            let extensions = match dict_value(declaration, "UTTypeTagSpecification")? {
                Some(tags) if tags.has_tag_name("dict") => {
                    strings(tags, "public.filename-extension")?
                }
                _ => Vec::new(),
            };
            extensions_by_type
                .entry(identifier.to_owned())
                .or_default()
                .extend(extensions);
        }
    }
    for document in array_dicts(top, "CFBundleDocumentTypes")? {
        for content_type in strings(document, "LSItemContentTypes")? {
            if let Some(extensions) = extensions_by_type.get(&content_type) {
                opens.file_types.extend(
                    extensions
                        .iter()
                        .map(|extension| extension.to_ascii_lowercase()),
                );
            }
        }
        opens.file_types.extend(
            strings(document, "CFBundleTypeExtensions")?
                .iter()
                .map(|extension| extension.to_ascii_lowercase()),
        );
    }
    Ok(opens)
}

/// The `<dict>` entries of the `<array>` under `key` in `dict`, none when it
/// has no such key.
fn array_dicts<'a, 'input>(
    dict: roxmltree::Node<'a, 'input>,
    key: &str,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, String> {
    match dict_value(dict, key)? {
        None => Ok(Vec::new()),
        Some(array) if array.has_tag_name("array") => elements(array)
            .map(|entry| {
                if entry.has_tag_name("dict") {
                    Ok(entry)
                } else {
                    Err(format!("lists a {key} entry that is not a <dict>"))
                }
            })
            .collect(),
        Some(_) => Err(format!("states {key}, but not as an <array>")),
    }
}

/// The strings under `key` in `dict`: an `<array>` of `<string>`s, or one
/// `<string>`, as a property list may give either.
fn strings(dict: roxmltree::Node<'_, '_>, key: &str) -> Result<Vec<String>, String> {
    match dict_value(dict, key)? {
        None => Ok(Vec::new()),
        Some(value) if value.has_tag_name("string") => Ok(vec![text(value).to_owned()]),
        Some(array) if array.has_tag_name("array") => elements(array)
            .map(|entry| {
                if entry.has_tag_name("string") {
                    Ok(text(entry).to_owned())
                } else {
                    Err(format!("lists a {key} entry that is not a <string>"))
                }
            })
            .collect(),
        Some(_) => Err(format!("states {key}, but not as an <array> of strings")),
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
