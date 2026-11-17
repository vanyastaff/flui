//! What an application opens (ADR-0307 §2, schema 3): its own URL schemes,
//! the web domains it is associated with and the file types it opens. Each
//! value is checked here, so the manifests it lands in never carry one that
//! breaks out of its attribute or that the operating system would refuse.

use std::fmt::{self, Display, Formatter};

use serde::Deserialize;

/// A URL scheme the application opens, such as `myapp`: RFC 3986's
/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`, kept in lower case, since
/// schemes compare without regard to case. A web scheme is not one: a web
/// URL is admitted by its domain (`associated-domains`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct UrlScheme(String);

/// Schemes the operating system keeps for itself or that name no application
/// of their own.
const RESERVED_SCHEMES: [&str; 7] = [
    "http",
    "https",
    "file",
    "content",
    "data",
    "javascript",
    "about",
];

impl UrlScheme {
    /// Parse `scheme`.
    ///
    /// # Errors
    ///
    /// Why `scheme` is not a scheme the application can own.
    pub(crate) fn try_new(scheme: impl Into<String>) -> Result<Self, String> {
        let scheme = scheme.into().to_ascii_lowercase();
        let well_formed = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !well_formed {
            return Err(format!(
                "`{scheme}` is not a URL scheme: a letter, then letters, digits, `+`, `-` or `.`"
            ));
        }
        if scheme.len() == 1 {
            return Err(format!(
                "`{scheme}` is one letter, which Windows reads as a drive (`{scheme}:\\…`) in the \
                 arguments a link launches with: a scheme has two characters or more"
            ));
        }
        if RESERVED_SCHEMES.contains(&scheme.as_str()) {
            return Err(format!(
                "`{scheme}` is not the application's own scheme; a web link is declared by its \
                 domain in `associated-domains`"
            ));
        }
        Ok(Self(scheme))
    }

    /// The scheme, as the manifests state it.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for UrlScheme {
    type Error = String;

    fn try_from(scheme: String) -> Result<Self, String> {
        Self::try_new(scheme)
    }
}

impl Display for UrlScheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A web domain whose links open the application, such as `example.com`:
/// two or more labels of ASCII letters, digits and `-` (none starting or
/// ending with `-`, each at most 63 bytes, the whole at most 253), kept in
/// lower case. A domain in another script is given in its `xn--` form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct AssociatedDomain(String);

impl AssociatedDomain {
    /// Parse `domain`.
    ///
    /// # Errors
    ///
    /// Why `domain` is not a host name.
    pub(crate) fn try_new(domain: impl Into<String>) -> Result<Self, String> {
        let domain = domain.into().to_ascii_lowercase();
        let well_formed = domain.len() <= 253
            && domain.split('.').count() >= 2
            && domain.split('.').all(|label| {
                (1..=63).contains(&label.len())
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        if well_formed {
            Ok(Self(domain))
        } else {
            Err(format!(
                "`{domain}` is not a domain such as `example.com`: two or more dot-separated \
                 labels of ASCII letters, digits and `-`, without a scheme, a path or `*`"
            ))
        }
    }
}

impl TryFrom<String> for AssociatedDomain {
    type Error = String;

    fn try_from(domain: String) -> Result<Self, String> {
        Self::try_new(domain)
    }
}

impl Display for AssociatedDomain {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A file type the application opens: its extension, its MIME type and the
/// name the system shows for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileType {
    extension: FileExtension,
    mime: MimeType,
    name: TypeName,
}

impl FileType {
    /// The extension, without its `.`.
    #[must_use]
    pub(crate) fn extension(&self) -> &str {
        &self.extension.0
    }

    /// The MIME type, such as `application/x-myapp-note`.
    #[must_use]
    pub(crate) fn mime(&self) -> &str {
        &self.mime.0
    }

    /// The name the system shows for the type.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name.0
    }

    /// The identifier Apple's systems declare for this extension themselves
    /// (`public.png` for `png`), when they do: a bundle opens such a file by
    /// that type and declares none of its own. `None` for an extension only
    /// the application gives a type, which its bundle exports.
    #[must_use]
    pub(crate) fn apple_system_type(&self) -> Option<&'static str> {
        APPLE_SYSTEM_TYPES
            .iter()
            .find(|(_, extensions)| extensions.contains(&self.extension()))
            .map(|(identifier, _)| *identifier)
    }
}

/// The extensions of the type Apple's systems declare as `identifier`, as a
/// bundle names it in `LSItemContentTypes`; none for any other identifier.
#[must_use]
pub(crate) fn apple_system_type_extensions(identifier: &str) -> &'static [&'static str] {
    APPLE_SYSTEM_TYPES
        .iter()
        .find(|(declared, _)| *declared == identifier)
        .map_or(&[], |(_, extensions)| extensions)
}

/// Types macOS and iOS declare themselves (the `UTType` constants of the
/// Uniform Type Identifiers framework), with the file extensions they claim.
/// A bundle that exported one of these under its own identifier would claim
/// to own every PNG or PDF on the machine.
const APPLE_SYSTEM_TYPES: [(&str, &[&str]); 25] = [
    ("public.png", &["png"]),
    ("public.jpeg", &["jpg", "jpeg"]),
    ("com.compuserve.gif", &["gif"]),
    ("public.tiff", &["tif", "tiff"]),
    ("public.heic", &["heic"]),
    ("com.microsoft.bmp", &["bmp"]),
    ("org.webmproject.webp", &["webp"]),
    ("com.microsoft.ico", &["ico"]),
    ("public.svg-image", &["svg"]),
    ("com.adobe.pdf", &["pdf"]),
    ("public.plain-text", &["txt"]),
    ("public.json", &["json"]),
    ("public.xml", &["xml"]),
    ("public.yaml", &["yaml", "yml"]),
    ("public.html", &["html", "htm"]),
    ("public.comma-separated-values-text", &["csv"]),
    ("public.rtf", &["rtf"]),
    ("public.zip-archive", &["zip"]),
    ("org.gnu.gnu-zip-archive", &["gz"]),
    ("public.tar-archive", &["tar"]),
    ("public.mp3", &["mp3"]),
    ("com.microsoft.waveform-audio", &["wav"]),
    ("com.apple.m4a-audio", &["m4a"]),
    ("public.mpeg-4", &["mp4"]),
    ("com.apple.quicktime-movie", &["mov"]),
];

/// A file extension without its `.`: 1 to 32 ASCII letters and digits, kept
/// in lower case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
struct FileExtension(String);

impl TryFrom<String> for FileExtension {
    type Error = String;

    fn try_from(extension: String) -> Result<Self, String> {
        let extension = extension.to_ascii_lowercase();
        if (1..=32).contains(&extension.len())
            && extension.chars().all(|c| c.is_ascii_alphanumeric())
        {
            Ok(Self(extension))
        } else {
            Err(format!(
                "`{extension}` is not a file extension: 1 to 32 ASCII letters and digits, \
                 without the `.`"
            ))
        }
    }
}

/// A MIME type, `type/subtype`, each part an RFC 6838 restricted name: a
/// letter or digit, then letters, digits and `!#$&-^_.+`, at most 127 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
struct MimeType(String);

impl TryFrom<String> for MimeType {
    type Error = String;

    fn try_from(mime: String) -> Result<Self, String> {
        let mime = mime.to_ascii_lowercase();
        let restricted = |part: &str| {
            (1..=127).contains(&part.len())
                && part.starts_with(|c: char| c.is_ascii_alphanumeric())
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
        };
        match mime.split_once('/') {
            Some((kind, subtype)) if restricted(kind) && restricted(subtype) => Ok(Self(mime)),
            _ => Err(format!(
                "`{mime}` is not a MIME type such as `application/x-myapp-note`"
            )),
        }
    }
}

/// The name a file type is shown by: 1 to 64 characters, none a control
/// character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
struct TypeName(String);

impl TryFrom<String> for TypeName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        if (1..=64).contains(&name.chars().count()) && !name.chars().any(char::is_control) {
            Ok(Self(name))
        } else {
            Err(format!(
                "`{}` is not a file type's name: 1 to 64 characters, no control character",
                name.escape_debug()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AssociatedDomain, FileType, UrlScheme};

    #[test]
    fn what_an_application_opens_is_checked_before_a_manifest_carries_it() {
        for (scheme, accepted) in [
            ("myapp", true),
            ("MyApp", true),
            ("com.example.notes+v2", true),
            ("", false),
            ("2fa", false),
            ("my app", false),
            ("my\"app", false),
            ("https", false),
            ("HTTP", false),
            ("file", false),
            ("x", false),
            ("xy", true),
        ] {
            assert_eq!(UrlScheme::try_new(scheme).is_ok(), accepted, "{scheme:?}");
        }
        assert_eq!(
            UrlScheme::try_new("MyApp").expect("a scheme").as_str(),
            "myapp"
        );
        for (domain, accepted) in [
            ("example.com", true),
            ("Notes.Example.COM", true),
            ("xn--bcher-kva.example", true),
            ("localhost", false),
            ("*.example.com", false),
            ("https://example.com", false),
            ("example.com/notes", false),
            ("-example.com", false),
            ("example..com", false),
            ("example.com.", false),
        ] {
            assert_eq!(
                AssociatedDomain::try_new(domain).is_ok(),
                accepted,
                "{domain:?}"
            );
        }
        let long_label = format!("{}.com", "a".repeat(64));
        assert!(AssociatedDomain::try_new(long_label).is_err());
        let parse = |toml: &str| toml::from_str::<FileType>(toml).map_err(|e| e.to_string());
        let note =
            parse("extension = \"NOTE\"\nmime = \"application/x-myapp-note\"\nname = \"Note\"")
                .expect("a file type");
        assert_eq!(
            (note.extension(), note.mime(), note.name()),
            ("note", "application/x-myapp-note", "Note")
        );
        for (toml, refusal) in [
            (
                "extension = \".note\"\nmime = \"text/plain\"\nname = \"Note\"",
                "not a file extension",
            ),
            (
                "extension = \"note\"\nmime = \"text\"\nname = \"Note\"",
                "not a MIME type",
            ),
            (
                "extension = \"note\"\nmime = \"text/pl\\\"ain\"\nname = \"Note\"",
                "not a MIME type",
            ),
            (
                "extension = \"note\"\nmime = \"text/plain\"\nname = \"\"",
                "not a file type's name",
            ),
            (
                "extension = \"note\"\nmime = \"text/plain\"\nname = \"a\\nb\"",
                "not a file type's name",
            ),
            (
                "extension = \"note\"\nmime = \"text/plain\"\nname = \"Note\"\nicon = \"x\"",
                "unknown field `icon`",
            ),
        ] {
            match parse(toml) {
                Err(error) if error.contains(refusal) => {}
                other => panic!("{toml:?}: {other:?}"),
            }
        }
    }
}
