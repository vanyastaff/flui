//! What `flui build` states in a Gradle project's `gradle.properties`, and
//! the refusal of a second statement in `app/build.gradle.kts`.
//!
//! - The floor (ADR-0244 §10): `flui.toml` states it (`[platforms.android]
//!   min`), and `flui build` writes it as `flui.minSdk`.
//! - The `targetSdk` (ADR-0255 decision 7) is FLUI's, written as
//!   `flui.targetSdk`; the scaffolded script reads it as its `targetSdk` and
//!   `compileSdk`.
//! - The application's package: `flui.toml`'s `[app] id` gives it
//!   ([`AppId::android_package`](crate::app_id::AppId::android_package)), and
//!   `flui build` writes it as `flui.applicationId`, so a changed identifier
//!   reaches the Gradle build as it reaches the packager without Gradle and
//!   `flui run`.
//!
//! - The Java sources of the crates the application links (ADR-0255
//!   decision 4, ADR-0307: `android.java`), written as `flui.javaSources`,
//!   which the scaffolded script adds to its `main` source set: Gradle
//!   compiles `FluiActivity` and the rest itself, where the packager without
//!   Gradle packs the checked-in dex.
//!
//! The scaffolded `app/build.gradle.kts` reads each property. A literal
//! `minSdk` or `applicationId` in the script that differs would be a second
//! place the value is stated, so the build refuses it, and a script that
//! does not read `flui.javaSources` would build an APK without the Activity
//! its manifest names, so the build refuses that too.

use std::fmt::{Display, Write as _};
use std::path::{Path, PathBuf};

use crate::build::error::{BuildError, BuildResult};

/// The property the scaffolded `app/build.gradle.kts` reads its `minSdk` from.
const PROPERTY: &str = "flui.minSdk";
/// The property the scaffolded `app/build.gradle.kts` reads its `targetSdk`
/// and `compileSdk` from.
const TARGET_PROPERTY: &str = "flui.targetSdk";
/// The property the scaffolded `app/build.gradle.kts` reads its
/// `applicationId` from.
const APPLICATION_ID_PROPERTY: &str = "flui.applicationId";
/// The property the scaffolded `app/build.gradle.kts` reads its Java source
/// directories from.
const JAVA_SOURCES_PROPERTY: &str = "flui.javaSources";

/// What `flui build` states for a Gradle project.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GradleFacts<'a> {
    /// The Android floor, from `flui.toml`.
    pub(crate) min_sdk: u32,
    /// FLUI's `targetSdk`.
    pub(crate) target_sdk: u32,
    /// The Android package, from `flui.toml`'s `[app] id`.
    pub(crate) application_id: &'a str,
    /// The directories of Java sources the linked crates declare.
    pub(crate) java_sources: &'a [PathBuf],
}

/// Writes `facts` into the project's `gradle.properties`, after refusing a
/// `minSdk` or `applicationId` in `app/build.gradle.kts` that neither reads
/// its property nor states the same value.
pub(crate) fn state_project(android_dir: &Path, facts: &GradleFacts<'_>) -> BuildResult<()> {
    let script = android_dir.join("app").join("build.gradle.kts");
    let text = std::fs::read_to_string(&script).unwrap_or_default();
    if let Some(refusal) = refusal(&text, facts) {
        return Err(BuildError::invalid_config(
            script.display().to_string(),
            refusal,
        ));
    }
    let properties = android_dir.join("gradle.properties");
    let text = match std::fs::read_to_string(&properties) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let restated = restate_all(&text, facts)?;
    if restated != text {
        std::fs::write(&properties, restated)?;
    }
    Ok(())
}

/// Why the build script `script` cannot be built with `facts`, if it cannot:
/// the first `minSdk`, then the first `applicationId`, that states another
/// value or one the build cannot read, then Java sources it does not read.
fn refusal(script: &str, facts: &GradleFacts<'_>) -> Option<String> {
    let min_sdk = min_sdk_assignments(script)
        .into_iter()
        .find_map(|stated| match stated {
            Stated::Property => None,
            Stated::Literal(literal) if literal == facts.min_sdk => None,
            Stated::Literal(literal) => Some(literal.to_string()),
            Stated::Unverified(expression) => Some(expression),
        });
    if let Some(stated) = min_sdk {
        return Some(format!(
            "states `minSdk = {stated}`, but the Android floor is {}; state it in flui.toml \
             (`[platforms.android] min`) and read it with \
             `minSdk = providers.gradleProperty(\"{PROPERTY}\").get().toInt()`",
            facts.min_sdk
        ));
    }
    let application_id = application_id_assignments(script)
        .into_iter()
        .find_map(|stated| match stated {
            Stated::Property => None,
            Stated::Literal(literal) if literal == facts.application_id => None,
            Stated::Literal(literal) => Some(format!("\"{literal}\"")),
            Stated::Unverified(expression) => Some(expression),
        });
    if let Some(stated) = application_id {
        return Some(format!(
            "states `applicationId = {stated}`, but flui.toml's `[app] id` gives the package \
             `{}`; change it there and read it with \
             `applicationId = providers.gradleProperty(\"{APPLICATION_ID_PROPERTY}\").get()`",
            facts.application_id
        ));
    }
    let reads_java_sources =
        without_comments(script).contains(&format!("\"{JAVA_SOURCES_PROPERTY}\""));
    (!facts.java_sources.is_empty() && !reads_java_sources).then(|| {
        format!(
            "does not read `{JAVA_SOURCES_PROPERTY}`, the Java sources of the crates the app \
             links, so the APK would lack the Activity its manifest names \
             ({}); add them to the main source set: `java.srcDirs(providers.gradleProperty(\
             \"{JAVA_SOURCES_PROPERTY}\").get().split(File.pathSeparator).filter {{ \
             it.isNotEmpty() }})`",
            crate::build::android_manifest::ACTIVITY
        )
    })
}

/// `text` (a `gradle.properties`) stating every property of `facts`.
///
/// # Errors
///
/// Why the Java source directories cannot be listed in one property: a path
/// that holds the platform's path separator.
fn restate_all(text: &str, facts: &GradleFacts<'_>) -> BuildResult<String> {
    let text = restate(text, PROPERTY, facts.min_sdk);
    let text = restate(&text, TARGET_PROPERTY, facts.target_sdk);
    let text = restate(&text, APPLICATION_ID_PROPERTY, facts.application_id);
    Ok(restate(
        &text,
        JAVA_SOURCES_PROPERTY,
        java_sources_value(facts.java_sources)?,
    ))
}

/// `directories` as one `gradle.properties` value that Kotlin splits on
/// `File.pathSeparator`: `/`-separated, since a `\` escapes in that format,
/// and every character outside ASCII as a `\uXXXX` escape, since Gradle reads
/// the file as ISO 8859-1.
fn java_sources_value(directories: &[PathBuf]) -> BuildResult<String> {
    let joined = std::env::join_paths(directories).map_err(|error| {
        BuildError::Other(format!(
            "the Java source directories cannot be listed for Gradle: {error}"
        ))
    })?;
    let mut value = String::new();
    for c in joined.to_string_lossy().replace('\\', "/").chars() {
        if c.is_ascii() {
            value.push(c);
        } else {
            let mut units = [0_u16; 2];
            for unit in c.encode_utf16(&mut units) {
                let _ = write!(value, "\\u{unit:04x}");
            }
        }
    }
    Ok(value)
}

/// `text` (a `gradle.properties`) stating `property=value`: the existing
/// line replaced, or one appended.
fn restate(text: &str, property: &str, value: impl Display) -> String {
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            if states(line, property) {
                found = true;
                format!("{property}={value}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{property}={value}"));
    }
    let mut restated = lines.join("\n");
    restated.push('\n');
    restated
}

/// Whether `line` assigns `property`.
fn states(line: &str, property: &str) -> bool {
    line.split_once('=')
        .is_some_and(|(key, _)| key.trim() == property)
}

/// What one assignment in a Gradle build script states.
#[derive(Debug, PartialEq, Eq)]
enum Stated<T> {
    /// It reads the property `flui build` writes.
    Property,
    /// A literal value.
    Literal(T),
    /// Anything else (a version catalog, another property, arithmetic, a
    /// string template): the build cannot tell which value it states.
    Unverified(String),
}

impl<T> Stated<T> {
    /// What `value`, the right-hand side of an assignment, states: the read
    /// of `property` written as `read` (spaces aside), a literal `literal`
    /// accepts, or neither.
    fn of(value: &str, read: &str, literal: impl FnOnce(&str) -> Option<T>) -> Self {
        if value.replace(' ', "") == read {
            Self::Property
        } else if let Some(literal) = literal(value) {
            Self::Literal(literal)
        } else {
            Self::Unverified(value.to_owned())
        }
    }
}

/// Every minimum-SDK declaration in a Gradle build script, in order, as
/// [`assignments`] finds them under `minSdk` and `minSdkVersion`.
fn min_sdk_assignments(script: &str) -> Vec<Stated<u32>> {
    let read = format!("providers.gradleProperty(\"{PROPERTY}\").get().toInt()");
    assignments(script, "minSdk", Some("Version"))
        .into_iter()
        .map(|value| Stated::of(&value, &read, |value| value.parse().ok()))
        .collect()
}

/// Every `applicationId` declaration in a Gradle build script, in order, as
/// [`assignments`] finds them; a literal is a plain string literal.
fn application_id_assignments(script: &str) -> Vec<Stated<String>> {
    let read = format!("providers.gradleProperty(\"{APPLICATION_ID_PROPERTY}\").get()");
    assignments(script, "applicationId", None)
        .into_iter()
        .map(|value| Stated::of(&value, &read, string_literal))
        .collect()
}

/// The text of `value` when it is a string literal without templates or
/// escapes (`"com.example.app"` or Groovy's `'com.example.app'`).
fn string_literal(value: &str) -> Option<String> {
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })?;
    (!inner.contains(['"', '\'', '$', '\\'])).then(|| inner.to_owned())
}

/// The values every declaration of `name` in a Gradle build script states,
/// in order: each `name` identifier (or `name` and `alias_suffix`, such as
/// `minSdkVersion`), qualified (`this.minSdk`,
/// `android.defaultConfig.minSdk`) or not, wherever the layout puts it
/// (`defaultConfig { minSdk = 23 }` on one line included), that is assigned
/// rather than read ([`declared_value`]). Comments are dropped first. A later
/// declaration overrides an earlier one, so each must state the value.
fn assignments(script: &str, name: &str, alias_suffix: Option<&str>) -> Vec<String> {
    let code = without_comments(script);
    let mut found = Vec::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after_name = &rest[at + name.len()..];
        let after_name = alias_suffix
            .and_then(|suffix| after_name.strip_prefix(suffix))
            .unwrap_or(after_name);
        let continues_identifier = after_name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let inside_name_or_string =
            before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '"' | '\''));
        rest = after_name;
        if continues_identifier || inside_name_or_string {
            continue;
        }
        if let Some(value) = declared_value(after_name) {
            found.push(value.to_owned());
        }
    }
    found
}

/// The value a declared identifier (qualified or not) states, given the
/// text after it, or `None` when the identifier is only read (`minSdk.get()`,
/// `minSdk == 23`, an argument). A declaration is `= v`, `(v)`, `.set(v)` or
/// Groovy's `minSdk v`; a compound assignment declares an unverified value.
fn declared_value(after_name: &str) -> Option<&str> {
    let trimmed = after_name.trim_start_matches([' ', '\t']);
    let spaced = trimmed.len() < after_name.len();
    if trimmed.starts_with("==") {
        None
    } else if let Some(value) = trimmed.strip_prefix('=') {
        Some(up_to_end(value))
    } else if ["+=", "-=", "*=", "/=", "%="]
        .iter()
        .any(|operator| trimmed.starts_with(operator))
    {
        Some(up_to_end(trimmed))
    } else if let Some(value) = trimmed.strip_prefix('(') {
        Some(call_argument(value))
    } else if let Some(value) = after_name.strip_prefix(".set(") {
        Some(call_argument(value))
    } else if spaced
        && trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '"' | '\''))
    {
        Some(up_to_end(trimmed))
    } else {
        None
    }
}

/// `value` up to the end of its line, a `;` or a `}`.
fn up_to_end(value: &str) -> &str {
    let end = value.find(['\n', ';', '}']).unwrap_or(value.len());
    value[..end].trim()
}

/// `value` (what follows a call's `(`) up to the `)` that closes the call,
/// nested calls included.
fn call_argument(value: &str) -> &str {
    let mut depth = 0_usize;
    let close = value.char_indices().find_map(|(at, c)| match c {
        '(' => {
            depth += 1;
            None
        }
        ')' if depth == 0 => Some(at),
        ')' => {
            depth -= 1;
            None
        }
        _ => None,
    });
    value[..close.unwrap_or(value.len())].trim()
}

/// `script` with its `//` and `/* … */` comments removed.
fn without_comments(script: &str) -> String {
    let mut code = String::with_capacity(script.len());
    let mut rest = script;
    loop {
        // The comment that opens first, and whether it is a block comment.
        let next = [
            rest.find("//").map(|at| (at, false)),
            rest.find("/*").map(|at| (at, true)),
        ]
        .into_iter()
        .flatten()
        .min();
        match next {
            None => {
                code.push_str(rest);
                return code;
            }
            Some((at, false)) => {
                code.push_str(&rest[..at]);
                rest = rest[at..].find('\n').map_or("", |end| &rest[at + end..]);
            }
            Some((at, true)) => {
                code.push_str(&rest[..at]);
                rest = rest[at + 2..]
                    .find("*/")
                    .map_or("", |end| &rest[at + 2 + end + 2..]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GradleFacts, PathBuf, Stated, application_id_assignments, min_sdk_assignments, refusal,
        restate_all, state_project,
    };

    const FACTS: GradleFacts<'static> = GradleFacts {
        min_sdk: 26,
        target_sdk: 36,
        application_id: "com.example.notes",
        java_sources: &[],
    };

    /// `flui.minSdk`, `flui.targetSdk` and `flui.applicationId` are each
    /// replaced or appended, other lines kept, one never taken for another; a
    /// `minSdk` assignment reads as a literal (its comment dropped), the
    /// property, or an expression the build cannot verify.
    #[test]
    fn gradle_states_the_floor_once() {
        let mut failures = Vec::new();
        for (text, expected) in [
            (
                "android.useAndroidX=true\nflui.minSdk=23\n",
                "android.useAndroidX=true\nflui.minSdk=26\nflui.targetSdk=36\n\
                 flui.applicationId=com.example.notes\n",
            ),
            (
                "android.useAndroidX=true\n",
                "android.useAndroidX=true\nflui.minSdk=26\nflui.targetSdk=36\n\
                 flui.applicationId=com.example.notes\n",
            ),
            (
                "",
                "flui.minSdk=26\nflui.targetSdk=36\nflui.applicationId=com.example.notes\n",
            ),
            (
                "flui.minSdk = 23\n# kept\nflui.targetSdk = 35\nflui.applicationId = com.old.notes\n",
                "flui.minSdk=26\n# kept\nflui.targetSdk=36\nflui.applicationId=com.example.notes\n",
            ),
            (
                "flui.applicationId=com.old.notes\nflui.targetSdk=35\nflui.minSdk=23\n",
                "flui.applicationId=com.example.notes\nflui.targetSdk=36\nflui.minSdk=26\n",
            ),
        ] {
            let found = restate_all(text, &FACTS).expect("restated");
            // No Java sources: the property is stated empty, after the rest.
            let expected = format!("{expected}flui.javaSources=\n");
            if found != expected {
                failures.push(format!("{text:?}: {found:?}"));
            }
        }
        for (script, expected) in [
            ("        minSdk = 21\n", vec![Stated::Literal(21)]),
            (
                "        minSdk = 23 // minimum supported API\n",
                vec![Stated::Literal(23)],
            ),
            (
                "        minSdk = providers.gradleProperty(\"flui.minSdk\").get().toInt()\n",
                vec![Stated::Property],
            ),
            (
                "        minSdk = libs.versions.minSdk.get().toInt()\n",
                vec![Stated::Unverified(
                    "libs.versions.minSdk.get().toInt()".to_owned(),
                )],
            ),
            (
                "        minSdk = providers.gradleProperty(\"flui.minSdk\").get().toInt()\n\
                 \x20       minSdk = 23\n",
                vec![Stated::Property, Stated::Literal(23)],
            ),
            (
                "    defaultConfig { minSdk = 23 }\n",
                vec![Stated::Literal(23)],
            ),
            (
                "        minSdkVersion 21\n        minSdkVersion(22); targetSdk = 35\n",
                vec![Stated::Literal(21), Stated::Literal(22)],
            ),
            (
                "        /* minSdk = 1 */ minSdk = 26 // minSdk = 2\n",
                vec![Stated::Literal(26)],
            ),
            ("        minSdk.set(23)\n", vec![Stated::Literal(23)]),
            (
                "    defaultConfig { this.minSdk = 23 }\n",
                vec![Stated::Literal(23)],
            ),
            (
                "android.defaultConfig.minSdk = 23\n",
                vec![Stated::Literal(23)],
            ),
            (
                "android.defaultConfig.minSdkVersion 21\n",
                vec![Stated::Literal(21)],
            ),
            (
                "android.defaultConfig.minSdk.set(libs.versions.min.get().toInt())\n",
                vec![Stated::Unverified(
                    "libs.versions.min.get().toInt()".to_owned(),
                )],
            ),
            (
                "        minSdk += 3\n",
                vec![Stated::Unverified("+= 3".to_owned())],
            ),
            (
                "val floor = android.defaultConfig.minSdk ?: 21\n\
                 check(minSdk == 26)\n\
                 println(android.defaultConfig.minSdk)\n",
                vec![],
            ),
            (
                include_str!("../../templates/platforms/android/build.gradle.kts"),
                vec![Stated::Property],
            ),
            ("        targetSdk = 35\n", vec![]),
        ] {
            let found = min_sdk_assignments(script);
            if found != expected {
                failures.push(format!("{script:?}: {found:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// An `applicationId` assignment reads as a plain string literal, the
    /// property, or an expression the build cannot verify; a suffix, a read
    /// and the property's own name are not assignments.
    #[test]
    fn gradle_states_the_package_once() {
        let literal = |id: &str| Stated::Literal(id.to_owned());
        let unverified = |expression: &str| Stated::Unverified(expression.to_owned());
        let mut failures = Vec::new();
        for (script, expected) in [
            (
                "        applicationId = \"com.example.notes\"\n",
                vec![literal("com.example.notes")],
            ),
            (
                "        applicationId 'com.example.notes' // Groovy\n",
                vec![literal("com.example.notes")],
            ),
            (
                "        applicationId = providers.gradleProperty(\"flui.applicationId\").get()\n",
                vec![Stated::Property],
            ),
            (
                "        applicationId = \"com.example.$flavor\"\n",
                vec![unverified("\"com.example.$flavor\"")],
            ),
            (
                "        applicationId.set(libs.versions.id.get())\n",
                vec![unverified("libs.versions.id.get()")],
            ),
            (
                "    defaultConfig { this.applicationId = \"com.a.b\" }\n\
                 android.defaultConfig.applicationId = \"com.c.d\"\n",
                vec![literal("com.a.b"), literal("com.c.d")],
            ),
            (
                "        applicationIdSuffix = \".debug\"\n\
                 println(android.defaultConfig.applicationId)\n\
                 check(applicationId == \"x\")\n\
                 /* applicationId = \"com.old\" */\n",
                vec![],
            ),
            (
                include_str!("../../templates/platforms/android/build.gradle.kts"),
                vec![Stated::Property],
            ),
        ] {
            let found = application_id_assignments(script);
            if found != expected {
                failures.push(format!("{script:?}: {found:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The build refuses a script whose `minSdk` or `applicationId` states
    /// another value than `flui.toml`, or one it cannot read, and leaves
    /// `gradle.properties` as it was; a script that reads the properties, or
    /// states the same values, gets them written.
    #[test]
    fn a_build_script_states_what_flui_toml_states_or_is_refused() {
        let reads = include_str!("../../templates/platforms/android/build.gradle.kts");
        let cases = [
            (reads.to_owned(), None),
            (
                "    minSdk = 26\n    applicationId = \"com.example.notes\"\n".to_owned(),
                None,
            ),
            (
                reads.replace(
                    "providers.gradleProperty(\"flui.applicationId\").get()",
                    "\"com.example.old_name\"",
                ),
                Some("`applicationId = \"com.example.old_name\"`"),
            ),
            (
                "    applicationId = \"com.example.\" + name\n".to_owned(),
                Some("`applicationId = \"com.example.\" + name`"),
            ),
            ("    minSdk = 21\n".to_owned(), Some("`minSdk = 21`")),
        ];
        let mut failures = Vec::new();
        for (script, refused) in cases {
            let named = refusal(&script, &FACTS);
            let as_named = match (&named, refused) {
                (None, None) => true,
                (Some(named), Some(refused)) => named.contains(refused),
                _ => false,
            };
            if !as_named {
                failures.push(format!("{script:?}: {named:?}"));
            }

            let project = tempfile::tempdir().expect("a temporary directory");
            let app = project.path().join("app");
            std::fs::create_dir_all(&app).expect("the app directory");
            std::fs::write(app.join("build.gradle.kts"), &script).expect("the build script");
            let properties = project.path().join("gradle.properties");
            std::fs::write(&properties, "flui.applicationId=com.example.old_name\n")
                .expect("the properties");
            let stated = state_project(project.path(), &FACTS);
            let written = std::fs::read_to_string(&properties).expect("the properties");
            let as_expected = match refused {
                None => {
                    stated.is_ok()
                        && written
                            .lines()
                            .any(|line| line == "flui.applicationId=com.example.notes")
                }
                Some(_) => {
                    stated.is_err() && written == "flui.applicationId=com.example.old_name\n"
                }
            };
            if !as_expected {
                failures.push(format!("{script:?}: {stated:?}, {written:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The Java source directories the linked crates declare reach Gradle as
    /// one property it splits on `File.pathSeparator`, `/`-separated and with
    /// every non-ASCII character escaped; a script that does not read them is
    /// refused, naming the Activity the APK would lack, while the scaffolded
    /// one reads them.
    #[test]
    fn gradle_compiles_the_java_sources_the_crates_declare() {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let sources = [
            PathBuf::from("/deps/flui-native/android/java"),
            PathBuf::from("/home/ваня/other/java"),
        ];
        let facts = GradleFacts {
            java_sources: &sources,
            ..FACTS
        };
        let written = restate_all("", &facts).expect("restated");
        let expected = format!(
            "flui.javaSources=/deps/flui-native/android/java{separator}\
             /home/\\u0432\\u0430\\u043d\\u044f/other/java\n"
        );
        assert!(written.ends_with(&expected), "{written}");

        let template = include_str!("../../templates/platforms/android/build.gradle.kts");
        assert_eq!(refusal(template, &facts), None);
        let without = template.replace("\"flui.javaSources\"", "\"other.sources\"");
        let refused = refusal(&without, &facts).expect("a script without the sources");
        assert!(
            refused.contains("flui.javaSources")
                && refused.contains(crate::build::android_manifest::ACTIVITY),
            "{refused}"
        );
        assert_eq!(
            refusal(&without, &FACTS),
            None,
            "no sources declared, none to read"
        );
    }
}
