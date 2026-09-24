//! `cargo xtask workspace`: the shape of the workspace.
//!
//! - **Layers.** Each crate under `crates/` and the facade declare
//!   `[package.metadata.flui] layer`; the names live in the root manifest's
//!   `[workspace.metadata.flui] layers`. A normal or build dependency on another
//!   workspace package points to the same layer or lower, never higher, and never
//!   at an example or tool. Cargo rejects cycles itself; this adds direction
//!   (ADR-0041). Dev-dependencies may point up (tests use `flui-testing`).
//! - **Allowed dependents.** A crate may list `allowed-dependents`, the complete
//!   set of crates allowed a normal or build dependency on it, and
//!   `allowed-dev-dependents`, the same for dev-dependencies: `flui-log`, which
//!   only composition roots link, and the design systems, which nothing else
//!   depends on in any form (ADR-0028). Examples and tools are applications and
//!   may depend on anything.
//! - **wasm32.** `wasm = false` marks a package that cannot build for wasm32;
//!   wasm-check and the fast lane leave it out. Any other key in
//!   `[package.metadata.flui]`, or a mistyped value, is an error rather than a
//!   silently ignored setting.
//! - **Manifests.** Crates inherit the shared `[workspace.package]` keys and the
//!   workspace lints; examples and tools are `publish = false`.
//! - **Unreachable tests.** Under `autotests = false` a new `tests/*.rs` file is
//!   silently never compiled unless a `[[test]]` target declares or mounts it;
//!   that went unnoticed for eleven days across five crates once.
//! - **ADR numbers** are unique, because code and docs cite them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::{Context, bail};
use cargo_metadata::{DependencyKind, Metadata, Package};
use regex::Regex;
use serde_json::Value as Json;
use toml::{Table, Value as Toml};

use crate::util;

/// Arguments for `cargo xtask workspace`.
#[derive(Debug, clap::Args)]
pub(crate) struct WorkspaceArgs {}

/// `cargo xtask workspace`: check layers, manifests, test reachability and ADR
/// numbers. Exit code 1 lists every finding.
pub(crate) fn workspace(_args: &WorkspaceArgs) -> anyhow::Result<ExitCode> {
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let (findings, summary) = check(&root, &metadata)?;
    if findings.is_empty() {
        println!("workspace: {summary}");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("workspace: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// Runs every check; returns the findings and, when there are none, a summary.
fn check(root: &Path, metadata: &Metadata) -> anyhow::Result<(Vec<String>, String)> {
    let members = Members::load(root, metadata)?;
    let mut findings = Vec::new();
    let edges = check_layers(&members, &metadata.workspace_metadata, &mut findings)?;
    check_manifests(root, &members, &mut findings)?;
    check_unique_adr_numbers(root, &mut findings)?;
    let layered = members
        .iter()
        .filter(|member| member.layer.is_some())
        .count();
    Ok((
        findings,
        format!("{layered} layered crates, {edges} dependency edges checked"),
    ))
}

/// The keys a member's `[package.metadata.flui]` may set.
const FLUI_KEYS: [&str; 4] = [
    "layer",
    "allowed-dependents",
    "allowed-dev-dependents",
    "wasm",
];

/// A workspace member, with what the checks read from it.
struct Member<'m> {
    package: &'m Package,
    /// Manifest path relative to the repository root, `/`-separated.
    rel: String,
    /// `[package.metadata.flui] layer`, when declared.
    layer: Option<usize>,
    /// `[package.metadata.flui] allowed-dependents`: the crates allowed a
    /// normal or build dependency on this one, when declared.
    allowed_dependents: Option<BTreeSet<String>>,
    /// `[package.metadata.flui] allowed-dev-dependents`: the crates allowed a
    /// dev-dependency on this one, when declared.
    allowed_dev_dependents: Option<BTreeSet<String>>,
}

impl Member<'_> {
    fn name(&self) -> &str {
        self.package.name.as_ref()
    }

    /// Crates under `crates/` and the root facade carry a layer.
    fn must_have_layer(&self) -> bool {
        self.rel == "Cargo.toml" || self.rel.starts_with("crates/")
    }

    fn is_example_or_tool(&self) -> bool {
        self.rel.starts_with("examples/") || self.rel.starts_with("tools/")
    }
}

struct Members<'m>(Vec<Member<'m>>);

impl<'m> Members<'m> {
    fn load(root: &Path, metadata: &'m Metadata) -> anyhow::Result<Self> {
        let mut members = Vec::new();
        for package in metadata.workspace_packages() {
            let rel = relative(root, package.manifest_path.as_std_path())?;
            let flui = &package.metadata["flui"];
            if let Some(key) = flui
                .as_object()
                .and_then(|table| table.keys().find(|key| !FLUI_KEYS.contains(&key.as_str())))
            {
                bail!(
                    "{rel}: unknown `[package.metadata.flui]` key `{key}` (known: {})",
                    FLUI_KEYS.join(", ")
                );
            }
            if !(flui["wasm"].is_null() || flui["wasm"].is_boolean()) {
                bail!("{rel}: `wasm` must be `true` or `false`");
            }
            let layer = match &flui["layer"] {
                Json::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .and_then(|layer| usize::try_from(layer).ok())
                        .with_context(|| {
                            format!("{rel}: `layer` must be a non-negative integer")
                        })?,
                ),
            };
            let allowed_dependents = package_names(flui, "allowed-dependents", &rel)?;
            let allowed_dev_dependents = package_names(flui, "allowed-dev-dependents", &rel)?;
            members.push(Member {
                package,
                rel,
                layer,
                allowed_dependents,
                allowed_dev_dependents,
            });
        }
        members.sort_by(|a, b| a.rel.cmp(&b.rel));
        Ok(Self(members))
    }

    fn iter(&self) -> impl Iterator<Item = &Member<'m>> {
        self.0.iter()
    }

    fn by_name(&self) -> HashMap<&str, &Member<'m>> {
        self.0
            .iter()
            .map(|member| (member.name(), member))
            .collect()
    }
}

/// The list of package names under `key`, when the manifest declares one.
fn package_names(flui: &Json, key: &str, rel: &str) -> anyhow::Result<Option<BTreeSet<String>>> {
    match &flui[key] {
        Json::Null => Ok(None),
        value => value
            .as_array()
            .and_then(|names| {
                names
                    .iter()
                    .map(|name| name.as_str().map(str::to_owned))
                    .collect::<Option<BTreeSet<_>>>()
            })
            .map(Some)
            .with_context(|| format!("{rel}: `{key}` must be a list of package names")),
    }
}

/// Layer direction and allowed dependents. Returns the number of in-workspace
/// edges examined.
fn check_layers(
    members: &Members<'_>,
    workspace_metadata: &Json,
    findings: &mut Vec<String>,
) -> anyhow::Result<usize> {
    let names: Vec<&str> = workspace_metadata["flui"]["layers"]
        .as_array()
        .context("Cargo.toml needs `[workspace.metadata.flui] layers = [...]`")?
        .iter()
        .map(|name| name.as_str().context("layer names must be strings"))
        .collect::<anyhow::Result<_>>()?;
    let describe = |layer: usize| {
        names.get(layer).map_or_else(
            || format!("layer {layer}"),
            |name| format!("layer {layer} ({name})"),
        )
    };

    for member in members.iter() {
        match member.layer {
            None if member.must_have_layer() => findings.push(format!(
                "{} has no `[package.metadata.flui] layer`; every crate under crates/ declares \
                 one",
                member.rel
            )),
            Some(layer) if layer >= names.len() => findings.push(format!(
                "{} declares layer {layer}, but the root manifest names only {} layers",
                member.rel,
                names.len()
            )),
            _ => {}
        }
    }

    let by_name = members.by_name();
    let mut edges = 0;
    for member in members.iter() {
        for dependency in &member.package.dependencies {
            // Only in-repository edges are architecture; a registry crate that
            // happens to share a name is not a member.
            if dependency.path.is_none() {
                continue;
            }
            let Some(target) = by_name.get(dependency.name.as_str()) else {
                continue;
            };
            edges += 1;

            let dev = dependency.kind == DependencyKind::Development;
            let (allowed, key, edge) = if dev {
                (
                    &target.allowed_dev_dependents,
                    "allowed-dev-dependents",
                    "has a dev-dependency on",
                )
            } else {
                (
                    &target.allowed_dependents,
                    "allowed-dependents",
                    "depends on",
                )
            };
            if let Some(allowed) = allowed
                && !member.is_example_or_tool()
                && !allowed.contains(member.name())
            {
                findings.push(format!(
                    "{} {edge} {}, which allows only {} (its `{key}`)",
                    member.name(),
                    target.name(),
                    allowed.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if dev {
                continue;
            }

            let Some(from) = member.layer else { continue };
            match target.layer {
                Some(to) if to > from => findings.push(format!(
                    "{} ({}) depends on {} ({}): dependencies point to the same layer or lower",
                    member.name(),
                    describe(from),
                    target.name(),
                    describe(to)
                )),
                None if target.is_example_or_tool() => findings.push(format!(
                    "{} depends on {}, an example or tool; nothing layered depends on those",
                    member.name(),
                    target.name()
                )),
                _ => {}
            }
        }
    }
    Ok(edges)
}

/// The `[workspace.package]` keys every crate inherits; examples and tools
/// inherit the first four.
const INHERITED: [&str; 6] = [
    "version",
    "edition",
    "rust-version",
    "license",
    "authors",
    "repository",
];

/// Inherited package metadata, workspace lints, private examples and tools, and
/// test reachability under `autotests = false`.
fn check_manifests(
    root: &Path,
    members: &Members<'_>,
    findings: &mut Vec<String>,
) -> anyhow::Result<()> {
    for member in members.iter() {
        // The root manifest is the workspace itself; its package inherits
        // nothing from a table it defines.
        if member.rel == "Cargo.toml" {
            continue;
        }
        let manifest_path = root.join(&member.rel);
        let text = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", member.rel))?;
        let manifest: Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", member.rel))?;
        let package = manifest
            .get("package")
            .and_then(Toml::as_table)
            .with_context(|| format!("{} has no [package] table", member.rel))?;

        let inherited = if member.is_example_or_tool() {
            if package.get("publish") != Some(&Toml::Boolean(false)) {
                findings.push(format!("{} must set `publish = false`", member.rel));
            }
            &INHERITED[..4]
        } else {
            &INHERITED[..]
        };
        for key in inherited {
            if !inherits_workspace(package.get(*key)) {
                findings.push(format!(
                    "{} must inherit `{key}.workspace = true`",
                    member.rel
                ));
            }
        }
        // A missing `[lints]` table opts the crate out of the workspace lints
        // silently, and Cargo forbids mixing `workspace = true` with local keys.
        if !inherits_workspace(manifest.get("lints")) {
            findings.push(format!(
                "{} must set `[lints] workspace = true`",
                member.rel
            ));
        }

        if package.get("autotests") == Some(&Toml::Boolean(false)) {
            let crate_dir = manifest_path
                .parent()
                .expect("BUG: a manifest path has a parent directory");
            check_test_reachability(root, member.name(), crate_dir, &manifest, findings)?;
        }
    }
    Ok(())
}

fn inherits_workspace(value: Option<&Toml>) -> bool {
    value.and_then(Toml::as_table).is_some_and(|table| {
        table.len() == 1 && table.get("workspace") == Some(&Toml::Boolean(true))
    })
}

static PATH_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"#\[path\s*=\s*"(?P<path>[^"]+)"\]\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;"#)
        .expect("BUG: static regex is valid")
});

static BARE_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;").expect("BUG: static regex is valid")
});

/// Every top-level `tests/*.rs` file is its own `[[test]]` target or is mounted
/// (`mod` / `#[path] mod`) from one. A `tests/main.rs` that no `[[test]]`
/// declares is itself a finding: Cargo never compiles it, so trusting its `mod`
/// lines would call its siblings reachable while none of them run.
fn check_test_reachability(
    root: &Path,
    name: &str,
    crate_dir: &Path,
    manifest: &Table,
    findings: &mut Vec<String>,
) -> anyhow::Result<()> {
    let tests_dir = crate_dir.join("tests");
    if !tests_dir.is_dir() {
        return Ok(());
    }
    let declared: BTreeSet<PathBuf> = manifest
        .get("test")
        .and_then(Toml::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("path").and_then(Toml::as_str))
        .map(|path| normalize(&crate_dir.join(path)))
        .collect();

    let main_rs = normalize(&tests_dir.join("main.rs"));
    if main_rs.is_file() && !declared.contains(&main_rs) {
        findings.push(format!(
            "{} is not a declared `[[test]]` target: `{name}` sets `autotests = false`, so it \
             never compiles and nothing it mounts runs",
            relative(root, &main_rs)?
        ));
    }

    let mut reachable = BTreeSet::new();
    for source in declared.iter().filter(|path| path.is_file()) {
        let text = std::fs::read_to_string(source)
            .with_context(|| format!("reading {}", source.display()))?;
        let dir = source
            .parent()
            .expect("BUG: a file path has a parent directory");
        let mut pathed = BTreeSet::new();
        for found in PATH_MOD.captures_iter(&text) {
            pathed.insert(found["name"].to_owned());
            reachable.insert(normalize(&dir.join(&found["path"])));
        }
        // A bare `mod foo;` resolves next to the mounting file, which is
        // `tests/` only when that file lives there.
        if normalize(dir) == normalize(&tests_dir) {
            for found in BARE_MOD.captures_iter(&text) {
                if !pathed.contains(&found["name"]) {
                    reachable.insert(normalize(&tests_dir.join(format!("{}.rs", &found["name"]))));
                }
            }
        }
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&tests_dir)
        .with_context(|| format!("listing {}", tests_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
                && path.file_name().is_some_and(|file| file != "main.rs")
        })
        .map(|path| normalize(&path))
        .collect();
    entries.sort();
    for file in entries {
        if !declared.contains(&file) && !reachable.contains(&file) {
            findings.push(format!(
                "{} never runs: `{name}` sets `autotests = false`; give it a `[[test]]` target \
                 or mount it from one",
                relative(root, &file)?
            ));
        }
    }
    Ok(())
}

/// Two ADR files may not share a number.
fn check_unique_adr_numbers(root: &Path, findings: &mut Vec<String>) -> anyhow::Result<()> {
    let dir = root.join("docs").join("adr");
    let mut by_number: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
        let file = entry?.file_name().to_string_lossy().into_owned();
        if let Some(number) = file
            .strip_prefix("ADR-")
            .and_then(|rest| rest.get(..4))
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
        {
            by_number.entry(number.to_owned()).or_default().push(file);
        }
    }
    for (number, mut files) in by_number {
        if files.len() > 1 {
            files.sort();
            findings.push(format!("ADR-{number} is used by {}", files.join(", ")));
        }
    }
    Ok(())
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> anyhow::Result<String> {
    let path = normalize(path);
    let rel = path
        .strip_prefix(normalize(root))
        .with_context(|| format!("{} is outside the repository", path.display()))?;
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Lexically resolves `.` and `..`, so paths from manifests and `#[path]`
/// attributes compare equal to directory listings on every host.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests;
