//! Values interpolated into the XML files the CLI writes (`Info.plist`,
//! `AndroidManifest.xml`).

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
    use super::xml_escape;

    #[test]
    fn metacharacters_are_escaped_so_the_plist_stays_well_formed() {
        assert_eq!(xml_escape("R&D"), "R&amp;D");
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
    }
}
