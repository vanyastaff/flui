//! The Android floor in a Gradle project (ADR-0244 §10): `flui.toml` states
//! it, `flui build` writes it into `gradle.properties` as `flui.minSdk`, and
//! the scaffolded `app/build.gradle.kts` reads that property. A literal
//! `minSdk` in the build script that differs would be a second place the
//! floor is stated, so the build refuses it.

use std::path::Path;

use crate::build::error::{BuildError, BuildResult};

/// The property the scaffolded `app/build.gradle.kts` reads its `minSdk` from.
const PROPERTY: &str = "flui.minSdk";

/// Writes `api` into the project's `gradle.properties` and refuses a `minSdk`
/// in `app/build.gradle.kts` that neither reads that property nor states
/// `api` itself.
pub(crate) fn state_min_sdk(android_dir: &Path, api: u32) -> BuildResult<()> {
    let script = android_dir.join("app").join("build.gradle.kts");
    let refused = std::fs::read_to_string(&script)
        .ok()
        .and_then(|text| stated_min_sdk(&text))
        .and_then(|stated| match stated {
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
    let restated = restate(&text, api);
    if restated != text {
        std::fs::write(&properties, restated)?;
    }
    Ok(())
}

/// `text` (a `gradle.properties`) stating `flui.minSdk=api`: the existing
/// line replaced, or one appended.
fn restate(text: &str, api: u32) -> String {
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            if property_value(line).is_some() {
                found = true;
                format!("{PROPERTY}={api}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{PROPERTY}={api}"));
    }
    let mut restated = lines.join("\n");
    restated.push('\n');
    restated
}

/// The value of a `flui.minSdk` line, `None` for any other line.
fn property_value(line: &str) -> Option<&str> {
    let (key, value) = line.split_once('=')?;
    (key.trim() == PROPERTY).then(|| value.trim())
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

/// The first `minSdk = …` assignment in a Gradle build script, a trailing
/// `//` comment dropped; `None` when the script has none.
fn stated_min_sdk(script: &str) -> Option<MinSdk> {
    script.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "minSdk" {
            return None;
        }
        let value = value
            .split_once("//")
            .map_or(value, |(code, _)| code)
            .trim();
        Some(if let Ok(literal) = value.parse() {
            MinSdk::Literal(literal)
        } else if value.replace(' ', "")
            == format!("providers.gradleProperty(\"{PROPERTY}\").get().toInt()")
        {
            MinSdk::Property
        } else {
            MinSdk::Unverified(value.to_owned())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{MinSdk, restate, stated_min_sdk};

    /// `flui.minSdk` is replaced or appended, other lines kept; a `minSdk`
    /// assignment reads as a literal (its comment dropped), the property, or
    /// an expression the build cannot verify.
    #[test]
    fn gradle_states_the_floor_once() {
        let mut failures = Vec::new();
        for (text, expected) in [
            (
                "android.useAndroidX=true\nflui.minSdk=23\n",
                "android.useAndroidX=true\nflui.minSdk=26\n",
            ),
            (
                "android.useAndroidX=true\n",
                "android.useAndroidX=true\nflui.minSdk=26\n",
            ),
            ("", "flui.minSdk=26\n"),
            ("flui.minSdk = 23\n# kept\n", "flui.minSdk=26\n# kept\n"),
        ] {
            let found = restate(text, 26);
            if found != expected {
                failures.push(format!("{text:?}: {found:?}"));
            }
        }
        for (script, expected) in [
            ("        minSdk = 21\n", Some(MinSdk::Literal(21))),
            (
                "        minSdk = 23 // minimum supported API\n",
                Some(MinSdk::Literal(23)),
            ),
            (
                "        minSdk = providers.gradleProperty(\"flui.minSdk\").get().toInt()\n",
                Some(MinSdk::Property),
            ),
            (
                "        minSdk = libs.versions.minSdk.get().toInt()\n",
                Some(MinSdk::Unverified(
                    "libs.versions.minSdk.get().toInt()".to_owned(),
                )),
            ),
            ("        targetSdk = 35\n", None),
        ] {
            let found = stated_min_sdk(script);
            if found != expected {
                failures.push(format!("{script:?}: {found:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
