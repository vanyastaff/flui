//! The OS version floors, declared once in `platform-support.toml` (ADR-0244).
//!
//! `flui-platform` and `flui-cli` are published crates whose package archives cannot read a
//! repository-root file, so this command generates a committed Rust source in each holding the
//! floors that crate compiles in, and the version map under `docs/`. Without arguments it checks
//! that every generated file is what the root file generates and that every template stating a
//! floor states the same one; `--write` regenerates the files.

use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::{Context, bail, ensure};
use regex::Regex;
use serde::Deserialize;

use crate::util::{read, repo_root};

/// The hand-written source, relative to the repository root.
pub(crate) const SOURCE: &str = "platform-support.toml";

/// The Rust source holding the floors `flui-platform` compiles in.
const PLATFORM_OUTPUT: &str = "crates/flui-platform/src/shared/platform_floor.rs";
/// The Rust source holding the floors `flui-cli` packages with.
const CLI_OUTPUT: &str = "crates/flui-cli/src/build/platform_floor.rs";
/// The version map.
const MAP_OUTPUT: &str = "docs/version-map.md";
/// The Gradle template `flui platform add android` scaffolds.
const GRADLE_TEMPLATE: &str = "crates/flui-cli/templates/platforms/android/build.gradle.kts";

static GRADLE_MIN_SDK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*minSdk\s*=\s*([0-9]+)\s*$").expect("BUG: static regex"));

/// Arguments for `cargo xtask platform-floor`.
#[derive(Debug, clap::Args)]
pub(crate) struct PlatformFloorArgs {
    /// Regenerate the floor sources and the version map instead of checking them.
    #[arg(long)]
    write: bool,
}

/// `cargo xtask platform-floor`: check (or `--write`) what `platform-support.toml` generates.
pub(crate) fn platform_floor(args: &PlatformFloorArgs) -> anyhow::Result<ExitCode> {
    let floors = Floors::parse(&read(SOURCE)?)?;
    let generated = generate(&floors);
    if args.write {
        let root = repo_root();
        for (path, contents) in &generated {
            std::fs::write(root.join(path), contents).with_context(|| format!("writing {path}"))?;
        }
        println!(
            "platform-floor: wrote {} file(s) from {SOURCE}",
            generated.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let mut committed = Vec::new();
    for (path, _) in &generated {
        committed.push((*path, read(path).unwrap_or_default()));
    }
    let templates = [(GRADLE_TEMPLATE, read(GRADLE_TEMPLATE)?)];
    let findings = check(&floors, &committed, &templates);
    if findings.is_empty() {
        println!(
            "platform-floor: {} generated file(s) and {} template(s) agree with {SOURCE}",
            generated.len(),
            templates.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for finding in &findings {
        eprintln!("platform-floor: {finding}");
    }
    eprintln!("platform-floor: run `cargo xtask platform-floor --write` after editing {SOURCE}");
    Ok(ExitCode::FAILURE)
}

/// The Android floor, for checks that need it outside the generated files (`doctor`).
pub(crate) fn android_api() -> anyhow::Result<u32> {
    Ok(Floors::parse(&read(SOURCE)?)?.android.api)
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
    fn parse(text: &str) -> anyhow::Result<Self> {
        let floors: Self = toml::from_str(text).with_context(|| format!("parsing {SOURCE}"))?;
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
                bail!("{SOURCE}: {field} = \"{version}\" is not a dotted version like \"13.0\"");
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
            ensure!(!text.trim().is_empty(), "{SOURCE}: {field} is empty");
        }
        ensure!(
            !floors.linux.requires.is_empty() && !floors.web.requires.is_empty(),
            "{SOURCE}: linux.requires and web.requires each name at least one requirement"
        );
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
        (MAP_OUTPUT, version_map(floors)),
    ]
}

const GENERATED_HEADER: &str = "// Generated by `cargo xtask platform-floor --write` from `platform-support.toml`.\n\
     // Edit that file, not this one.\n";

fn platform_source(floors: &Floors) -> String {
    format!(
        "{GENERATED_HEADER}\n\
         //! The floors this crate compiles in (ADR-0244).\n\
         \n\
         /// The lowest Android API level FLUI supports.\n\
         #[cfg(target_os = \"android\")]\n\
         pub(crate) const ANDROID_API: u32 = {};\n",
        floors.android.api
    )
}

fn cli_source(floors: &Floors) -> String {
    format!(
        "{GENERATED_HEADER}\n\
         //! The floors applications are packaged with (ADR-0244).\n\
         \n\
         /// The Android `minSdk`: the lowest API level FLUI supports.\n\
         pub(crate) const ANDROID_MIN_SDK: u32 = {};\n",
        floors.android.api
    )
}

fn version_map(floors: &Floors) -> String {
    let mut page = String::from(
        "# Version map\n\
         \n\
         > Generated by `cargo xtask platform-floor --write` from\n\
         > [`platform-support.toml`](../platform-support.toml); edit that file, not this page.\n\
         \n\
         The lowest version of each environment FLUI runs on, and what sets it\n\
         ([ADR-0244](adr/ADR-0244-os-version-floors-and-gates.md)). Below a version floor FLUI does\n\
         not start; where a floor is a requirement rather than a version, a missing one is reported\n\
         by name at start. An API newer than the floor is used behind a typed gate, never by raising\n\
         the floor.\n\
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
        let stated: Vec<u32> = text
            .lines()
            .filter_map(|line| GRADLE_MIN_SDK.captures(line))
            .filter_map(|caps| caps[1].parse().ok())
            .collect();
        match stated.as_slice() {
            [] => findings.push(format!("{path} states no `minSdk = N`")),
            values => {
                for value in values {
                    if *value != floors.android.api {
                        findings.push(format!(
                            "{path} states minSdk = {value}, but {SOURCE} has android.api = {}",
                            floors.android.api
                        ));
                    }
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = include_str!("../../../platform-support.toml");

    fn floors() -> Floors {
        Floors::parse(ROOT).expect("the root file parses")
    }

    fn agreeing() -> (Files, Files) {
        let floors = floors();
        let committed = generate(&floors);
        let gradle = format!(
            "android {{\n    defaultConfig {{\n        minSdk = {}\n    }}\n}}\n",
            floors.android.api
        );
        (committed, vec![(GRADLE_TEMPLATE, gradle)])
    }

    fn the_checked_in_files_agree() {
        let floors = floors();
        let root = repo_root();
        let committed: Vec<_> = generate(&floors)
            .into_iter()
            .map(|(path, _)| {
                (
                    path,
                    std::fs::read_to_string(root.join(path)).unwrap_or_default(),
                )
            })
            .collect();
        let gradle = std::fs::read_to_string(root.join(GRADLE_TEMPLATE)).expect("template");
        let findings = check(&floors, &committed, &[(GRADLE_TEMPLATE, gradle)]);
        assert!(findings.is_empty(), "{findings:?}");
    }

    fn agreeing_sources_report_nothing() {
        let (committed, templates) = agreeing();
        assert_eq!(
            check(&floors(), &committed, &templates),
            Vec::<String>::new()
        );
    }

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

    fn a_planted_drift_in_each_generated_file_is_reported() {
        for path in [PLATFORM_OUTPUT, CLI_OUTPUT, MAP_OUTPUT] {
            let (mut committed, templates) = agreeing();
            let (_, text) = committed
                .iter_mut()
                .find(|(committed, _)| *committed == path)
                .expect("generated");
            let planted = text.replacen("23", "24", 1);
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

    fn a_missing_generated_file_is_reported() {
        let (mut committed, templates) = agreeing();
        committed.retain(|(path, _)| *path != CLI_OUTPUT);
        committed.push((CLI_OUTPUT, String::new()));
        assert_eq!(
            check(&floors(), &committed, &templates),
            [format!("{CLI_OUTPUT} is missing")]
        );
    }

    fn a_template_floor_that_differs_is_reported() {
        let (committed, _) = agreeing();
        let drifted = [(GRADLE_TEMPLATE, "        minSdk = 21\n".to_owned())];
        assert_eq!(
            check(&floors(), &committed, &drifted),
            [format!(
                "{GRADLE_TEMPLATE} states minSdk = 21, but {SOURCE} has android.api = 23"
            )]
        );
        let silent = [(GRADLE_TEMPLATE, "android {}\n".to_owned())];
        assert_eq!(
            check(&floors(), &committed, &silent),
            [format!("{GRADLE_TEMPLATE} states no `minSdk = N`")]
        );
    }

    fn a_malformed_root_file_is_refused() {
        for (plant, needle) in [
            (ROOT.replace("version = \"13.0\"", "version = \"13\""), "ios.version"),
            (ROOT.replace("glibc = \"2.17\"", "glibc = \"2.x\""), "linux.glibc"),
            (ROOT.replace("[web]", "[web]\nmin = 1"), "unknown field"),
            (
                ROOT.replace(
                    "set-by = \"UIScene and UIWindowScene, through which the backend gets its window\"",
                    "set-by = \" \"",
                ),
                "ios.set-by is empty",
            ),
        ] {
            let error = Floors::parse(&plant).expect_err(needle);
            assert!(format!("{error:#}").contains(needle), "{needle}: {error:#}");
        }
    }

    #[test]
    fn platform_floor_sources_agree() {
        crate::table_test::run_table(
            "platform_floor_sources_agree",
            &[
                (
                    "the_checked_in_files_agree",
                    the_checked_in_files_agree as fn(),
                ),
                (
                    "agreeing_sources_report_nothing",
                    agreeing_sources_report_nothing as fn(),
                ),
                (
                    "a_crlf_checkout_still_agrees",
                    a_crlf_checkout_still_agrees as fn(),
                ),
                (
                    "a_planted_drift_in_each_generated_file_is_reported",
                    a_planted_drift_in_each_generated_file_is_reported as fn(),
                ),
                (
                    "a_missing_generated_file_is_reported",
                    a_missing_generated_file_is_reported as fn(),
                ),
                (
                    "a_template_floor_that_differs_is_reported",
                    a_template_floor_that_differs_is_reported as fn(),
                ),
                (
                    "a_malformed_root_file_is_refused",
                    a_malformed_root_file_is_refused as fn(),
                ),
            ],
        );
    }
}
