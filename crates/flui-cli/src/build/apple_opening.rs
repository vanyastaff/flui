//! What an Apple bundle's `Info.plist` states about what the application
//! opens (ADR-0307 §4): its URL schemes (`CFBundleURLTypes`) and its file
//! types, each opened as a document of its type (`CFBundleDocumentTypes`).
//! A type the system declares (`public.png`, `com.adobe.pdf`) is named by
//! its own identifier and opened as an alternate viewer; a type the
//! application crate declares is exported as a uniform type the bundle owns
//! (`UTExportedTypeDeclarations`) and opened as its owner; a type only a
//! linked crate declares is imported (`UTImportedTypeDeclarations`) and
//! opened as an alternate viewer, with a warning (ADR-0361). One source for
//! the iOS and the macOS bundle.
//!
//! The associated domains are not here: they are an entitlement
//! (`com.apple.developer.associated-domains`), which a signed build carries.

use serde_json::{Map, Value, json};

#[cfg(any(target_os = "macos", test))]
use crate::build::util::xml::xml_escape;
use crate::declarations::{Declarations, FileType};

/// Which bundle the keys are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplePlatform {
    Ios,
    #[cfg(any(target_os = "macos", test))]
    Macos,
}

/// The `Info.plist` keys for what `declarations` says the bundle
/// `identifier` opens; none when it declares nothing.
#[must_use]
pub(crate) fn info_plist_keys(
    identifier: &str,
    declarations: &Declarations,
    platform: ApplePlatform,
) -> Map<String, Value> {
    let mut keys = Map::new();
    let schemes: Vec<&str> = declarations
        .url_schemes()
        .map(|(scheme, _)| scheme.as_str())
        .collect();
    if !schemes.is_empty() {
        keys.insert(
            "CFBundleURLTypes".to_owned(),
            json!([{
                "CFBundleURLName": identifier,
                "CFBundleTypeRole": "Viewer",
                "CFBundleURLSchemes": schemes,
            }]),
        );
    }
    let mut exported = Vec::new();
    let mut imported = Vec::new();
    let mut documents = Vec::new();
    for (file_type, _) in declarations.file_types() {
        let (content_type, rank) = match type_kind(file_type, declarations) {
            // Opened by the system's identifier, as one viewer among others.
            TypeKind::System(system) => (system.to_owned(), "Alternate"),
            // Declared under the bundle's identifier: exported and owned when
            // the application defines it, imported as one viewer among
            // others when only a crate it links names it.
            kind => {
                let uniform = format!("{identifier}.{}", file_type.extension());
                let declaration = json!({
                    "UTTypeIdentifier": uniform,
                    "UTTypeDescription": file_type.name(),
                    "UTTypeConformsTo": ["public.data", "public.content"],
                    "UTTypeTagSpecification": {
                        "public.filename-extension": [file_type.extension()],
                        "public.mime-type": [file_type.mime()],
                    },
                });
                if kind == TypeKind::Application {
                    exported.push(declaration);
                    (uniform, "Owner")
                } else {
                    imported.push(declaration);
                    (uniform, "Alternate")
                }
            }
        };
        documents.push(json!({
            "CFBundleTypeName": file_type.name(),
            "CFBundleTypeRole": "Viewer",
            "LSHandlerRank": rank,
            "LSItemContentTypes": [content_type],
        }));
    }
    for (key, declarations) in [
        ("UTExportedTypeDeclarations", exported),
        ("UTImportedTypeDeclarations", imported),
    ] {
        if !declarations.is_empty() {
            keys.insert(key.to_owned(), Value::Array(declarations));
        }
    }
    if !documents.is_empty() {
        keys.insert("CFBundleDocumentTypes".to_owned(), Value::Array(documents));
        if platform == ApplePlatform::Ios {
            // The application gets its own copy of a file it is handed,
            // rather than editing the original where it lies.
            keys.insert(
                "LSSupportsOpeningDocumentsInPlace".to_owned(),
                Value::Bool(false),
            );
        }
    }
    keys
}

/// How a bundle states one file type it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    /// A type the system declares, opened by this identifier.
    System(&'static str),
    /// A type the application crate declares and the system does not: the
    /// application's own format.
    Application,
    /// A type only a crate the application links declares, which the system
    /// does not know: another party's format.
    Linked,
}

fn type_kind(file_type: &FileType, declarations: &Declarations) -> TypeKind {
    match file_type.apple_system_type() {
        Some(system) => TypeKind::System(system),
        None if declarations.is_application_type(file_type) => TypeKind::Application,
        None => TypeKind::Linked,
    }
}

/// The file types the bundle opens that neither the system nor the
/// application declares: a crate it links names them, so the bundle imports
/// each under its own identifier and does not own it. A packager warns about
/// each, since another application that exports the type may give it a
/// different identifier.
fn types_only_a_linked_crate_declares(
    declarations: &Declarations,
) -> impl Iterator<Item = (&FileType, &std::collections::BTreeSet<String>)> {
    declarations
        .file_types()
        .filter(|(file_type, _)| type_kind(file_type, declarations) == TypeKind::Linked)
}

/// Warn about each type [`types_only_a_linked_crate_declares`] names, as a
/// bundle is staged with it.
pub(crate) fn warn_about_types_only_a_linked_crate_declares(declarations: &Declarations) {
    for (file_type, crates) in types_only_a_linked_crate_declares(declarations) {
        let crates = crates.iter().cloned().collect::<Vec<_>>().join(", ");
        // A warning that cannot be printed changes nothing the build writes.
        let _ = crate::ui::warning(format!(
            "`.{}` ({}) is declared by {crates}, not by the application, and the system does \
             not declare it: the bundle opens it as an imported type and does not own it",
            file_type.extension(),
            file_type.mime()
        ));
    }
}

/// `keys` as a whole XML property list document, its top `<dict>` holding
/// them in key order.
#[must_use]
#[cfg(any(target_os = "macos", test))]
pub(crate) fn plist_xml_document(keys: &Map<String, Value>) -> String {
    let mut document = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    document.push_str(&plist_xml_entries(keys, 1));
    document.push_str("</dict>\n</plist>\n");
    document
}

/// `keys` as the entries of an XML property list's top `<dict>`, each line
/// indented by `depth` levels of four spaces.
#[must_use]
#[cfg(any(target_os = "macos", test))]
pub(crate) fn plist_xml_entries(keys: &Map<String, Value>, depth: usize) -> String {
    let mut out = String::new();
    for (key, value) in keys {
        indent(&mut out, depth);
        out.push_str("<key>");
        out.push_str(&xml_escape(key));
        out.push_str("</key>\n");
        plist_xml_value(&mut out, value, depth);
    }
    out
}

#[cfg(any(target_os = "macos", test))]
fn plist_xml_value(out: &mut String, value: &Value, depth: usize) {
    indent(out, depth);
    match value {
        Value::Bool(true) => out.push_str("<true/>\n"),
        Value::Bool(false) => out.push_str("<false/>\n"),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            out.push_str("<integer>");
            out.push_str(&number.to_string());
            out.push_str("</integer>\n");
        }
        Value::Number(number) => {
            out.push_str("<real>");
            out.push_str(&number.to_string());
            out.push_str("</real>\n");
        }
        Value::String(text) => {
            out.push_str("<string>");
            out.push_str(&xml_escape(text));
            out.push_str("</string>\n");
        }
        Value::Array(items) => {
            out.push_str("<array>\n");
            for item in items {
                plist_xml_value(out, item, depth + 1);
            }
            indent(out, depth);
            out.push_str("</array>\n");
        }
        Value::Object(entries) => {
            out.push_str("<dict>\n");
            out.push_str(&plist_xml_entries(entries, depth + 1));
            indent(out, depth);
            out.push_str("</dict>\n");
        }
        // A property list has no null; no key above produces one.
        Value::Null => out.push_str("<string></string>\n"),
    }
}

#[cfg(any(target_os = "macos", test))]
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{ApplePlatform, info_plist_keys, plist_xml_document, plist_xml_entries};
    use crate::declarations::Declarations;

    fn notes() -> Declarations {
        Declarations::of_table(
            "notes",
            "schema = 3\nurl-schemes = [\"notes\", \"Notes-Beta\"]\n\
             associated-domains = [\"notes.example.com\"]\n\
             [[file-types]]\nextension = \"note\"\nmime = \"application/x-notes-note\"\n\
             name = \"Note & <draft>\"\n",
        )
    }

    /// Both Apple bundles register the application's schemes and own its
    /// file types as types exported under the bundle's identifier; only iOS
    /// says how a handed file is opened. A bundle that declares nothing gets
    /// no key, and the domains are left to the signed entitlement.
    #[test]
    fn a_bundle_registers_what_the_application_opens() {
        let keys = info_plist_keys("com.example.notes", &notes(), ApplePlatform::Ios);
        assert_eq!(
            keys["CFBundleURLTypes"],
            json!([{
                "CFBundleURLName": "com.example.notes",
                "CFBundleTypeRole": "Viewer",
                "CFBundleURLSchemes": ["notes", "notes-beta"],
            }])
        );
        assert_eq!(
            keys["UTExportedTypeDeclarations"],
            json!([{
                "UTTypeIdentifier": "com.example.notes.note",
                "UTTypeDescription": "Note & <draft>",
                "UTTypeConformsTo": ["public.data", "public.content"],
                "UTTypeTagSpecification": {
                    "public.filename-extension": ["note"],
                    "public.mime-type": ["application/x-notes-note"],
                },
            }])
        );
        assert_eq!(
            keys["CFBundleDocumentTypes"],
            json!([{
                "CFBundleTypeName": "Note & <draft>",
                "CFBundleTypeRole": "Viewer",
                "LSHandlerRank": "Owner",
                "LSItemContentTypes": ["com.example.notes.note"],
            }])
        );
        assert_eq!(keys["LSSupportsOpeningDocumentsInPlace"], json!(false));
        assert!(
            !keys.keys().any(|key| key.contains("ssociated")),
            "{keys:?}"
        );
        let macos = info_plist_keys("com.example.notes", &notes(), ApplePlatform::Macos);
        assert!(!macos.contains_key("LSSupportsOpeningDocumentsInPlace"));
        assert!(
            info_plist_keys(
                "com.example.notes",
                &Declarations::default(),
                ApplePlatform::Ios
            )
            .is_empty()
        );
    }

    /// A file type the system declares (`png`, `jpg`, `docx`) is opened by
    /// the system's identifier as an alternate viewer and never exported: a
    /// bundle exporting `com.example.notes.png` would claim to own every PNG.
    /// The application crate's own type is exported and owned; a type only a
    /// linked crate declares, unknown to the system, is imported and opened
    /// as an alternate viewer, and is the one the packager warns about. The
    /// doctor reads all three back from the property list the bundle writes.
    #[test]
    fn a_type_the_system_declares_is_opened_and_not_owned() {
        use crate::declarations::verify::Manifest;

        let mut declarations = Declarations::of_table(
            "notes",
            "schema = 3\n\
             [[file-types]]\nextension = \"note\"\nmime = \"application/x-notes-note\"\n\
             name = \"Note\"\n\
             [[file-types]]\nextension = \"png\"\nmime = \"image/png\"\nname = \"Image\"\n\
             [[file-types]]\nextension = \"JPG\"\nmime = \"image/jpeg\"\nname = \"Photo\"\n\
             [[file-types]]\nextension = \"docx\"\n\
             mime = \"application/vnd.openxmlformats-officedocument.wordprocessingml.document\"\n\
             name = \"Word document\"\n",
        );
        declarations
            .join(Declarations::of_dependency_table(
                "vectors",
                "schema = 3\n[[file-types]]\nextension = \"vec\"\n\
                 mime = \"application/x-vectors\"\nname = \"Vectors\"\n",
            ))
            .expect("one type per extension");
        let identifiers = |keys: &serde_json::Map<String, Value>, key: &str| -> Vec<Value> {
            keys.get(key)
                .and_then(Value::as_array)
                .map_or_default(|declared| {
                    declared
                        .iter()
                        .map(|declaration| declaration["UTTypeIdentifier"].clone())
                        .collect()
                })
        };
        for platform in [ApplePlatform::Ios, ApplePlatform::Macos] {
            let keys = info_plist_keys("com.example.notes", &declarations, platform);
            assert_eq!(
                identifiers(&keys, "UTExportedTypeDeclarations"),
                [json!("com.example.notes.note")],
                "{platform:?}: only the application's own type is exported"
            );
            assert_eq!(
                identifiers(&keys, "UTImportedTypeDeclarations"),
                [json!("com.example.notes.vec")],
                "{platform:?}: a type only a linked crate declares is imported"
            );
            let opened: Vec<(&Value, &Value)> = keys["CFBundleDocumentTypes"]
                .as_array()
                .expect("document types")
                .iter()
                .map(|document| (&document["LSItemContentTypes"], &document["LSHandlerRank"]))
                .collect();
            assert_eq!(
                opened,
                [
                    (
                        &json!(["org.openxmlformats.wordprocessingml.document"]),
                        &json!("Alternate")
                    ),
                    (&json!(["public.jpeg"]), &json!("Alternate")),
                    (&json!(["com.example.notes.note"]), &json!("Owner")),
                    (&json!(["public.png"]), &json!("Alternate")),
                    (&json!(["com.example.notes.vec"]), &json!("Alternate")),
                ],
                "{platform:?}"
            );
            let linked: Vec<&str> = super::types_only_a_linked_crate_declares(&declarations)
                .map(|(file_type, _)| file_type.extension())
                .collect();
            assert_eq!(linked, ["vec"], "the type the packager warns about");
            let written = Manifest::parse(plist_xml_document(&keys).as_bytes())
                .expect("the plist reads back");
            assert_eq!(written.missing(&declarations), [], "{platform:?}");
        }
    }

    /// The XML form escapes every string and nests arrays and dictionaries,
    /// so the macOS bundle's hand-written plist takes the same keys.
    #[test]
    fn the_keys_render_as_xml_property_list_entries() {
        let keys = info_plist_keys("com.example.notes", &notes(), ApplePlatform::Ios);
        let entries = plist_xml_entries(&keys, 1);
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
             {entries}</dict>\n</plist>\n"
        );
        let parsed = roxmltree::Document::parse(&document).expect("the entries are XML");
        let texts: Vec<&str> = parsed
            .descendants()
            .filter(|node| node.has_tag_name("string"))
            .filter_map(|node| node.text())
            .collect();
        assert!(texts.contains(&"Note & <draft>"), "{texts:?}");
        assert!(entries.contains("    <key>CFBundleURLTypes</key>\n    <array>\n        <dict>\n"));
        assert!(
            entries.contains("    <key>LSSupportsOpeningDocumentsInPlace</key>\n    <false/>\n")
        );
    }

    /// `flui doctor` reads back what the bundle writes: an `Info.plist`
    /// carrying these keys lacks nothing the application opens, and one
    /// without them lacks each scheme and file type.
    #[test]
    fn the_doctor_finds_what_the_bundle_writes() {
        use crate::declarations::verify::{Manifest, ManifestKind};

        let plist = |entries: &str| {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n    \
                 <key>CFBundleIdentifier</key>\n    <string>com.example.notes</string>\n\
                 {entries}</dict>\n</plist>\n"
            )
        };
        let declarations = notes();
        for platform in [ApplePlatform::Ios, ApplePlatform::Macos] {
            let keys = info_plist_keys("com.example.notes", &declarations, platform);
            let written = Manifest::parse(plist(&plist_xml_entries(&keys, 1)).as_bytes())
                .expect("the plist reads back");
            assert_eq!(written.kind(), ManifestKind::AppleInfo);
            assert_eq!(written.missing(&declarations), [], "{platform:?}");
        }
        let bare = Manifest::parse(plist("").as_bytes()).expect("a bare plist reads");
        let entries: Vec<String> = bare
            .missing(&declarations)
            .into_iter()
            .map(|missing| missing.entry)
            .collect();
        assert_eq!(
            entries,
            [
                "URL scheme notes",
                "URL scheme notes-beta",
                "file type .note (application/x-notes-note)",
            ]
        );
    }
}
