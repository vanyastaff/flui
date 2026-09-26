//! `cargo xtask globals`: process-global state is listed, reviewed and only
//! shrinks (ADR-0097).
//!
//! Every `static` (at any depth, `static mut` included), every entry of a
//! `thread_local!`, and every `static` in the tokens of a macro invocation or
//! a `macro_rules!` body, in the library, proc-macro and bin targets of each
//! crate under `crates/` and the facade (not `tier-kind = "tool"`), is a
//! global. Items under a cfg that is false whatever the build (`test`) are
//! left out; every other cfg is scanned, so the result does not depend on the
//! host. Two shapes are exempt:
//!
//! - **immutable data**: a non-`mut` `static` of a primitive, a shared
//!   reference to immutable data, an array, slice or tuple of it, or a bare
//!   `fn` pointer;
//! - **monotonic ID counters**: a non-`mut`, non-`pub` atomic integer `static`
//!   initialized with a literal, whose every mention where it is visible is
//!   its declaration, `NAME.fetch_add(<literal ≥ 1>, …)` or a `use` path.
//!
//! Every other global has an entry in its crate's
//! `[package.metadata.flui] globals`, keyed by its item path in the target:
//! debt names the ADR that removes it (`exit`); a permanent grant names
//! ADR-0097 and a class (`trampoline`, `counter`, `immutable`, `process`,
//! `diagnostic`), and the gate checks what it can of each class. A global
//! with no entry, an entry with no global (or for an exempt one), a class the
//! global does not fit, and a second trampoline in the host or in one
//! platform backend are findings.
//!
//! The gate sees source, not expansions: environment-variable selection,
//! `include!` of build-script output, state inside dependencies, statics that
//! dependency proc-macros emit, and `const` items are out of its reach.
//!
//! `--self-test` runs the rules over built-in crates with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use cargo_metadata::{Metadata, TargetKind};
use serde_json::Value as Json;

use crate::util;

mod allowlist;
mod fixture;
mod scan;

use allowlist::{Class, Standing};
use scan::{Def, Shape, Source, Target, Vis};

/// Arguments for `cargo xtask globals`.
#[derive(Debug, clap::Args)]
pub(crate) struct GlobalsArgs {
    /// Run the rules over built-in crates with planted violations instead of
    /// the workspace; exit 1 unless they report exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print, per crate, a `globals = [...]` skeleton of every global with
    /// no entry and no exemption. Writes nothing; the empty reasons it
    /// prints do not parse until filled in.
    #[arg(long, conflicts_with = "self_test")]
    seed: bool,
}

/// The crate that hosts every realm; its one trampoline is `APP_RUNTIME`.
/// ADR-0083 moves the host, and this constant with it.
const HOST: &str = "flui-app";

/// The crate whose backend directories may each hold one trampoline.
const PLATFORM: &str = "flui-platform";

/// The backend directories, one per platform.
const BACKENDS: &str = "crates/flui-platform/src/platforms/";

/// `cargo xtask globals`.
pub(crate) fn globals(args: &GlobalsArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let crates = crates(&root, &metadata)?;
    let report = check(&Disk(root.clone()), &crates, &adr_numbers(&root)?)?;
    if args.seed {
        print!("{}", report.seed());
        return Ok(ExitCode::SUCCESS);
    }
    if report.problems.is_empty() {
        println!("globals: {}", report.summary());
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("globals: {} problem(s)", report.problems.len());
    for problem in &report.problems {
        eprintln!("  - {problem}");
    }
    Ok(ExitCode::FAILURE)
}

/// A crate the gate scans.
#[derive(Debug, Clone)]
struct Krate {
    name: String,
    /// Manifest path, repository-relative.
    manifest: String,
    /// `[package.metadata.flui]`, `null` when absent.
    flui: Json,
    targets: Vec<Target>,
}

/// The crates under `crates/` and the facade, less applications, with their
/// library, proc-macro and bin targets.
fn crates(root: &Path, metadata: &Metadata) -> anyhow::Result<Vec<Krate>> {
    let mut crates = Vec::new();
    for package in metadata.workspace_packages() {
        let manifest = relative(root, package.manifest_path.as_std_path())?;
        let flui = package.metadata["flui"].clone();
        let in_scope = manifest == "Cargo.toml" || manifest.starts_with("crates/");
        if !in_scope || flui["tier-kind"].as_str() == Some("tool") {
            continue;
        }
        let mut targets = Vec::new();
        for target in &package.targets {
            let library = target.kind.iter().any(|kind| {
                matches!(
                    kind,
                    TargetKind::Lib
                        | TargetKind::RLib
                        | TargetKind::CDyLib
                        | TargetKind::DyLib
                        | TargetKind::StaticLib
                        | TargetKind::ProcMacro
                )
            });
            let bin = target.kind.contains(&TargetKind::Bin);
            if library || bin {
                targets.push(Target {
                    src: relative(root, target.src_path.as_std_path())?,
                    bin: (bin && !library).then(|| target.name.clone()),
                });
            }
        }
        crates.push(Krate {
            name: package.name.to_string(),
            manifest,
            flui,
            targets,
        });
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crates)
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> anyhow::Result<String> {
    let normalize = |path: &Path| -> PathBuf {
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
    };
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

/// The `ADR-NNNN` numbers with a file under `docs/adr`.
fn adr_numbers(root: &Path) -> anyhow::Result<BTreeSet<String>> {
    let dir = root.join("docs").join("adr");
    let mut numbers = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
        let file = entry?.file_name().to_string_lossy().into_owned();
        if let Some(number) = file.get(..8).filter(|prefix| {
            prefix.starts_with("ADR-") && prefix[4..].bytes().all(|byte| byte.is_ascii_digit())
        }) {
            numbers.insert(number.to_owned());
        }
    }
    Ok(numbers)
}

/// The repository on disk.
struct Disk(PathBuf);

impl Source for Disk {
    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(rel)).ok()
    }

    fn exists(&self, rel: &str) -> bool {
        self.0.join(rel).is_file()
    }
}

/// Why a global needs no entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exempt {
    Immutable,
    Counter,
}

impl fmt::Display for Exempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Immutable => "immutable data",
            Self::Counter => "a monotonic ID counter",
        })
    }
}

/// The exemption every definition of one global shares, if any.
fn exemption(defs: &[Def]) -> Option<Exempt> {
    let of = |def: &Def| {
        if def.shape != Shape::Static {
            return None;
        }
        if def.ty.immutable_data {
            return Some(Exempt::Immutable);
        }
        let counter = def.ty.atomic_int
            && def.counter_init
            && def.vis != Vis::Public
            && def.uses.as_ref().is_some_and(|uses| uses.disallowed == 0);
        counter.then_some(Exempt::Counter)
    };
    let first = of(defs.first()?)?;
    defs.iter()
        .all(|def| of(def) == Some(first))
        .then_some(first)
}

/// One problem the gate reports.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Problem {
    krate: String,
    /// The defining file, or the manifest for an entry with no global.
    file: String,
    item: String,
    kind: &'static str,
    detail: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {} — {}: {}",
            self.krate, self.file, self.item, self.kind, self.detail
        )
    }
}

const NEW: &str = "new global";
const STALE: &str = "stale entry";
const CLASS: &str = "class";
const SECOND_TRAMPOLINE: &str = "second trampoline";
const PLACEMENT: &str = "trampoline placement";
const AMBIGUOUS: &str = "ambiguous";

/// What one run found.
#[derive(Debug, Default)]
struct Report {
    problems: Vec<Problem>,
    listed: usize,
    crates_listing: usize,
    trampolines: usize,
    exempt_counters: usize,
    exempt_immutable: usize,
    files: usize,
    /// Per crate: its manifest and the unexempted globals with no entry.
    unlisted: BTreeMap<String, (String, Vec<String>)>,
}

impl Report {
    fn summary(&self) -> String {
        format!(
            "{} listed in {} crates ({} trampoline), {} exempt ({} counters, {} immutable data), \
             {} files parsed",
            self.listed,
            self.crates_listing,
            self.trampolines,
            self.exempt_counters + self.exempt_immutable,
            self.exempt_counters,
            self.exempt_immutable,
            self.files
        )
    }

    fn seed(&self) -> String {
        let mut lines = Vec::new();
        for (krate, (manifest, items)) in &self.unlisted {
            lines.push(format!("# {krate} ({manifest})"));
            lines.push("globals = [".to_owned());
            lines.extend(
                items
                    .iter()
                    .map(|item| format!("    {{ item = \"{item}\", reason = \"\" }},")),
            );
            lines.push("]".to_owned());
            lines.push(String::new());
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

/// Scans `crates` and reconciles each with its entries.
fn check(source: &dyn Source, crates: &[Krate], adrs: &BTreeSet<String>) -> anyhow::Result<Report> {
    let mut report = Report::default();
    // (slot, crate, item, file) of each trampoline grant
    let mut trampolines: Vec<(Option<String>, String, String, String)> = Vec::new();

    for krate in crates {
        let entries = allowlist::parse(&krate.flui, &krate.manifest, adrs)?;
        let mut by_item: BTreeMap<String, Vec<Def>> = BTreeMap::new();
        for target in &krate.targets {
            let scanned = scan::scan_target(source, target)
                .with_context(|| format!("{}: scanning {}", krate.name, target.src))?;
            report.files += scanned.files;
            for def in scanned.defs {
                by_item.entry(def.item.clone()).or_default().push(def);
            }
        }
        if !entries.is_empty() {
            report.crates_listing += 1;
        }

        let problem = |file: &str, item: &str, kind, detail: String| Problem {
            krate: krate.name.clone(),
            file: file.to_owned(),
            item: item.to_owned(),
            kind,
            detail,
        };
        let mut unlisted = Vec::new();
        let mut stale = Vec::new();
        for (item, defs) in &by_item {
            let file = &defs[0].file;
            let mut cfgs = BTreeSet::new();
            let ambiguous = defs
                .iter()
                .filter(|def| def.shape != Shape::Macro)
                .any(|def| !cfgs.insert(def.cfg.as_str()));
            if ambiguous {
                report.problems.push(problem(
                    file,
                    item,
                    AMBIGUOUS,
                    "two definitions share this key under the same cfg; rename one".to_owned(),
                ));
            }
            let entry = entries.iter().find(|entry| &entry.item == item);
            match (exemption(defs), entry) {
                (Some(exempt), Some(_)) => stale.push(problem(
                    &krate.manifest,
                    item,
                    STALE,
                    format!("exempt as {exempt}; remove the entry"),
                )),
                (Some(Exempt::Counter), None) => report.exempt_counters += 1,
                (Some(Exempt::Immutable), None) => report.exempt_immutable += 1,
                (None, None) => unlisted.push((item.clone(), file.clone())),
                (None, Some(entry)) => {
                    report.listed += 1;
                    if let Standing::Grant(class) = entry.standing {
                        if let Some(detail) = allowlist::class_problem(class, defs) {
                            report.problems.push(problem(
                                file,
                                item,
                                CLASS,
                                format!("listed as `{class}`, but {detail}"),
                            ));
                        }
                        if class == Class::Trampoline {
                            report.trampolines += 1;
                            trampolines.push((
                                trampoline_slot(&krate.name, defs),
                                krate.name.clone(),
                                item.clone(),
                                file.clone(),
                            ));
                        }
                    }
                }
            }
        }
        for entry in &entries {
            if !by_item.contains_key(&entry.item) {
                stale.push(problem(
                    &krate.manifest,
                    &entry.item,
                    STALE,
                    "no global has this key; remove the entry".to_owned(),
                ));
            }
        }
        for (item, file) in &unlisted {
            let name = item.rsplit("::").next().unwrap_or(item);
            let moved = stale
                .iter()
                .find(|entry| entry.item.rsplit("::").next() == Some(name))
                .map(|entry| {
                    format!(
                        " (the stale entry `{}` is probably its old key)",
                        entry.item
                    )
                })
                .unwrap_or_default();
            report.problems.push(problem(
                file,
                item,
                NEW,
                format!(
                    "no `[package.metadata.flui] globals` entry in {}; move the state into a \
                     realm, or list it with the ADR that removes it{moved}",
                    krate.manifest
                ),
            ));
        }
        report.problems.extend(stale);
        if !unlisted.is_empty() {
            report.unlisted.insert(
                krate.name.clone(),
                (
                    krate.manifest.clone(),
                    unlisted.into_iter().map(|(item, _)| item).collect(),
                ),
            );
        }
    }

    trampolines.sort();
    let mut holders: BTreeMap<String, String> = BTreeMap::new();
    for (slot, krate, item, file) in trampolines {
        let problem = |kind, detail| Problem {
            krate: krate.clone(),
            file: file.clone(),
            item: item.clone(),
            kind,
            detail,
        };
        match slot {
            None => report.problems.push(problem(
                PLACEMENT,
                format!(
                    "a trampoline lives only in the host ({HOST}) or in a backend directory under \
                     {BACKENDS}"
                ),
            )),
            Some(slot) => match holders.get(&slot) {
                Some(first) => report.problems.push(problem(
                    SECOND_TRAMPOLINE,
                    format!("{slot} already has its one trampoline, `{first}`"),
                )),
                None => {
                    holders.insert(slot, format!("{krate}::{item}"));
                }
            },
        }
    }
    Ok(report)
}

/// Which one-trampoline slot a global occupies: the host, or one backend
/// directory of the platform crate.
fn trampoline_slot(krate: &str, defs: &[Def]) -> Option<String> {
    if krate == HOST {
        return Some(format!("the host ({HOST})"));
    }
    if krate != PLATFORM {
        return None;
    }
    let backend = |def: &Def| {
        let rest = def.file.strip_prefix(BACKENDS)?;
        let (backend, _) = rest.split_once('/')?;
        Some(backend.to_owned())
    };
    let first = backend(&defs[0])?;
    defs.iter()
        .all(|def| backend(def).as_deref() == Some(first.as_str()))
        .then(|| format!("the {first} backend"))
}

// ---------------------------------------------------------------------------
// self-test

/// `(crate, item, kind)`: what the self-test compares.
type Identity = (String, String, String);

/// `(missed, false positives)` of the rules over the fixture crates against
/// `expected`.
fn self_test_diff(expected: &[(&str, &str, &str)]) -> (Vec<Identity>, Vec<Identity>) {
    let (source, crates) = fixture::crates();
    let report = check(&source, &crates, &fixture::adrs())
        .expect("BUG: the self-test crates scan and their entries parse");
    let seen: BTreeSet<Identity> = report
        .problems
        .iter()
        .map(|problem| {
            (
                problem.krate.clone(),
                problem.item.clone(),
                problem.kind.to_owned(),
            )
        })
        .collect();
    let expected: BTreeSet<Identity> = expected
        .iter()
        .map(|&(krate, item, kind)| (krate.to_owned(), item.to_owned(), kind.to_owned()))
        .collect();
    (
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    )
}

/// `cargo xtask globals --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff(&fixture::EXPECTED);
    for (krate, item, kind) in &missed {
        println!("self-test: MISSED ({krate}, {item}, {kind})");
    }
    for (krate, item, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({krate}, {item}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "globals: self-test ok ({} expected findings, no others)",
        fixture::EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
