//! What an Apple bundle's `Info.plist` states about what the application
//! opens (ADR-0307 §4): its URL schemes (`CFBundleURLTypes`) and its file
//! types, each exported as a uniform type the bundle owns
//! (`UTExportedTypeDeclarations`) and opened as a document of that type
//! (`CFBundleDocumentTypes`). One source for the iOS and the macOS bundle.
//!
//! The associated domains are not here: they are an entitlement
//! (`com.apple.developer.associated-domains`), which a signed build carries.

use serde_json::{Map, Value, json};

#[cfg(any(target_os = "macos", test))]
use crate::build::util::xml::xml_escape;
use crate::declarations::Declarations;

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
    let mut documents = Vec::new();
    for (file_type, _) in declarations.file_types() {
        let uniform = format!("{identifier}.{}", file_type.extension());
        exported.push(json!({
            "UTTypeIdentifier": uniform,
            "UTTypeDescription": file_type.name(),
            "UTTypeConformsTo": ["public.data", "public.content"],
            "UTTypeTagSpecification": {
                "public.filename-extension": [file_type.extension()],
                "public.mime-type": [file_type.mime()],
            },
        }));
        documents.push(json!({
            "CFBundleTypeName": file_type.name(),
            "CFBundleTypeRole": "Viewer",
            "LSHandlerRank": "Owner",
            "LSItemContentTypes": [uniform],
        }));
    }
    if !documents.is_empty() {
        keys.insert(
            "UTExportedTypeDeclarations".to_owned(),
            Value::Array(exported),
        );
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
    use serde_json::json;

    use super::{ApplePlatform, info_plist_keys, plist_xml_entries};
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
}
