//! Values interpolated into the XML files the CLI writes (`Info.plist`,
//! `AndroidManifest.xml`).

/// Whether `text` is a name an XML file can carry as written: no control
/// character, and only characters XML 1.0 allows (`U+FFFE` and `U+FFFF`
/// are not, and no escape can stand for them). Every name a generated XML
/// file takes from the project (a display name, a bundle name, a file
/// type's name) is checked with this before it is accepted.
#[must_use]
pub(crate) fn is_xml_name_text(text: &str) -> bool {
    text.chars()
        .all(|c| !c.is_control() && !matches!(c, '\u{FFFE}' | '\u{FFFF}'))
}

/// Escape the five XML metacharacters in a text or attribute value.
///
/// An app name like `R&D` interpolated raw (`<string>R&D</string>`,
/// `android:label="R&D"`) makes the file malformed, and Launch Services or
/// `aapt2` can reject or misread it. Escaping the interpolated values keeps
/// any human-readable name valid; the `&` must be escaped first so the
/// entities this function introduces are not escaped a second time.
pub(crate) fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{is_xml_name_text, xml_escape};

    /// A name holding a character no XML file can carry is refused before
    /// any writer escapes it; metacharacters and other scripts are names.
    #[test]
    fn a_name_an_xml_file_cannot_carry_is_refused() {
        for (text, accepted) in [
            ("R&D <Notes> \"β\"", true),
            ("ノート", true),
            ("a\u{FFFE}", false),
            ("a\u{FFFF}", false),
            ("a\u{1}", false),
            ("a\tb", false),
            ("a\u{85}", false),
        ] {
            assert_eq!(is_xml_name_text(text), accepted, "{text:?}");
        }
    }

    #[test]
    fn metacharacters_are_escaped_so_the_plist_stays_well_formed() {
        assert_eq!(xml_escape("R&D"), "R&amp;D");
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
    }
}
