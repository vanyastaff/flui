//! The OS version floors, declared once in `platform-support.toml` (ADR-0244).
//!
//! `flui-native`, `flui-system` and `flui-cli` are published crates whose package archives cannot
//! read a repository-root file, so a committed Rust source in each holds the floors that crate
//! compiles in, beside the version map under `docs/`. [`the_checked_in_files_agree`] regenerates
//! every one in memory and fails when a committed file differs or a template states another floor;
//! with `FLUI_BLESS=1` it writes the files and restates the template floors instead:
//!
//! ```text
//! FLUI_BLESS=1 cargo nextest run -p flui-cli --test cli_it -E 'test(platform_floor)'
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The hand-written source, relative to the repository root.
const SOURCE: &str = "platform-support.toml";

/// The Rust source holding the floors `flui-native` compiles in.
const PLATFORM_OUTPUT: &str = "crates/flui-native/src/version/floor.rs";
/// The Rust source holding the floors `flui-cli` packages with.
const CLI_OUTPUT: &str = "crates/flui-cli/src/build/platform_floor.rs";
/// The Rust source holding the Apple floors `flui-system`'s gates compile against.
const SYSTEM_OUTPUT: &str = "crates/flui-system/src/apple/floor.rs";
/// The version map.
const MAP_OUTPUT: &str = "docs/version-map.md";
/// The Gradle properties `flui platform add android` scaffolds; the scaffolded
/// `build.gradle.kts` reads its `flui.minSdk` and `flui.targetSdk`, and `flui build` restates
/// them.
const GRADLE_TEMPLATE: &str = "crates/flui-cli/templates/platforms/android/gradle.properties";

/// What a reader runs after editing `platform-support.toml`.
const BLESS_COMMAND: &str =
    "FLUI_BLESS=1 cargo nextest run -p flui-cli --test cli_it -E 'test(platform_floor)'";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("reading {path}: {error}"))
}

/// A `flui.minSdk = N` or `flui.targetSdk = N` line, split around its value.
struct SdkLine<'a> {
    /// The indentation, the key and the `=` with its spaces.
    prefix: &'a str,
    key: &'static str,
    value: u32,
    /// The whitespace after the value.
    suffix: &'a str,
}

/// The line, without its line ending, when it is a `flui.minSdk` or `flui.targetSdk` assignment
/// of a number.
fn sdk_line(body: &str) -> Option<SdkLine<'_>> {
    let rest = body.trim_start();
    let (key, after_key) = ["flui.minSdk", "flui.targetSdk"]
        .into_iter()
        .find_map(|key| Some((key, rest.strip_prefix(key)?)))?;
    let after_equals = after_key.trim_start().strip_prefix('=')?.trim_start();
    let digits = after_equals
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_equals.len());
    let (number, suffix) = after_equals.split_at(digits);
    if number.is_empty() || !suffix.trim().is_empty() {
        return None;
    }
    Some(SdkLine {
        prefix: &body[..body.len() - after_equals.len()],
        key,
        value: number.parse().ok()?,
        suffix,
    })
}

/// Each Gradle property the template states, with the field of `platform-support.toml` it
/// restates and that field's value.
fn gradle_properties(floors: &Floors) -> [(&'static str, &'static str, u32); 2] {
    [
        ("flui.minSdk", "android.api", floors.android.api),
        (
            "flui.targetSdk",
            "android.target-sdk",
            floors.android.target_sdk,
        ),
    ]
}

/// `platform-support.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Floors {
    windows: Windows,
    macos: MacOs,
    ios: Ios,
    android: Android,
    linux: Linux,
    web: Web,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Windows {
    build: u32,
    release: String,
    set_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct MacOs {
    #[serde(rename = "x86_64")]
    x86_64: String,
    aarch64: String,
    set_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Ios {
    version: String,
    set_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Android {
    api: u32,
    release: String,
    set_by: String,
    /// The `targetSdk` every APK declares (ADR-0255 decision 7): a packaging value, not a floor.
    target_sdk: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Linux {
    glibc: String,
    requires: Vec<String>,
    set_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Web {
    requires: Vec<String>,
    set_by: String,
}

impl Floors {
    /// Parses and validates the file: every dotted version is numeric, and nothing is empty.
    fn parse(text: &str) -> Result<Self, String> {
        let floors: Self =
            toml::from_str(text).map_err(|error| format!("parsing {SOURCE}: {error}"))?;
        for (field, version) in [
            ("macos.x86_64", &floors.macos.x86_64),
            ("macos.aarch64", &floors.macos.aarch64),
            ("ios.version", &floors.ios.version),
            ("linux.glibc", &floors.linux.glibc),
        ] {
            let parts: Vec<_> = version.split('.').collect();
            if !(2..=3).contains(&parts.len())
                || !parts.iter().all(|part| part.parse::<u32>().is_ok())
            {
                return Err(format!(
                    "{SOURCE}: {field} = \"{version}\" is not a dotted version like \"13.0\""
                ));
            }
        }
        for (field, text) in [
            ("windows.release", &floors.windows.release),
            ("windows.set-by", &floors.windows.set_by),
            ("macos.set-by", &floors.macos.set_by),
            ("ios.set-by", &floors.ios.set_by),
            ("android.release", &floors.android.release),
            ("android.set-by", &floors.android.set_by),
            ("linux.set-by", &floors.linux.set_by),
            ("web.set-by", &floors.web.set_by),
        ] {
            if text.trim().is_empty() {
                return Err(format!("{SOURCE}: {field} is empty"));
            }
        }
        if floors.android.target_sdk < floors.android.api {
            return Err(format!(
                "{SOURCE}: android.target-sdk = {} is below android.api = {}",
                floors.android.target_sdk, floors.android.api
            ));
        }
        if floors.linux.requires.is_empty() || floors.web.requires.is_empty() {
            return Err(format!(
                "{SOURCE}: linux.requires and web.requires each name at least one requirement"
            ));
        }
        Ok(floors)
    }
}

/// Files by repository-relative path, with their contents.
type Files = Vec<(&'static str, String)>;

/// Every generated file.
fn generate(floors: &Floors) -> Files {
    vec![
        (PLATFORM_OUTPUT, platform_source(floors)),
        (CLI_OUTPUT, cli_source(floors)),
        (SYSTEM_OUTPUT, system_source(floors)),
        (MAP_OUTPUT, version_map(floors)),
    ]
}

/// The Apple floors of `flui-system`, a capability crate that cannot reach `flui-native`'s:
/// each is built for its system and for the tests of the module that holds the gates.
fn system_source(floors: &Floors) -> String {
    let (major, minor, patch) = dotted(&floors.ios.version);
    let (arm_major, arm_minor, arm_patch) = dotted(&floors.macos.aarch64);
    let (intel_major, intel_minor, intel_patch) = dotted(&floors.macos.x86_64);
    format!(
        "{GENERATED_HEADER}\n\
         //! The Apple floors this crate compiles in (ADR-0244); `apple::version` refuses a gate at\n\
         //! or below its system's.\n\
         \n\
         /// The lowest iOS version FLUI supports, as (major, minor, patch).\n\
         #[cfg(any(target_os = \"ios\", test))]\n\
         pub(crate) const IOS_FLOOR: (u32, u32, u32) = ({major}, {minor}, {patch});\n\
         \n\
         /// The lowest macOS version FLUI supports on arm64, as (major, minor, patch).\n\
         #[cfg(all(any(target_os = \"macos\", test), target_arch = \"aarch64\"))]\n\
         pub(crate) const MACOS_FLOOR: (u32, u32, u32) = ({arm_major}, {arm_minor}, {arm_patch});\n\
         \n\
         /// The lowest macOS version FLUI supports on x86_64, as (major, minor, patch).\n\
         #[cfg(all(any(target_os = \"macos\", test), not(target_arch = \"aarch64\")))]\n\
         pub(crate) const MACOS_FLOOR: (u32, u32, u32) = ({intel_major}, {intel_minor}, {intel_patch});\n"
    )
}

const GENERATED_HEADER: &str = "// Generated by flui-cli's `platform_floor` test from `platform-support.toml`.\n\
     // Edit that file, then run `FLUI_BLESS=1 cargo nextest run -p flui-cli --test cli_it -E 'test(platform_floor)'`.\n";

fn platform_source(floors: &Floors) -> String {
    let (major, minor, patch) = dotted(&floors.ios.version);
    let (arm_major, arm_minor, arm_patch) = dotted(&floors.macos.aarch64);
    let (intel_major, intel_minor, intel_patch) = dotted(&floors.macos.x86_64);
    format!(
        "{GENERATED_HEADER}\n\
         //! The floors this crate compiles in (ADR-0244); `version::gate` refuses a gate at or below\n\
         //! them.\n\
         \n\
         /// The lowest Windows build FLUI supports, as `RtlGetVersion` reports it.\n\
         #[cfg(any(windows, test))]\n\
         pub(crate) const WINDOWS_BUILD: u32 = {};\n\
         \n\
         /// The lowest iOS version FLUI supports, as (major, minor, patch).\n\
         #[cfg(any(target_os = \"ios\", test))]\n\
         pub(crate) const IOS_FLOOR: (u32, u32, u32) = ({major}, {minor}, {patch});\n\
         \n\
         /// The lowest macOS version FLUI supports on arm64, as (major, minor, patch).\n\
         #[cfg(all(any(target_os = \"macos\", test), target_arch = \"aarch64\"))]\n\
         pub(crate) const MACOS_FLOOR: (u32, u32, u32) = ({arm_major}, {arm_minor}, {arm_patch});\n\
         \n\
         /// The lowest macOS version FLUI supports on x86_64, as (major, minor, patch).\n\
         #[cfg(all(any(target_os = \"macos\", test), not(target_arch = \"aarch64\")))]\n\
         pub(crate) const MACOS_FLOOR: (u32, u32, u32) = ({intel_major}, {intel_minor}, {intel_patch});\n\
         \n\
         /// The lowest Android API level FLUI supports.\n\
         #[cfg(any(target_os = \"android\", test))]\n\
         pub(crate) const ANDROID_API: u32 = {};\n",
        floors.windows.build, floors.android.api
    )
}

/// The components of a dotted version [`Floors::parse`] accepted, the patch zero when absent.
fn dotted(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').map(|part| {
        part.parse::<u32>()
            .expect("BUG: Floors::parse checked the version")
    });
    let mut next = || parts.next().unwrap_or(0);
    (next(), next(), next())
}

fn cli_source(floors: &Floors) -> String {
    let (ios_major, ios_minor, ios_patch) = dotted(&floors.ios.version);
    let (arm_major, arm_minor, arm_patch) = dotted(&floors.macos.aarch64);
    let (intel_major, intel_minor, intel_patch) = dotted(&floors.macos.x86_64);
    format!(
        "{GENERATED_HEADER}\n\
         //! The floors applications are packaged with (ADR-0244); an application's own\n\
         //! `[platforms.<environment>] min` may only raise them.\n\
         \n\
         /// The lowest Windows build FLUI supports, as `RtlGetVersion` reports it.\n\
         pub(crate) const WINDOWS_BUILD: u32 = {};\n\
         \n\
         /// The lowest macOS release FLUI supports on arm64, as (major, minor, patch).\n\
         pub(crate) const MACOS_AARCH64: (u32, u32, u32) = ({arm_major}, {arm_minor}, {arm_patch});\n\
         \n\
         /// The lowest macOS release FLUI supports on x86_64, as (major, minor, patch).\n\
         pub(crate) const MACOS_X86_64: (u32, u32, u32) = ({intel_major}, {intel_minor}, {intel_patch});\n\
         \n\
         /// The lowest iOS release FLUI supports, as (major, minor, patch).\n\
         pub(crate) const IOS: (u32, u32, u32) = ({ios_major}, {ios_minor}, {ios_patch});\n\
         \n\
         /// The Android `minSdk`: the lowest API level FLUI supports.\n\
         pub(crate) const ANDROID_MIN_SDK: u32 = {};\n\
         \n\
         /// The Android `targetSdk` every APK declares (ADR-0255 decision 7). A packaging value,\n\
         /// not a floor: `flui.toml` does not change it.\n\
         pub(crate) const ANDROID_TARGET_SDK: u32 = {};\n",
        floors.windows.build, floors.android.api, floors.android.target_sdk
    )
}

fn version_map(floors: &Floors) -> String {
    let mut page = String::from(
        "# Version map\n\
         \n\
         > Generated by flui-cli's `platform_floor` test from\n\
         > [`platform-support.toml`](../platform-support.toml); edit that file, not this page.\n\
         \n\
         The lowest version of each environment FLUI runs on, and what sets it\n\
         ([ADR-0244](adr/ADR-0244-os-version-floors-and-gates.md)). The backends refuse to start below\n\
         them, and `flui build` packages them; an application may raise one in `flui.toml`\n\
         (`[platforms.<environment>] min`), never lower it. The ADR puts an API newer than a floor\n\
         behind a typed gate rather than raising the floor.\n\
         \n\
         | Environment | Floor | Set by |\n\
         |---|---|---|\n",
    );
    let rows = [
        (
            "Windows",
            format!(
                "{} (build {})",
                floors.windows.release, floors.windows.build
            ),
            &floors.windows.set_by,
        ),
        (
            "macOS",
            format!(
                "{} on x86_64, {} on arm64",
                floors.macos.x86_64, floors.macos.aarch64
            ),
            &floors.macos.set_by,
        ),
        ("iOS", floors.ios.version.clone(), &floors.ios.set_by),
        (
            "Android",
            format!("API {} ({})", floors.android.api, floors.android.release),
            &floors.android.set_by,
        ),
        (
            "Linux",
            format!(
                "glibc {}; {}",
                floors.linux.glibc,
                floors.linux.requires.join("; ")
            ),
            &floors.linux.set_by,
        ),
        ("Web", floors.web.requires.join("; "), &floors.web.set_by),
    ];
    for (environment, floor, set_by) in rows {
        writeln!(page, "| {environment} | {floor} | {set_by} |").expect("BUG: writing to a String");
    }
    page
}

/// `text` with every `flui.minSdk = N` and `flui.targetSdk = N` line stating the value
/// `platform-support.toml` gives it, its indentation and line endings kept.
fn restate_gradle_properties(text: &str, floors: &Floors) -> String {
    let properties = gradle_properties(floors);
    text.split_inclusive('\n')
        .map(|line| {
            let (body, ending) = match line.strip_suffix("\r\n") {
                Some(body) => (body, "\r\n"),
                None => match line.strip_suffix('\n') {
                    Some(body) => (body, "\n"),
                    None => (line, ""),
                },
            };
            let stated = sdk_line(body).and_then(|sdk| {
                let (_, _, value) = properties.iter().find(|(key, _, _)| *key == sdk.key)?;
                Some(format!("{}{value}{}{ending}", sdk.prefix, sdk.suffix))
            });
            stated.unwrap_or_else(|| line.to_owned())
        })
        .collect()
}

/// Every disagreement between the root file and what states a floor elsewhere.
///
/// `committed` is each generated path with its checked-in contents (empty when missing);
/// `templates` each template path with its contents.
fn check(
    floors: &Floors,
    committed: &[(&str, String)],
    templates: &[(&str, String)],
) -> Vec<String> {
    let mut findings = Vec::new();
    for (path, expected) in generate(floors) {
        match committed.iter().find(|(committed, _)| *committed == path) {
            Some((_, actual)) if actual.replace("\r\n", "\n") == expected => {}
            Some((_, actual)) if actual.is_empty() => {
                findings.push(format!("{path} is missing"));
            }
            _ => findings.push(format!("{path} differs from what {SOURCE} generates")),
        }
    }
    for (path, text) in templates {
        for (key, field, expected) in gradle_properties(floors) {
            let stated: Vec<u32> = text
                .lines()
                .filter_map(sdk_line)
                .filter(|sdk| sdk.key == key)
                .map(|sdk| sdk.value)
                .collect();
            if stated.is_empty() {
                findings.push(format!("{path} states no `{key} = N`"));
            }
            for value in stated {
                if value != expected {
                    findings.push(format!(
                        "{path} states {key} = {value}, but {SOURCE} has {field} = {expected}"
                    ));
                }
            }
        }
    }
    findings
}

/// The committed generated files and the floors of `platform-support.toml` agree; with
/// `FLUI_BLESS=1` the files are regenerated and the template floors restated instead.
#[test]
fn the_checked_in_files_agree() {
    let floors = Floors::parse(&read(SOURCE)).expect("BUG: platform-support.toml is malformed");
    let root = repo_root();
    if std::env::var("FLUI_BLESS").is_ok_and(|value| value == "1") {
        for (path, contents) in generate(&floors) {
            std::fs::write(root.join(path), contents)
                .unwrap_or_else(|error| panic!("writing {path}: {error}"));
        }
        let template = read(GRADLE_TEMPLATE);
        let restated = restate_gradle_properties(&template, &floors);
        if restated != template {
            std::fs::write(root.join(GRADLE_TEMPLATE), restated)
                .unwrap_or_else(|error| panic!("writing {GRADLE_TEMPLATE}: {error}"));
        }
        return;
    }
    let committed: Vec<_> = generate(&floors)
        .into_iter()
        .map(|(path, _)| {
            (
                path,
                std::fs::read_to_string(root.join(path)).unwrap_or_default(),
            )
        })
        .collect();
    let templates = [(GRADLE_TEMPLATE, read(GRADLE_TEMPLATE))];
    let findings = check(&floors, &committed, &templates);
    assert!(
        findings.is_empty(),
        "{}\nrun `{BLESS_COMMAND}` after editing {SOURCE}",
        findings.join("\n")
    );
}

mod checker {
    use super::*;

    fn root_text() -> String {
        read(SOURCE)
    }

    fn floors() -> Floors {
        Floors::parse(&root_text()).expect("the root file parses")
    }

    fn agreeing() -> (Files, Files) {
        let floors = floors();
        let committed = generate(&floors);
        let gradle = format!(
            "android.useAndroidX=true\nflui.minSdk={}\nflui.targetSdk={}\n",
            floors.android.api, floors.android.target_sdk
        );
        (committed, vec![(GRADLE_TEMPLATE, gradle)])
    }

    #[test]
    fn agreeing_sources_report_nothing() {
        let (committed, templates) = agreeing();
        assert_eq!(
            check(&floors(), &committed, &templates),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_crlf_checkout_still_agrees() {
        let (mut committed, templates) = agreeing();
        for (_, text) in &mut committed {
            *text = text.replace('\n', "\r\n");
        }
        assert_eq!(
            check(&floors(), &committed, &templates),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_planted_drift_in_each_generated_file_is_reported() {
        // Each file's plant changes a floor it states: the Android API level, or the
        // arm64 macOS floor in the file that states the Apple floors alone.
        for (path, floor, drifted) in [
            (PLATFORM_OUTPUT, "23", "24"),
            (CLI_OUTPUT, "23", "24"),
            (SYSTEM_OUTPUT, "(11, 0, 0)", "(11, 1, 0)"),
            (MAP_OUTPUT, "23", "24"),
        ] {
            let (mut committed, templates) = agreeing();
            let (_, text) = committed
                .iter_mut()
                .find(|(committed, _)| *committed == path)
                .expect("generated");
            let planted = text.replacen(floor, drifted, 1);
            assert_ne!(planted, *text, "{path}: the plant changed nothing");
            *text = planted;
            let findings = check(&floors(), &committed, &templates);
            assert_eq!(
                findings,
                [format!("{path} differs from what {SOURCE} generates")],
                "{path}"
            );
        }
    }

    #[test]
    fn a_missing_generated_file_is_reported() {
        let (mut committed, templates) = agreeing();
        committed.retain(|(path, _)| *path != CLI_OUTPUT);
        committed.push((CLI_OUTPUT, String::new()));
        assert_eq!(
            check(&floors(), &committed, &templates),
            [format!("{CLI_OUTPUT} is missing")]
        );
    }

    #[test]
    fn a_template_floor_that_differs_is_reported() {
        let (committed, _) = agreeing();
        let floors = floors();
        let (api, target) = (floors.android.api, floors.android.target_sdk);
        let drifted = [(
            GRADLE_TEMPLATE,
            "flui.minSdk=21\nflui.targetSdk=34\n".to_owned(),
        )];
        assert_eq!(
            check(&floors, &committed, &drifted),
            [
                format!(
                    "{GRADLE_TEMPLATE} states flui.minSdk = 21, but {SOURCE} has android.api = {api}"
                ),
                format!(
                    "{GRADLE_TEMPLATE} states flui.targetSdk = 34, but {SOURCE} has \
                     android.target-sdk = {target}"
                ),
            ]
        );
        let silent = [(GRADLE_TEMPLATE, "android.useAndroidX=true\n".to_owned())];
        assert_eq!(
            check(&floors, &committed, &silent),
            [
                format!("{GRADLE_TEMPLATE} states no `flui.minSdk = N`"),
                format!("{GRADLE_TEMPLATE} states no `flui.targetSdk = N`"),
            ]
        );
    }

    #[test]
    fn bless_restates_a_template_floor_that_differs() {
        let (committed, _) = agreeing();
        let floors = floors();
        let drifted = "android.useAndroidX=true\r\nflui.minSdk=21\r\nflui.targetSdk = 34\r\n\
                       org.gradle.caching=true";
        let restated = restate_gradle_properties(drifted, &floors);
        let (api, target) = (floors.android.api, floors.android.target_sdk);
        assert_eq!(
            restated,
            format!(
                "android.useAndroidX=true\r\nflui.minSdk={api}\r\nflui.targetSdk = {target}\r\n\
                 org.gradle.caching=true"
            )
        );
        assert_eq!(
            check(&floors, &committed, &[(GRADLE_TEMPLATE, restated)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_malformed_root_file_is_refused() {
        let root = root_text();
        for (plant, needle) in [
            (root.replace("version = \"13.0\"", "version = \"13\""), "ios.version"),
            (root.replace("glibc = \"2.17\"", "glibc = \"2.x\""), "linux.glibc"),
            (root.replace("[web]", "[web]\nmin = 1"), "unknown field"),
            (
                root.replace("target-sdk = 36", "target-sdk = 22"),
                "android.target-sdk = 22 is below android.api = 23",
            ),
            (
                root.replace(
                    "set-by = \"UIScene and UIWindowScene, through which the backend gets its window\"",
                    "set-by = \" \"",
                ),
                "ios.set-by is empty",
            ),
        ] {
            assert_ne!(plant, root, "{needle}: the plant changed nothing");
            let error = Floors::parse(&plant).expect_err(needle);
            assert!(error.contains(needle), "{needle}: {error}");
        }
    }
}
