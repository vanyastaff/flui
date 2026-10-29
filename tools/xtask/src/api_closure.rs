//! `cargo xtask api-closure`: what a crate's public API may name from crates
//! outside the workspace (ADR-0089).
//!
//! A package is checked when its manifest says so:
//!
//! - `[package.metadata.flui] tier-kind = "stable"`: the public API names, from
//!   outside the workspace and the standard library, only what ADR-0089 §2
//!   allows: `raw-window-handle`'s `HasWindowHandle`, `HasDisplayHandle`,
//!   `WindowHandle`, `DisplayHandle` and `HandleError`; `cursor-icon`'s
//!   `CursorIcon`; `serde` and `schemars` inside an implementation of one of
//!   their own traits (a derive). Every other upstream item is a finding.
//! - `[package.metadata.flui.api-closure] allow = ["parking_lot", …]`: the
//!   public API names, from outside the workspace and the standard library,
//!   only the listed crates. The list is what the API may name, not what it
//!   may not, so a crate nobody thought of (an OS binding a dependency
//!   brings) is a finding; the core host uses it to keep every OS binding,
//!   AccessKit and tokio out of its paths (ADR-0151 §8).
//!
//! The table's other keys, each a list that only shrinks: an entry no build
//! needs is a finding.
//!
//! - `exceptions = [{ crate = "…", exit = "ADR-NNNN" }]` lets a Stable package
//!   name one more upstream crate until the change in the ADR `exit` names
//!   removes it.
//! - `entries = [{ item = "pkg::path", crate = "…" }]` lets one item of an
//!   `allow` package name one crate the list leaves out: what a composition
//!   root hands the package from outside (the Activity an Android
//!   `android_main` is handed).
//! - `traits = [{ trait = "crate::path::Trait", crate = "…" }]` lets the
//!   methods of an `allow` package's implementations of one workspace trait
//!   name one crate the list leaves out, because the trait's own signature
//!   names it (the core host's windows implement `flui-semantics`'
//!   accessibility trait, whose methods take AccessKit's tree). The
//!   implementation's header and associated types are still checked.
//! - `targets = [{ target = "…", features = [..] | all-features = true }]`
//!   checks the package's API as each target builds it, for a package whose
//!   public items differ by target (one host per operating system); without
//!   it, the host's build with default features is checked. The toolchain
//!   needs each target's standard library. The builds run with `DOCS_RS` set,
//!   as docs.rs builds documentation: a build script that compiles C for a
//!   target it cannot reach (the Objective-C exception helper on iOS) skips
//!   it.
//!
//! The table is read strictly: an unknown key (`all_features` for
//! `all-features`) is an error, not a narrower build.
//!
//! What counts: every item of the package that rustdoc's JSON output lists
//! (public items, hidden ones included, and their implementations), and every
//! item it refers to: a type in a signature, a bound, a trait an implementation
//! implements or a public re-export. A blanket implementation and a synthetic
//! one (an auto trait) belong to the crate that wrote them and are skipped. The
//! JSON is read through its `index`, `paths` and `external_crates` tables and
//! every `"id"` an item holds, not through a typed model of one format version,
//! so a rustdoc format change that keeps those tables keeps the gate working.
//!
//! Limit: an item of another workspace crate that a signature names is not
//! followed; that crate's own rule covers it.
//!
//! Why a scan (ADR-0078 §4, condition 1): no type or lint says which crates a
//! signature may name. It reads rustdoc's JSON, `--self-test` plants
//! violations into the real output of a checked package, and its exceptions
//! are manifest data that name an ADR and only shrink.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::util;

/// Arguments for `cargo xtask api-closure`.
#[derive(Debug, clap::Args)]
pub(crate) struct ApiClosureArgs {
    /// Check only this package (repeatable). Default: every package whose
    /// manifest opts in.
    #[arg(short = 'p', long = "package", conflicts_with = "self_test")]
    packages: Vec<String>,
    /// The toolchain that writes rustdoc's JSON output (`rustup run <toolchain>`).
    #[arg(long, default_value = "nightly")]
    toolchain: String,
    /// Plant violations into the output of a checked package instead of
    /// checking the workspace; exit 1 unless the rule reports exactly those.
    #[arg(long)]
    self_test: bool,
}

/// The standard library's crates.
const STD: &[&str] = &["core", "alloc", "std", "proc_macro"];

/// The package `--self-test` plants violations into.
const SELF_TEST_PACKAGE: &str = "flui-platform";

/// The target kinds that build a library other crates link.
const LIBRARY_KINDS: &[&str] = &["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];

fn is_library(target: &cargo_metadata::Target) -> bool {
    target
        .kind
        .iter()
        .any(|kind| LIBRARY_KINDS.contains(&kind.to_string().as_str()))
}

/// What ADR-0089 §2 lets a Stable signature name from an upstream crate.
enum Allowed {
    /// These items only, by their last path segment.
    Items(&'static [&'static str]),
    /// Anything, inside an implementation of one of the crate's own traits.
    DeriveOnly,
}

const ALLOWED: &[(&str, Allowed)] = &[
    (
        "raw_window_handle",
        Allowed::Items(&[
            "HasWindowHandle",
            "HasDisplayHandle",
            "WindowHandle",
            "DisplayHandle",
            "HandleError",
        ]),
    ),
    ("cursor_icon", Allowed::Items(&["CursorIcon"])),
    ("serde", Allowed::DeriveOnly),
    ("schemars", Allowed::DeriveOnly),
];

/// What a checked package's public API may name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// ADR-0089 §2, plus the package's exceptions.
    Stable,
    /// These upstream crates, and no other.
    Allow(BTreeSet<String>),
}

/// One upstream crate a Stable package may still name, until the ADR in `exit`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Exception {
    krate: String,
    exit: String,
}

/// The one item of an `allow` package that may name one crate the list leaves
/// out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    /// The item's path, as a finding prints it (`flui_native::android_platform`).
    item: String,
    krate: String,
}

/// A workspace trait whose implementations' methods may name one crate an
/// `allow` package's list leaves out, because the trait's signature names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Admission {
    /// The trait's path (`flui_semantics::platform::PlatformAccessibility`).
    path: String,
    krate: String,
}

/// One rustdoc build of a checked package.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Build {
    /// `None`: the host.
    target: Option<String>,
    all_features: bool,
    features: Vec<String>,
}

impl Build {
    /// The host's build with default features.
    const HOST: Self = Self {
        target: None,
        all_features: false,
        features: Vec::new(),
    };

    fn label(&self) -> &str {
        self.target.as_deref().unwrap_or("host")
    }
}

/// A checked package and its rule.
#[derive(Debug, Clone)]
struct Rule {
    package: String,
    /// The library target's crate name.
    lib: String,
    mode: Mode,
    exceptions: Vec<Exception>,
    entries: Vec<Entry>,
    traits: Vec<Admission>,
    builds: Vec<Build>,
}

/// `[package.metadata.flui.api-closure]`, read strictly.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    allow: Option<BTreeSet<String>>,
    #[serde(default)]
    exceptions: Vec<ExceptionRow>,
    #[serde(default)]
    entries: Vec<EntryRow>,
    #[serde(default)]
    traits: Vec<TraitRow>,
    #[serde(default)]
    targets: Vec<BuildRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExceptionRow {
    #[serde(rename = "crate")]
    krate: String,
    exit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryRow {
    item: String,
    #[serde(rename = "crate")]
    krate: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraitRow {
    #[serde(rename = "trait")]
    path: String,
    #[serde(rename = "crate")]
    krate: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct BuildRow {
    target: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    all_features: bool,
}

/// One violation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// `item`, written at `location`, names `named` in the build `build`.
    Names {
        package: String,
        location: String,
        build: String,
        item: String,
        named: String,
    },
    /// An exception nothing needs any more.
    UnusedException {
        package: String,
        krate: String,
        exit: String,
    },
    /// An entry no build needs any more.
    UnusedEntry {
        package: String,
        item: String,
        krate: String,
    },
    /// An admitted trait no build's implementation needs any more.
    UnusedTrait {
        package: String,
        path: String,
        krate: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Names {
                package,
                location,
                build,
                item,
                named,
            } => write!(
                f,
                "{location}: {package} ({build}): `{item}` names `{named}`"
            ),
            Self::UnusedException {
                package,
                krate,
                exit,
            } => write!(
                f,
                "{package}: the exception for `{krate}` ({exit}) is no longer needed; remove it"
            ),
            Self::UnusedEntry {
                package,
                item,
                krate,
            } => write!(
                f,
                "{package}: the entry `{item}` names no `{krate}` item in any build; remove it"
            ),
            Self::UnusedTrait {
                package,
                path,
                krate,
            } => write!(
                f,
                "{package}: no implementation of `{path}` names `{krate}` in any build; remove the admission"
            ),
        }
    }
}

/// What one build of a package shows: its findings, and the exceptions,
/// entries and admitted traits it needed.
#[derive(Debug, Default)]
struct Scan {
    findings: BTreeSet<Finding>,
    used_exceptions: BTreeSet<String>,
    used_entries: BTreeSet<Entry>,
    used_traits: BTreeSet<Admission>,
}

/// `cargo xtask api-closure`. Exit code 1 lists every finding.
pub(crate) fn api_closure(args: &ApiClosureArgs) -> anyhow::Result<ExitCode> {
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let workspace = workspace_crates(&metadata);
    let mut rules = rules(&root, &metadata)?;
    if args.self_test {
        // The contract is the Stable package with the most upstream items to
        // tell apart (raw-window-handle, cursor-icon, blanket impls).
        let stable = rules
            .iter()
            .find(|rule| rule.package == SELF_TEST_PACKAGE)
            .cloned()
            .with_context(|| format!("`{SELF_TEST_PACKAGE}` is not a checked Stable package"))?;
        let doc = rustdoc_json(&root, &args.toolchain, &stable, &Build::HOST)?;
        return Ok(self_test(&doc, &stable, &workspace));
    }
    if !args.packages.is_empty() {
        for name in &args.packages {
            if !rules.iter().any(|rule| &rule.package == name) {
                bail!(
                    "api-closure: `{name}` is not a checked package (tier-kind stable or an api-closure table)"
                );
            }
        }
        rules.retain(|rule| args.packages.contains(&rule.package));
    }
    let mut all = Vec::new();
    for rule in &rules {
        let mut scans = Vec::new();
        for build in &rule.builds {
            let doc = rustdoc_json(&root, &args.toolchain, rule, build)?;
            scans.push(scan(&doc, rule, build, &workspace, &root)?);
        }
        all.extend(findings(rule, scans));
    }
    all.sort();
    for finding in &all {
        println!("{finding}");
    }
    if all.is_empty() {
        let names: Vec<&str> = rules.iter().map(|rule| rule.package.as_str()).collect();
        println!(
            "api-closure: {} package(s) clean: {}",
            rules.len(),
            names.join(", ")
        );
        Ok(ExitCode::SUCCESS)
    } else {
        println!("api-closure: {} finding(s)", all.len());
        Ok(ExitCode::FAILURE)
    }
}

/// The library crate names of every workspace package (`flui_platform`, …).
fn workspace_crates(metadata: &cargo_metadata::Metadata) -> BTreeSet<String> {
    metadata
        .workspace_packages()
        .into_iter()
        .flat_map(|package| {
            package
                .targets
                .iter()
                .filter(|target| is_library(target))
                .map(|target| target.name.replace('-', "_"))
        })
        .collect()
}

/// The checked packages, from their manifests.
fn rules(root: &Path, metadata: &cargo_metadata::Metadata) -> anyhow::Result<Vec<Rule>> {
    let adrs = util::adr_files(root);
    let mut rules = Vec::new();
    for package in metadata.workspace_packages() {
        let flui = &package.metadata["flui"];
        let stable = flui["tier-kind"].as_str() == Some("stable");
        let table: Table = match &flui["api-closure"] {
            Json::Null => Table::default(),
            table => serde_json::from_value(table.clone()).with_context(|| {
                format!(
                    "{}: `[package.metadata.flui.api-closure]` is malformed",
                    package.name
                )
            })?,
        };
        let mode = match (stable, table.allow) {
            (true, None) => Mode::Stable,
            (false, Some(allow)) => Mode::Allow(allow),
            (false, None) => {
                let empty = table.exceptions.is_empty()
                    && table.entries.is_empty()
                    && table.traits.is_empty()
                    && table.targets.is_empty();
                if empty {
                    continue;
                }
                bail!(
                    "{}: an api-closure table without `allow` checks nothing",
                    package.name
                );
            }
            (true, Some(_)) => bail!(
                "{}: a Stable package takes ADR-0089 §2's allowlist, not an `allow` list",
                package.name
            ),
        };
        let mut exceptions = Vec::new();
        for ExceptionRow { krate, exit } in table.exceptions {
            if !util::is_adr_number(&exit) || !util::adr_exists(&adrs, &exit) {
                bail!(
                    "{}: the exception for `{krate}` names `{exit}`, which is no ADR in docs/adr",
                    package.name
                );
            }
            if mode != Mode::Stable {
                bail!(
                    "{}: exceptions belong to a Stable package's allowlist",
                    package.name
                );
            }
            exceptions.push(Exception { krate, exit });
        }
        let leaves_out =
            |krate: &str| matches!(&mode, Mode::Allow(allow) if !allow.contains(krate));
        let mut entries = Vec::new();
        for EntryRow { item, krate } in table.entries {
            if !leaves_out(&krate) {
                bail!(
                    "{}: the entry `{item}` names `{krate}`, which the package's `allow` list does not leave out",
                    package.name
                );
            }
            entries.push(Entry { item, krate });
        }
        let mut traits = Vec::new();
        for TraitRow { path, krate } in table.traits {
            if !leaves_out(&krate) {
                bail!(
                    "{}: the admission of `{path}` names `{krate}`, which the package's `allow` list does not leave out",
                    package.name
                );
            }
            traits.push(Admission { path, krate });
        }
        let mut builds: Vec<Build> = table
            .targets
            .into_iter()
            .map(|row| Build {
                target: Some(row.target),
                all_features: row.all_features,
                features: row.features,
            })
            .collect();
        if builds.is_empty() {
            builds.push(Build::HOST);
        }
        let lib = package
            .targets
            .iter()
            .find(|target| is_library(target))
            .with_context(|| format!("{}: a checked package needs a library target", package.name))?
            .name
            .replace('-', "_");
        rules.push(Rule {
            package: package.name.to_string(),
            lib,
            mode,
            exceptions,
            entries,
            traits,
            builds,
        });
    }
    rules.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(rules)
}

/// Runs rustdoc's JSON output for the package's library as `build` builds it,
/// and reads it.
fn rustdoc_json(root: &Path, toolchain: &str, rule: &Rule, build: &Build) -> anyhow::Result<Json> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), PathBuf::from)
        .join("api-closure");
    println!(
        "$ rustup run {toolchain} cargo rustdoc -p {} ({}, JSON)",
        rule.package,
        build.label()
    );
    let mut command = Command::new("rustup");
    command
        .current_dir(root)
        // A documentation build, as docs.rs's: a build script that compiles C
        // for a target this host cannot reach skips it.
        .env("DOCS_RS", "1")
        .args([
            "run",
            toolchain,
            "cargo",
            "rustdoc",
            "-p",
            &rule.package,
            "--lib",
            "--locked",
        ])
        .arg("--target-dir")
        .arg(&target_dir);
    if let Some(target) = &build.target {
        command.args(["--target", target]);
    }
    if build.all_features {
        command.arg("--all-features");
    } else if !build.features.is_empty() {
        command.args(["--features", &build.features.join(",")]);
    }
    let status = command
        .args([
            "--",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--document-hidden-items",
        ])
        .status()
        .with_context(|| format!("spawning `rustup run {toolchain} cargo rustdoc`"))?;
    if !status.success() {
        bail!(
            "rustdoc failed for {} ({}); its errors are above (a missing toolchain or target \
             is one of them)",
            rule.package,
            build.label()
        );
    }
    let doc_dir = match &build.target {
        Some(target) => target_dir.join(target).join("doc"),
        None => target_dir.join("doc"),
    };
    let path = doc_dir.join(format!("{}.json", rule.lib));
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// The findings of `rule` on `doc`, a package's rustdoc JSON as its one build
/// writes it.
fn check(
    doc: &Json,
    rule: &Rule,
    workspace: &BTreeSet<String>,
    root: &Path,
) -> anyhow::Result<Vec<Finding>> {
    Ok(findings(
        rule,
        vec![scan(doc, rule, &Build::HOST, workspace, root)?],
    ))
}

/// The findings of `rule` over every build's scan: what each build names, and
/// the exceptions, entries and admitted traits no build needed.
fn findings(rule: &Rule, scans: Vec<Scan>) -> Vec<Finding> {
    let mut all = BTreeSet::new();
    let mut used_exceptions = BTreeSet::new();
    let mut used_entries = BTreeSet::new();
    let mut used_traits = BTreeSet::new();
    for scan in scans {
        all.extend(scan.findings);
        used_exceptions.extend(scan.used_exceptions);
        used_entries.extend(scan.used_entries);
        used_traits.extend(scan.used_traits);
    }
    for exception in &rule.exceptions {
        if !used_exceptions.contains(&exception.krate) {
            all.insert(Finding::UnusedException {
                package: rule.package.clone(),
                krate: exception.krate.clone(),
                exit: exception.exit.clone(),
            });
        }
    }
    for entry in &rule.entries {
        if !used_entries.contains(entry) {
            all.insert(Finding::UnusedEntry {
                package: rule.package.clone(),
                item: entry.item.clone(),
                krate: entry.krate.clone(),
            });
        }
    }
    for admission in &rule.traits {
        if !used_traits.contains(admission) {
            all.insert(Finding::UnusedTrait {
                package: rule.package.clone(),
                path: admission.path.clone(),
                krate: admission.krate.clone(),
            });
        }
    }
    all.into_iter().collect()
}

/// What `rule` finds in `doc`, a package's rustdoc JSON as `build` writes it.
fn scan(
    doc: &Json,
    rule: &Rule,
    build: &Build,
    workspace: &BTreeSet<String>,
    root: &Path,
) -> anyhow::Result<Scan> {
    let index = doc["index"]
        .as_object()
        .context("rustdoc JSON without an `index` table: the format changed")?;
    let paths = doc["paths"]
        .as_object()
        .context("rustdoc JSON without a `paths` table: the format changed")?;
    let external = doc["external_crates"]
        .as_object()
        .context("rustdoc JSON without an `external_crates` table: the format changed")?;
    let crate_of = |id: &str| -> Option<(String, String)> {
        let path = paths.get(id)?;
        let crate_id = path["crate_id"].as_u64()?;
        if crate_id == 0 {
            return None;
        }
        let name = external.get(&crate_id.to_string())?["name"]
            .as_str()?
            .to_owned();
        let segments: Vec<&str> = path["path"]
            .as_array()?
            .iter()
            .filter_map(Json::as_str)
            .collect();
        Some((name, segments.join("::")))
    };

    // The trait (its crate and path) of each implementation, for its own
    // items too.
    let mut impl_trait: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (id, item) in index {
        let Some(implementation) = item["inner"].get("impl") else {
            continue;
        };
        let Some(trait_id) = implementation["trait"].get("id").map(id_text) else {
            continue;
        };
        if let Some(trait_of) = crate_of(&trait_id) {
            impl_trait.insert(id.clone(), trait_of.clone());
            for child in implementation["items"].as_array().into_iter().flatten() {
                impl_trait.insert(id_text(child), trait_of.clone());
            }
        }
    }

    let mut scan = Scan::default();
    for (id, item) in index {
        if item["crate_id"].as_u64() != Some(0) || skipped(item) {
            continue;
        }
        // The admissions of the workspace trait whose method this item is: its
        // signature is the trait's.
        let admitted: Vec<&Admission> = match impl_trait.get(id) {
            Some((krate, path))
                if workspace.contains(krate) && item["inner"].get("function").is_some() =>
            {
                rule.traits
                    .iter()
                    .filter(|admission| &admission.path == path)
                    .collect()
            }
            _ => Vec::new(),
        };
        let mut ids = Vec::new();
        referenced(&item["inner"], &mut ids);
        for target in ids {
            let Some((krate, path)) = crate_of(&target) else {
                continue;
            };
            if STD.contains(&krate.as_str()) || workspace.contains(&krate) {
                continue;
            }
            let allowed = match &rule.mode {
                Mode::Allow(allowed) => {
                    if allowed.contains(&krate) {
                        true
                    } else if let Some(admission) =
                        admitted.iter().find(|admission| admission.krate == krate)
                    {
                        scan.used_traits.insert((*admission).clone());
                        true
                    } else {
                        let label = label(id, item, paths);
                        let entry = rule
                            .entries
                            .iter()
                            .find(|entry| entry.item == label && entry.krate == krate);
                        if let Some(entry) = entry {
                            scan.used_entries.insert(entry.clone());
                        }
                        entry.is_some()
                    }
                }
                Mode::Stable => {
                    let by_adr = ALLOWED.iter().any(|(name, allowed)| {
                        *name == krate
                            && match allowed {
                                Allowed::Items(items) => items
                                    .iter()
                                    .any(|item| path.rsplit("::").next() == Some(item)),
                                Allowed::DeriveOnly => {
                                    impl_trait.get(id).map(|(krate, _)| krate) == Some(&krate)
                                }
                            }
                    });
                    let by_exception = !by_adr
                        && rule
                            .exceptions
                            .iter()
                            .any(|exception| exception.krate == krate);
                    if by_exception {
                        scan.used_exceptions.insert(krate.clone());
                    }
                    by_adr || by_exception
                }
            };
            if !allowed {
                scan.findings.insert(Finding::Names {
                    package: rule.package.clone(),
                    location: location(item, root),
                    build: build.label().to_owned(),
                    item: label(id, item, paths),
                    named: path,
                });
            }
        }
    }
    Ok(scan)
}

/// A blanket implementation and a synthetic one belong to whoever wrote them.
fn skipped(item: &Json) -> bool {
    item["inner"].get("impl").is_some_and(|implementation| {
        !implementation["blanket_impl"].is_null()
            || implementation["is_synthetic"].as_bool() == Some(true)
    })
}

/// An `"id"` as the key `paths` and `index` use.
fn id_text(id: &Json) -> String {
    match id {
        Json::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Every `"id"` held by an object inside `value`: the items it refers to.
/// A bare list of ids (a module's children, an implementation's items) names
/// items that are checked on their own, so only ids inside objects count.
fn referenced(value: &Json, out: &mut Vec<String>) {
    match value {
        Json::Object(map) => {
            if let Some(id) = map.get("id").filter(|id| id.is_u64() || id.is_string()) {
                out.push(id_text(id));
            }
            for child in map.values() {
                referenced(child, out);
            }
        }
        Json::Array(list) => {
            for child in list {
                referenced(child, out);
            }
        }
        _ => {}
    }
}

/// Where an item is written, as `path:line` from the repository root.
fn location(item: &Json, root: &Path) -> String {
    let span = &item["span"];
    let file = span["filename"]
        .as_str()
        .unwrap_or("<unknown>")
        .replace('\\', "/");
    let root = root.to_string_lossy().replace('\\', "/");
    let file = file
        .strip_prefix(&root)
        .map_or(file.as_str(), |rest| rest.trim_start_matches('/'))
        .to_owned();
    let line = match &span["begin"] {
        Json::Array(begin) => begin.first().and_then(Json::as_u64),
        begin => begin["line"].as_u64(),
    };
    line.map_or(file.clone(), |line| format!("{file}:{line}"))
}

/// An item's path, `use <source>` for a re-export, its name, or `impl` for an
/// implementation.
fn label(id: &str, item: &Json, paths: &serde_json::Map<String, Json>) -> String {
    if let Some(source) = item["inner"]["use"]["source"].as_str() {
        return format!("use {source}");
    }
    if let Some(path) = paths.get(id).and_then(|path| path["path"].as_array()) {
        return path
            .iter()
            .filter_map(Json::as_str)
            .collect::<Vec<_>>()
            .join("::");
    }
    item["name"]
        .as_str()
        .map_or_else(|| "impl".to_owned(), str::to_owned)
}

/// One past the largest numeric key of a rustdoc JSON table.
fn next_key(table: &Json) -> u64 {
    table.as_object().map_or(0, |map| {
        map.keys()
            .filter_map(|key| key.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
    }) + 1
}

/// Declares the item at `path` (`crate::…::Item`) of an upstream crate in
/// `doc`; returns its id.
fn plant_upstream(doc: &mut Json, path: &[&str], kind: &str) -> u64 {
    let crate_id = next_key(&doc["external_crates"]);
    let path_id = next_key(&doc["paths"]).max(next_key(&doc["index"]));
    doc["external_crates"][crate_id.to_string()] = json!({ "name": path[0] });
    doc["paths"][path_id.to_string()] = json!({ "crate_id": crate_id, "path": path, "kind": kind });
    path_id
}

/// Plants a public function of the package named `name` into `doc`, whose
/// return type is the item `named`; returns its id.
fn plant_function(doc: &mut Json, name: &str, named: u64, last: &str) -> u64 {
    let item_id = next_key(&doc["paths"]).max(next_key(&doc["index"]));
    doc["index"][item_id.to_string()] = json!({
        "id": item_id,
        "crate_id": 0,
        "name": name,
        "visibility": "public",
        "span": { "filename": "planted.rs", "begin": [1, 0], "end": [1, 0] },
        "inner": { "function": { "sig": { "inputs": [], "output": {
            "resolved_path": { "path": last, "id": named, "args": null }
        } } } }
    });
    item_id
}

/// Plants an upstream crate's item into `doc`: a public function of the
/// package whose return type names `path` (`crate::…::Item`).
fn plant(doc: &mut Json, name: &str, path: &[&str]) {
    let named = plant_upstream(doc, path, "struct");
    plant_function(doc, name, named, path[path.len() - 1]);
}

/// Plants an implementation of the trait at `trait_path` into `doc` whose one
/// method, `name`, returns the upstream item at `path`.
fn plant_implementation(doc: &mut Json, trait_path: &[&str], name: &str, path: &[&str]) {
    let trait_id = plant_upstream(doc, trait_path, "trait");
    let named = plant_upstream(doc, path, "struct");
    let method = plant_function(doc, name, named, path[path.len() - 1]);
    let implementation = method + 1;
    doc["index"][implementation.to_string()] = json!({
        "id": implementation,
        "crate_id": 0,
        "name": null,
        "visibility": "default",
        "span": { "filename": "planted.rs", "begin": [1, 0], "end": [1, 0] },
        "inner": { "impl": {
            "trait": { "path": trait_path[trait_path.len() - 1], "id": trait_id, "args": null },
            "for": { "primitive": "u8" },
            "items": [method],
            "blanket_impl": null,
            "is_synthetic": false,
        } }
    });
}

/// Runs the rule over the real output of `stable` with planted violations.
fn self_test(doc: &Json, stable: &Rule, workspace: &BTreeSet<String>) -> ExitCode {
    let root = Path::new("");
    let names = |findings: &[Finding]| -> BTreeSet<String> {
        findings
            .iter()
            .map(|finding| match finding {
                Finding::Names { item, named, .. } => format!("{item} -> {named}"),
                Finding::UnusedException { krate, .. } => format!("unused exception {krate}"),
                Finding::UnusedEntry { item, krate, .. } => format!("unused entry {item} {krate}"),
                Finding::UnusedTrait { path, krate, .. } => format!("unused trait {path} {krate}"),
            })
            .collect()
    };
    let case = |label: &str, rule: &Rule, plants: &dyn Fn(&mut Json), expected: &[&str]| {
        let mut planted = doc.clone();
        plants(&mut planted);
        let found = check(&planted, rule, workspace, root).map(|f| names(&f));
        let expected: BTreeSet<String> = expected.iter().map(|text| (*text).to_owned()).collect();
        match found {
            Ok(found) if found == expected => {
                println!(
                    "api-closure self-test: {label}: ok ({} finding(s))",
                    expected.len()
                );
                true
            }
            Ok(found) => {
                println!("api-closure self-test: {label}: expected {expected:?}, found {found:?}");
                false
            }
            Err(error) => {
                println!("api-closure self-test: {label}: {error:#}");
                false
            }
        }
    };
    let with_exceptions = Rule {
        exceptions: vec![
            Exception {
                krate: "ui_events".into(),
                exit: "ADR-0143".into(),
            },
            Exception {
                krate: "keyboard_types".into(),
                exit: "ADR-0143".into(),
            },
        ],
        ..stable.clone()
    };
    // What the contract's own API names under an allow list, plus `tracing`.
    let core = Rule {
        package: "core-host".into(),
        mode: Mode::Allow(
            [
                "raw_window_handle",
                "cursor_icon",
                "serde",
                "schemars",
                "tracing",
            ]
            .map(str::to_owned)
            .into(),
        ),
        exceptions: Vec::new(),
        ..stable.clone()
    };
    let with_entries = Rule {
        entries: vec![
            Entry {
                item: "planted_entry".into(),
                krate: "windows".into(),
            },
            Entry {
                item: "planted_gone".into(),
                krate: "windows".into(),
            },
        ],
        ..core.clone()
    };
    const ACCESSIBILITY: &[&str] = &["flui_semantics", "platform", "PlatformAccessibility"];
    let with_traits = Rule {
        traits: vec![
            Admission {
                path: ACCESSIBILITY.join("::"),
                krate: "accesskit".into(),
            },
            Admission {
                path: "flui_semantics::platform::Gone".into(),
                krate: "accesskit".into(),
            },
        ],
        ..core.clone()
    };
    const HWND: &[&str] = &["windows", "Win32", "Foundation", "HWND"];
    const UI_EVENT: &[&str] = &["ui_events", "pointer", "PointerEvent"];
    const TREE: &[&str] = &["accesskit", "TreeUpdate"];
    let results = [
        case(
            &format!("{} as written", stable.package),
            stable,
            &|_| {},
            &[],
        ),
        case(
            "upstream types in a Stable signature",
            stable,
            &|doc| {
                plant(doc, "planted_ui_event", UI_EVENT);
                plant(
                    doc,
                    "planted_raw_handle",
                    &["raw_window_handle", "RawWindowHandle"],
                );
                plant(doc, "planted_cursor", &["cursor_icon", "CursorIcon"]);
                plant(
                    doc,
                    "planted_handle_error",
                    &["raw_window_handle", "HandleError"],
                );
            },
            &[
                "planted_ui_event -> ui_events::pointer::PointerEvent",
                "planted_raw_handle -> raw_window_handle::RawWindowHandle",
            ],
        ),
        case(
            "an exception in use and one no longer needed",
            &with_exceptions,
            &|doc| plant(doc, "planted_ui_event", UI_EVENT),
            &["unused exception keyboard_types"],
        ),
        case(
            "an OS type in the core host",
            &core,
            &|doc| {
                plant(doc, "planted_hwnd", HWND);
                plant(doc, "planted_span", &["tracing", "Span"]);
            },
            &["planted_hwnd -> windows::Win32::Foundation::HWND"],
        ),
        case(
            "an OS crate no list names (a dependency's binding)",
            &core,
            &|doc| plant(doc, "planted_device", &["objc2_metal", "MTLDevice"]),
            &["planted_device -> objc2_metal::MTLDevice"],
        ),
        case(
            "a left-out type through an admitted trait, another workspace trait, and outside one",
            &with_traits,
            &|doc| {
                plant_implementation(doc, ACCESSIBILITY, "planted_publish", TREE);
                plant_implementation(
                    doc,
                    &["flui_semantics", "platform", "Other"],
                    "planted_other",
                    TREE,
                );
                plant(doc, "planted_published", TREE);
            },
            &[
                "planted_other -> accesskit::TreeUpdate",
                "planted_published -> accesskit::TreeUpdate",
                "unused trait flui_semantics::platform::Gone accesskit",
            ],
        ),
        case(
            "an entry in use and one no longer needed",
            &with_entries,
            &|doc| {
                plant(doc, "planted_entry", HWND);
                plant(doc, "planted_hwnd", HWND);
            },
            &[
                "planted_hwnd -> windows::Win32::Foundation::HWND",
                "unused entry planted_gone windows",
            ],
        ),
    ];
    if results.iter().all(|ok| *ok) {
        println!("api-closure: self-test ok ({} cases)", results.len());
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests;
