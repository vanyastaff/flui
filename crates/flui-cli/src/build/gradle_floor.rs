//! The Android API levels in a Gradle project. The floor (ADR-0244 §10):
//! `flui.toml` states it, `flui build` writes it into `gradle.properties` as
//! `flui.minSdk`, and the scaffolded `app/build.gradle.kts` reads that
//! property. A literal `minSdk` in the build script that differs would be a
//! second place the floor is stated, so the build refuses it. The `targetSdk`
//! (ADR-0255 decision 7) is FLUI's, written as `flui.targetSdk` beside it, and
//! the scaffolded script reads it as its `targetSdk` and `compileSdk`.

use std::path::Path;

use crate::build::error::{BuildError, BuildResult};

/// The property the scaffolded `app/build.gradle.kts` reads its `minSdk` from.
const PROPERTY: &str = "flui.minSdk";
/// The property the scaffolded `app/build.gradle.kts` reads its `targetSdk`
/// and `compileSdk` from.
const TARGET_PROPERTY: &str = "flui.targetSdk";

/// Writes `api` and `target` into the project's `gradle.properties` and
/// refuses a `minSdk` in `app/build.gradle.kts` that neither reads that
/// property nor states `api` itself.
pub(crate) fn state_sdk_levels(android_dir: &Path, api: u32, target: u32) -> BuildResult<()> {
    let script = android_dir.join("app").join("build.gradle.kts");
    let text = std::fs::read_to_string(&script).unwrap_or_default();
    let refused = min_sdk_assignments(&text)
        .into_iter()
        .find_map(|stated| match stated {
            MinSdk::Property => None,
            MinSdk::Literal(literal) if literal == api => None,
            MinSdk::Literal(literal) => Some(literal.to_string()),
            MinSdk::Unverified(expression) => Some(expression),
        });
    if let Some(stated) = refused {
        return Err(BuildError::invalid_config(
            script.display().to_string(),
            format!(
                "states `minSdk = {stated}`, but the Android floor is {api}; state it in \
                 flui.toml (`[platforms.android] min`) and read it with \
                 `minSdk = providers.gradleProperty(\"{PROPERTY}\").get().toInt()`"
            ),
        ));
    }
    let properties = android_dir.join("gradle.properties");
    let text = match std::fs::read_to_string(&properties) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let restated = restate(&restate(&text, PROPERTY, api), TARGET_PROPERTY, target);
    if restated != text {
        std::fs::write(&properties, restated)?;
    }
    Ok(())
}

/// `text` (a `gradle.properties`) stating `property=value`: the existing
/// line replaced, or one appended.
fn restate(text: &str, property: &str, value: u32) -> String {
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

/// What a Gradle build script's `minSdk` assignment states.
#[derive(Debug, PartialEq, Eq)]
enum MinSdk {
    /// It reads `flui.minSdk`, which `flui build` writes.
    Property,
    /// A number.
    Literal(u32),
    /// Anything else (a version catalog, another property, arithmetic): the
    /// build cannot tell which floor it states.
    Unverified(String),
}

/// Every minimum-SDK declaration in a Gradle build script, in order: each
/// `minSdk` or `minSdkVersion` identifier, qualified (`this.minSdk`,
/// `android.defaultConfig.minSdk`) or not, wherever the layout puts it
/// (`defaultConfig { minSdk = 23 }` on one line included), that is assigned
/// rather than read ([`declared_value`]). Comments are dropped first. A later
/// declaration overrides an earlier one, so each must state the floor; a
/// value the build cannot read is an unverified one.
fn min_sdk_assignments(script: &str) -> Vec<MinSdk> {
    let code = without_comments(script);
    let mut found = Vec::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find("minSdk") {
        let before = rest[..at].chars().next_back();
        let after_name = &rest[at + "minSdk".len()..];
        let after_name = after_name.strip_prefix("Version").unwrap_or(after_name);
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
        let Some(value) = declared_value(after_name) else {
            continue;
        };
        found.push(if let Ok(literal) = value.parse() {
            MinSdk::Literal(literal)
        } else if value.replace(' ', "")
            == format!("providers.gradleProperty(\"{PROPERTY}\").get().toInt()")
        {
            MinSdk::Property
        } else {
            MinSdk::Unverified(value.to_owned())
        });
    }
    found
}

/// The value a `minSdk` identifier (qualified or not) declares, given the
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
    use super::{MinSdk, PROPERTY, TARGET_PROPERTY, min_sdk_assignments, restate};

    /// `flui.minSdk` and `flui.targetSdk` are each replaced or appended, other
    /// lines kept, one never taken for the other; a `minSdk` assignment reads
    /// as a literal (its comment dropped), the property, or an expression the
    /// build cannot verify.
    #[test]
    fn gradle_states_the_floor_once() {
        let mut failures = Vec::new();
        for (text, expected) in [
            (
                "android.useAndroidX=true\nflui.minSdk=23\n",
                "android.useAndroidX=true\nflui.minSdk=26\nflui.targetSdk=36\n",
            ),
            (
                "android.useAndroidX=true\n",
                "android.useAndroidX=true\nflui.minSdk=26\nflui.targetSdk=36\n",
            ),
            ("", "flui.minSdk=26\nflui.targetSdk=36\n"),
            (
                "flui.minSdk = 23\n# kept\nflui.targetSdk = 35\n",
                "flui.minSdk=26\n# kept\nflui.targetSdk=36\n",
            ),
            (
                "flui.targetSdk=35\nflui.minSdk=23\n",
                "flui.targetSdk=36\nflui.minSdk=26\n",
            ),
        ] {
            let found = restate(&restate(text, PROPERTY, 26), TARGET_PROPERTY, 36);
            if found != expected {
                failures.push(format!("{text:?}: {found:?}"));
            }
        }
        for (script, expected) in [
            ("        minSdk = 21\n", vec![MinSdk::Literal(21)]),
            (
                "        minSdk = 23 // minimum supported API\n",
                vec![MinSdk::Literal(23)],
            ),
            (
                "        minSdk = providers.gradleProperty(\"flui.minSdk\").get().toInt()\n",
                vec![MinSdk::Property],
            ),
            (
                "        minSdk = libs.versions.minSdk.get().toInt()\n",
                vec![MinSdk::Unverified(
                    "libs.versions.minSdk.get().toInt()".to_owned(),
                )],
            ),
            (
                "        minSdk = providers.gradleProperty(\"flui.minSdk\").get().toInt()\n\
                 \x20       minSdk = 23\n",
                vec![MinSdk::Property, MinSdk::Literal(23)],
            ),
            (
                "    defaultConfig { minSdk = 23 }\n",
                vec![MinSdk::Literal(23)],
            ),
            (
                "        minSdkVersion 21\n        minSdkVersion(22); targetSdk = 35\n",
                vec![MinSdk::Literal(21), MinSdk::Literal(22)],
            ),
            (
                "        /* minSdk = 1 */ minSdk = 26 // minSdk = 2\n",
                vec![MinSdk::Literal(26)],
            ),
            ("        minSdk.set(23)\n", vec![MinSdk::Literal(23)]),
            (
                "    defaultConfig { this.minSdk = 23 }\n",
                vec![MinSdk::Literal(23)],
            ),
            (
                "android.defaultConfig.minSdk = 23\n",
                vec![MinSdk::Literal(23)],
            ),
            (
                "android.defaultConfig.minSdkVersion 21\n",
                vec![MinSdk::Literal(21)],
            ),
            (
                "android.defaultConfig.minSdk.set(libs.versions.min.get().toInt())\n",
                vec![MinSdk::Unverified(
                    "libs.versions.min.get().toInt()".to_owned(),
                )],
            ),
            (
                "        minSdk += 3\n",
                vec![MinSdk::Unverified("+= 3".to_owned())],
            ),
            (
                "val floor = android.defaultConfig.minSdk ?: 21\n\
                 check(minSdk == 26)\n\
                 println(android.defaultConfig.minSdk)\n",
                vec![],
            ),
            (
                include_str!("../../templates/platforms/android/build.gradle.kts"),
                vec![MinSdk::Property],
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
}
