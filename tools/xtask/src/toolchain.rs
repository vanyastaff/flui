//! Every declared MSRV agrees with rust-toolchain.toml.
//!
//! `rust-toolchain.toml`'s channel minor is the single source of truth under
//! the pre-1.0 policy (MSRV tracks latest stable; see AGENTS.md). Every other
//! place the workspace states an MSRV is compared against it: the workspace
//! `rust-version` (which clippy also reads), the `flui-cli` templates, the
//! README badge and `llms.txt`.

use std::path::Path;
use std::process::ExitCode;
use std::sync::LazyLock;

use regex::Regex;

use crate::util::{read, repo_root};

const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";
const TEMPLATES_DIR: &str = "crates/flui-cli/src/templates";

static CHANNEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^channel = "([0-9]+\.[0-9]+)\.[0-9]+"\s*$"#).expect("BUG: static regex")
});
static RUST_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^rust-version = "([0-9]+\.[0-9]+)""#).expect("BUG: static regex")
});
static README_BADGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"MSRV-([0-9]+\.[0-9]+)-").expect("BUG: static regex"));
static LLMS_MSRV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"MSRV ([0-9]+\.[0-9]+)\)").expect("BUG: static regex"));

/// Arguments for `cargo xtask toolchain`.
#[derive(Debug, clap::Args)]
pub(crate) struct ToolchainArgs {}

/// `cargo xtask toolchain`: check that every declared MSRV matches rust-toolchain.toml.
pub(crate) fn toolchain(_args: &ToolchainArgs) -> anyhow::Result<ExitCode> {
    let sources = Sources {
        toolchain: read(TOOLCHAIN_FILE)?,
        cargo: read("Cargo.toml")?,
        templates: read_templates(&repo_root())?,
        readme: read("README.md")?,
        llms: read("llms.txt")?,
    };
    match check(&sources) {
        Ok(report) => {
            println!(
                "toolchain: rust-toolchain.toml ({}) == Cargo.toml, {} template(s), README badge, \
                 llms.txt",
                report.channel_minor, report.template_count
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(errors) => {
            for error in errors {
                eprintln!("toolchain: {error}");
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

/// The files the check reads, already loaded.
struct Sources {
    toolchain: String,
    cargo: String,
    /// `(repo-relative path, contents)` of every template, sorted by path.
    templates: Vec<(String, String)>,
    readme: String,
    llms: String,
}

/// What a consistent tree reports.
#[derive(Debug)]
struct Report {
    channel_minor: String,
    template_count: usize,
}

/// Every `crates/flui-cli/src/templates/*.rs`, sorted; none when the directory is absent.
fn read_templates(root: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let mut templates = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join(TEMPLATES_DIR)) else {
        return Ok(templates);
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let name = path
                .file_name()
                .expect("BUG: a read_dir entry has a file name")
                .to_string_lossy();
            let rel = format!("{TEMPLATES_DIR}/{name}");
            let text = read(&rel)?;
            templates.push((rel, text));
        }
    }
    templates.sort();
    Ok(templates)
}

/// The single capture group of `re` on every line of `text` it matches.
/// `lines()` drops a trailing `\r`, so CRLF checkouts anchor the same as LF.
fn line_captures<'a>(re: &'a Regex, text: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    text.lines().filter_map(|line| first_capture(re, line))
}

/// The single capture group of the first match of `re` in `text`.
fn first_capture<'a>(re: &Regex, text: &'a str) -> Option<&'a str> {
    re.captures(text).map(|caps| {
        caps.get(1)
            .expect("BUG: every pattern has one group")
            .as_str()
    })
}

/// Compares every declaration against the channel; the errors, un-prefixed, on a mismatch.
fn check(sources: &Sources) -> Result<Report, Vec<String>> {
    let Some(channel_line) = sources
        .toolchain
        .lines()
        .find(|line| line.starts_with("channel = "))
    else {
        return Err(vec![format!(
            "could not find 'channel = \"X.Y.Z\"' in {TOOLCHAIN_FILE}"
        )]);
    };
    let Some(channel_minor) = first_capture(&CHANNEL, channel_line) else {
        return Err(vec![format!(
            "could not parse a major.minor.patch version out of: {channel_line}"
        )]);
    };

    let mut errors = Vec::new();
    let mut compare = |label: &str, actual: &str| {
        if actual != channel_minor {
            errors.push(format!(
                "{label} declares \"{actual}\", expected \"{channel_minor}\" (from \
                 {TOOLCHAIN_FILE}'s channel)"
            ));
        }
    };
    let mut missing = Vec::new();

    let cargo: Vec<_> = line_captures(&RUST_VERSION, &sources.cargo).collect();
    if cargo.is_empty() {
        missing.push("could not find 'rust-version = \"X.Y\"' in Cargo.toml".to_owned());
    }
    for version in cargo {
        compare("Cargo.toml [workspace.package].rust-version", version);
    }

    let mut template_count = 0;
    for (path, text) in &sources.templates {
        let versions: Vec<_> = line_captures(&RUST_VERSION, text).collect();
        if !versions.is_empty() {
            template_count += 1;
        }
        for version in versions {
            compare(&format!("{path} rust-version"), version);
        }
    }
    if template_count == 0 {
        missing.push(format!(
            "found no 'rust-version = \"X.Y\"' line in any {TEMPLATES_DIR}/*.rs — template glob \
             or format changed?"
        ));
    }

    // README.md's badge and llms.txt's summary are prose, but both have a
    // machine-extractable shape, so they are checked rather than left as an
    // unenforced claim.
    match first_capture(&README_BADGE, &sources.readme) {
        Some(version) => compare("README.md MSRV badge", version),
        None => missing.push("could not find the 'MSRV-X.Y-' badge shield in README.md".to_owned()),
    }
    match first_capture(&LLMS_MSRV, &sources.llms) {
        Some(version) => compare("llms.txt MSRV", version),
        None => missing.push("could not find 'MSRV X.Y)' in llms.txt".to_owned()),
    }

    errors.extend(missing);
    if errors.is_empty() {
        return Ok(Report {
            channel_minor: channel_minor.to_owned(),
            template_count,
        });
    }
    let count = errors.len();
    errors.push(format!(
        "{count} mismatch(es) against {TOOLCHAIN_FILE} channel {channel_minor}"
    ));
    Err(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consistent() -> Sources {
        Sources {
            toolchain: "[toolchain]\r\nchannel = \"1.98.1\"\r\n".to_owned(),
            cargo: "[workspace.package]\r\nrust-version = \"1.98\"\r\n".to_owned(),
            templates: vec![
                (
                    "t/a.rs".to_owned(),
                    "r#\"\nrust-version = \"1.98\"\n\"#".to_owned(),
                ),
                (
                    "t/b.rs".to_owned(),
                    "rust-version.workspace = true\n".to_owned(),
                ),
            ],
            readme: "![MSRV](https://img.shields.io/badge/MSRV-1.98-blue)".to_owned(),
            llms: "FLUI (Rust, MSRV 1.98) is".to_owned(),
        }
    }

    #[test]
    fn consistent_tree_passes_and_counts_templates() {
        let report = check(&consistent()).expect("consistent");
        assert_eq!(report.channel_minor, "1.98");
        assert_eq!(report.template_count, 1);
    }

    #[test]
    fn each_mismatch_names_its_source() {
        let mut sources = consistent();
        sources.cargo = "rust-version = \"1.90\"\n".to_owned();
        sources.templates[0].1 = "rust-version = \"1.92\"\n".to_owned();
        sources.readme = "MSRV-1.93-".to_owned();
        sources.llms = "MSRV 1.94)".to_owned();
        let errors = check(&sources).expect_err("mismatch").join("\n");
        for needle in [
            "Cargo.toml [workspace.package].rust-version declares \"1.90\", expected \"1.98\"",
            "t/a.rs rust-version declares \"1.92\"",
            "README.md MSRV badge declares \"1.93\"",
            "llms.txt MSRV declares \"1.94\"",
            "4 mismatch(es) against rust-toolchain.toml channel 1.98",
        ] {
            assert!(errors.contains(needle), "missing {needle:?} in\n{errors}");
        }
    }

    #[test]
    fn missing_declarations_are_errors() {
        let mut sources = consistent();
        sources.templates.clear();
        sources.llms = String::new();
        let errors = check(&sources).expect_err("missing").join("\n");
        assert!(errors.contains("found no 'rust-version"), "{errors}");
        assert!(
            errors.contains("could not find 'MSRV X.Y)' in llms.txt"),
            "{errors}"
        );
    }

    #[test]
    fn unparsable_channel_is_reported() {
        let mut sources = consistent();
        sources.toolchain = "channel = \"stable\"\n".to_owned();
        let errors = check(&sources).expect_err("unparsable");
        assert!(errors[0].starts_with("could not parse"), "{errors:?}");
    }
}
