//! The application identifier `flui.toml` holds (`[app] id`), and every
//! platform form of it.
//!
//! One canonical [`AppId`] is written once, by `flui create`, and each platform
//! name is derived from it here and nowhere else: the Android package
//! ([`AppId::android_package`]), the Apple bundle identifier
//! ([`AppId::apple_bundle_id`]), and the `FLUI_APP_ID` the application is
//! compiled with ([`AppId::as_str`]). The grammar is `flui_log::AppId`'s; the
//! CLI ships without the framework, so it keeps its own parser and a test
//! holds the two to the same answers.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::error::{CliError, CliResult};
use crate::types::{OrganizationId, ProjectName};

/// The longest identifier, in bytes: the longest directory name, since the
/// application's data directory is named after it.
const MAX_LEN: usize = 255;

/// Words Java reserves, which Android refuses as a package-name label.
const JAVA_RESERVED: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "void",
    "volatile",
    "while",
];

/// A validated reverse-DNS application identifier, such as
/// `com.example.my_app`: two or more `.`-separated labels, each starting with
/// an ASCII letter and continuing with ASCII letters, digits and `_`, none a
/// word Java reserves, at most 255 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct AppId(String);

impl AppId {
    /// Parse `id`.
    ///
    /// # Errors
    ///
    /// [`CliError::InvalidAppId`] naming the first rule `id` breaks.
    pub(crate) fn try_new(id: impl Into<String>) -> CliResult<Self> {
        let id = id.into();
        match refusal(&id) {
            None => Ok(Self(id)),
            Some(reason) => Err(CliError::InvalidAppId { id, reason }),
        }
    }

    /// The identifier `flui create` gives a project: the organization, then
    /// the project name with `-` as `_`.
    ///
    /// # Errors
    ///
    /// [`CliError::InvalidAppId`] when the two do not make an identifier, such
    /// as a name starting with `_` or an organization label starting with a
    /// digit.
    pub(crate) fn from_parts(org: &OrganizationId, name: &ProjectName) -> CliResult<Self> {
        Self::try_new(format!(
            "{}.{}",
            org.as_str(),
            name.as_str().replace('-', "_")
        ))
    }

    /// The identifier of an application built without a `flui.toml`, such as
    /// an example in FLUI's own repository: `dev.flui.` and its name with
    /// everything but ASCII letters and digits as `_`, led by `app_` when it
    /// does not start with a letter.
    #[must_use]
    pub(crate) fn unconfigured(name: &str) -> Self {
        let mut label: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        if !label.starts_with(|c: char| c.is_ascii_alphabetic())
            || JAVA_RESERVED.contains(&label.as_str())
        {
            label.insert_str(0, "app_");
        }
        label.truncate(MAX_LEN - "dev.flui.".len());
        Self::try_new(format!("dev.flui.{label}"))
            .expect("BUG: an unconfigured name is made into a valid identifier")
    }

    /// The identifier as written in `flui.toml`, and as the application is
    /// compiled with (`FLUI_APP_ID`).
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The Android package (`applicationId`, the manifest's `package`): the
    /// identifier itself, whose grammar is the package grammar.
    #[must_use]
    pub(crate) fn android_package(&self) -> &str {
        &self.0
    }

    /// The Apple bundle identifier (`CFBundleIdentifier`): the identifier with
    /// `_`, which a bundle identifier does not accept, as `-`.
    #[must_use]
    pub(crate) fn apple_bundle_id(&self) -> String {
        self.0.replace('_', "-")
    }
}

/// Whether `label` is a name Windows reserves for a device in every
/// directory, in any case.
fn is_windows_device(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    match label.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// Why `id` is not an [`AppId`], if it is not.
fn refusal(id: &str) -> Option<String> {
    if id.len() > MAX_LEN {
        return Some(format!("it is longer than {MAX_LEN} bytes"));
    }
    let labels: Vec<&str> = id.split('.').collect();
    if labels.len() < 2 {
        return Some(
            "it needs at least two dot-separated labels, for example `com.example.app`".into(),
        );
    }
    for (position, label) in labels.into_iter().enumerate() {
        let Some(first) = label.chars().next() else {
            return Some("it has an empty label".into());
        };
        if let Some(c) = label
            .chars()
            .find(|c| !c.is_ascii_alphanumeric() && *c != '_')
        {
            return Some(format!(
                "it contains {c:?}; labels accept ASCII letters, digits and `_`"
            ));
        }
        if !first.is_ascii_alphabetic() {
            return Some(format!(
                "label `{label}` does not start with an ASCII letter"
            ));
        }
        if position == 0 && is_windows_device(label) {
            return Some(format!(
                "label `{label}` is a name Windows reserves for a device, so it cannot name the \
                 data directory"
            ));
        }
        if JAVA_RESERVED.contains(&label) {
            return Some(format!("label `{label}` is a word Java reserves"));
        }
    }
    None
}

impl TryFrom<String> for AppId {
    type Error = CliError;

    fn try_from(id: String) -> CliResult<Self> {
        Self::try_new(id)
    }
}

impl From<AppId> for String {
    fn from(id: AppId) -> Self {
        id.0
    }
}

impl Display for AppId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLI accepts exactly the identifiers the framework does, so what
    /// `flui.toml` holds is what the application is compiled with.
    #[test]
    fn the_cli_and_the_framework_accept_the_same_identifiers() {
        let long = format!("com.{}", "a".repeat(251));
        let too_long = format!("com.{}", "a".repeat(252));
        let cases = [
            "com.example.notes",
            "com.example.my_app",
            "dev.Flui.App2",
            long.as_str(),
            too_long.as_str(),
            "notes",
            "com..notes",
            "com.example.",
            ".com.example",
            "com.my-company.notes",
            "com.example.2notes",
            "com.example._notes",
            "com.example.new",
            "com.example.Ünïcode",
            "con.example",
            "Aux.notes",
            "lpt1.notes",
            "com.con.app",
        ];
        for id in cases {
            assert_eq!(
                AppId::try_new(id).is_ok(),
                flui_log::AppId::try_new(id).is_ok(),
                "{id}"
            );
        }
    }

    #[test]
    fn each_platform_form_is_derived_from_the_one_identifier() {
        let id = AppId::try_new("com.example.my_app2").expect("an identifier");
        assert_eq!(id.as_str(), "com.example.my_app2");
        assert_eq!(id.android_package(), "com.example.my_app2");
        assert_eq!(id.apple_bundle_id(), "com.example.my-app2");
    }

    #[test]
    fn a_project_identifier_is_its_organization_and_name() {
        let org = |org: &str| OrganizationId::new(org).expect("an organization");
        let name = |name: &str| ProjectName::new(name).expect("a project name");
        let cases = [
            ("com.example", "my-app", Some("com.example.my_app")),
            ("com.example", "notes", Some("com.example.notes")),
            ("com.example", "_private", None),
            ("com.example", "new", None),
            ("com.1example", "notes", None),
        ];
        for (organization, project, expected) in cases {
            let made = AppId::from_parts(&org(organization), &name(project)).ok();
            assert_eq!(
                made.as_ref().map(AppId::as_str),
                expected,
                "{organization} + {project}"
            );
        }
    }

    #[test]
    fn an_unconfigured_application_still_gets_an_identifier() {
        let cases = [
            ("counter", "dev.flui.counter"),
            ("My App", "dev.flui.My_App"),
            ("3d-demo", "dev.flui.app_3d_demo"),
            ("new", "dev.flui.app_new"),
        ];
        for (name, expected) in cases {
            assert_eq!(AppId::unconfigured(name).as_str(), expected, "{name}");
        }
    }
}
