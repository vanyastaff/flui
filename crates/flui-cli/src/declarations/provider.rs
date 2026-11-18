//! The Android content providers a crate declares (ADR-0307 §2, schema 4):
//! a class the crate ships and the authority it serves under, below the
//! application's package. Each value is checked here, so neither breaks out
//! of its manifest attribute.

use std::fmt::{self, Display, Formatter};

use serde::Deserialize;

/// A content provider a crate ships. The manifest declares it inside
/// `<application>`, always unexported and granting URI permissions, so it
/// serves only the URIs its crate grants per intent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AndroidProvider {
    /// The provider's class.
    pub(super) class: JavaClassName,
    /// What follows the application's package in the provider's authority.
    pub(super) authority: AuthoritySuffix,
}

/// A Java binary class name, such as `dev.flui.system.FluiDocuments`: two
/// or more `.`-separated ASCII Java identifiers, the grammar of an Android
/// permission name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct JavaClassName(String);

impl JavaClassName {
    /// Parse `name`.
    ///
    /// # Errors
    ///
    /// Why `name` is not a class name.
    pub(crate) fn try_new(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if super::is_dotted_java_name(&name) {
            Ok(Self(name))
        } else {
            Err(format!(
                "`{name}` is not a Java class name: two or more dot-separated identifiers of \
                 ASCII letters, digits and `_`, each starting with a letter or `_`"
            ))
        }
    }

    /// The name, as the manifest's `android:name` states it.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for JavaClassName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::try_new(name)
    }
}

impl Display for JavaClassName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The part of a provider's authority after the application's package, such
/// as `flui.documents`: one or more `.`-separated parts of lower-case ASCII
/// letters and digits, each starting with a letter, at most
/// [`AuthoritySuffix::MAX_BYTES`] in all. Android registers the authority
/// `<package>.<suffix>`, so two applications on one device never claim one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct AuthoritySuffix(String);

impl AuthoritySuffix {
    /// The longest suffix, in bytes.
    pub(crate) const MAX_BYTES: usize = 64;

    /// Parse `suffix`.
    ///
    /// # Errors
    ///
    /// Why `suffix` is not an authority suffix.
    pub(crate) fn try_new(suffix: impl Into<String>) -> Result<Self, String> {
        let suffix = suffix.into();
        let well_formed = suffix.len() <= Self::MAX_BYTES
            && suffix.split('.').all(|part| {
                part.starts_with(|c: char| c.is_ascii_lowercase())
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            });
        if well_formed {
            Ok(Self(suffix))
        } else {
            Err(format!(
                "`{suffix}` is not a provider authority suffix: one or more dot-separated parts \
                 of lower-case ASCII letters and digits, each starting with a letter, {} bytes \
                 at most",
                Self::MAX_BYTES
            ))
        }
    }

    /// The suffix, as the crate declares it.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The authority Android registers for the application whose package is
    /// `application_package`: the package, `.` and the suffix.
    #[must_use]
    pub(crate) fn authority(&self, application_package: &str) -> String {
        format!("{application_package}.{}", self.0)
    }
}

impl TryFrom<String> for AuthoritySuffix {
    type Error = String;

    fn try_from(suffix: String) -> Result<Self, String> {
        Self::try_new(suffix)
    }
}

impl Display for AuthoritySuffix {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthoritySuffix, JavaClassName};

    #[test]
    fn a_provider_names_a_java_class_and_an_authority_suffix() {
        for (name, accepted) in [
            ("dev.flui.system.FluiDocuments", true),
            ("_a.b2", true),
            ("FluiDocuments", false),
            ("dev..x", false),
            ("dev.flui.Flui Documents", false),
            ("dev.flui.\"x", false),
        ] {
            assert_eq!(JavaClassName::try_new(name).is_ok(), accepted, "{name}");
        }
        let longest = format!("a{}", "0".repeat(AuthoritySuffix::MAX_BYTES - 1));
        let over = format!("{longest}0");
        for (suffix, accepted) in [
            ("flui.documents", true),
            ("documents", true),
            ("a1.b2", true),
            (longest.as_str(), true),
            (over.as_str(), false),
            ("", false),
            ("Flui.documents", false),
            ("flui..documents", false),
            ("flui.documents.", false),
            ("1flui", false),
            ("flui_documents", false),
            ("flui\"documents", false),
            ("flui/documents", false),
        ] {
            assert_eq!(
                AuthoritySuffix::try_new(suffix).is_ok(),
                accepted,
                "{suffix:?}"
            );
        }
        assert_eq!(
            AuthoritySuffix::try_new("flui.documents")
                .expect("a suffix")
                .authority("com.example.notes"),
            "com.example.notes.flui.documents"
        );
    }
}
