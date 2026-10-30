//! `cargo xtask send-impls`: every `unsafe impl Send` and `unsafe impl Sync`
//! in the workspace is listed and the list only shrinks (ADR-0136).
//!
//! UI state is owner-local (ADR-0136): a framework type crosses a thread
//! because the compiler proves it may, never because a hand-written
//! `unsafe impl` says so. The impls that stay wrap an OS handle the platform
//! guarantees for another thread (a Win32 loop, a COM apartment, an
//! Objective-C object only the main thread touches) or a hot-reload plugin's
//! library; each sits in `flui-native`, `flui-hot-reload` or
//! `tools/desktop-mcp` with its `SAFETY:` comment.
//!
//! **What counts.** In the library and bin targets of every workspace member
//! ([`crate::source_tree`] follows each module tree the way rustc does; a
//! module or item under a cfg that is false in every build, `test`, is left
//! out), an `unsafe impl` of a trait whose path ends in `Send` or `Sync`, at
//! any depth: in a module, a function body or a block.
//!
//! **The list.** [`ALLOWLIST`] names, per crate, the self types and the traits
//! each implements. An impl the list does not name is a finding; so is a
//! listed one the code no longer has, so the list only shrinks. `--seed` prints
//! the list the tree needs.
//!
//! **Limits.** The gate reads source, not expansions: an impl a macro emits is
//! out of its reach, and a self type is named by its last path segment.
//! `deny.toml` bans the crates that wrap a value to make it `Send`
//! (`send_wrapper`, `fragile`, `send-cell`, `unsend`, `force-send-sync`).
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use cargo_metadata::{Metadata, TargetKind};
use serde::Deserialize;
use syn::visit::{self, Visit};

use crate::source_tree::{Disk, Memory, Source, Target, Truth, cfg_of, walk};
use crate::util;
use crate::workspace::relative;

/// The list, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/send-impls.toml";

/// Arguments for `cargo xtask send-impls`.
#[derive(Debug, clap::Args)]
pub(crate) struct SendImplsArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print the list the tree needs instead of checking it.
    #[arg(long, conflicts_with = "self_test")]
    seed: bool,
}

/// `cargo xtask send-impls`. Exit code 1 lists every finding.
pub(crate) fn send_impls(args: &SendImplsArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let crates = crates(&root, &util::metadata(&root)?)?;
    let impls = scan(&Disk(root.clone()), &crates)?;
    if args.seed {
        print!("{}", seed(&impls));
        return Ok(ExitCode::SUCCESS);
    }
    let path = root.join(ALLOWLIST);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let list = parse_list(&text).with_context(|| format!("parsing {ALLOWLIST}"))?;
    let findings = check(&impls, &list);
    if findings.is_empty() {
        println!(
            "send-impls: {} unsafe impls of Send or Sync, each listed in {ALLOWLIST}",
            impls.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("send-impls: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// The auto trait an impl implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
enum Marker {
    Send,
    Sync,
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Send => "Send",
            Self::Sync => "Sync",
        })
    }
}

/// What the list keys an impl by: its crate, its self type and the trait.
type Key = (String, String, Marker);

/// One `unsafe impl Send`/`Sync` the scan found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    krate: String,
    ty: String,
    marker: Marker,
    file: String,
    line: usize,
}

impl Found {
    fn key(&self) -> Key {
        (self.krate.clone(), self.ty.clone(), self.marker)
    }
}

/// A crate the scan reads.
struct Krate {
    name: String,
    targets: Vec<Target>,
}

/// Every workspace member, with its library and bin targets.
fn crates(root: &Path, metadata: &Metadata) -> anyhow::Result<Vec<Krate>> {
    let mut crates = Vec::new();
    for package in metadata.workspace_packages() {
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
            targets,
        });
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crates)
}

/// Every `unsafe impl Send`/`Sync` in the crates' targets, each once.
fn scan(source: &dyn Source, crates: &[Krate]) -> anyhow::Result<BTreeSet<Found>> {
    let mut found = BTreeSet::new();
    for krate in crates {
        let mut seen = BTreeSet::new();
        for target in &krate.targets {
            for file in walk(source, target)? {
                // A library and a bin of one package may share modules.
                if !seen.insert(file.rel.clone()) {
                    continue;
                }
                let mut visitor = Impls {
                    krate: &krate.name,
                    file: &file.rel,
                    found: &mut found,
                };
                visitor.visit_file(&file.ast);
            }
        }
    }
    Ok(found)
}

/// Collects the unsafe impls of one file, leaving out what a false cfg hides.
struct Impls<'a> {
    krate: &'a str,
    file: &'a str,
    found: &'a mut BTreeSet<Found>,
}

impl<'ast> Visit<'ast> for Impls<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if cfg_of(&module.attrs).0 != Truth::False {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if cfg_of(&function.attrs).0 != Truth::False {
            visit::visit_item_fn(self, function);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if cfg_of(&item.attrs).0 == Truth::False {
            return;
        }
        if let (Some(unsafety), Some((path, _))) = (&item.unsafety, &item.trait_)
            && let Some(marker) = marker(path)
        {
            self.found.insert(Found {
                krate: self.krate.to_owned(),
                ty: type_name(&item.self_ty),
                marker,
                file: self.file.to_owned(),
                line: unsafety.span.start().line,
            });
        }
        visit::visit_item_impl(self, item);
    }
}

/// `Send` or `Sync`, when `path` ends in one.
fn marker(path: &syn::Path) -> Option<Marker> {
    let last = path.segments.last()?;
    if !last.arguments.is_none() {
        return None;
    }
    match last.ident.to_string().as_str() {
        "Send" => Some(Marker::Send),
        "Sync" => Some(Marker::Sync),
        _ => None,
    }
}

/// A self type's name: the last segment of its path, through references and
/// pointers; `_` for a shape with no name.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_owned(), |segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => type_name(&reference.elem),
        syn::Type::Ptr(pointer) => type_name(&pointer.elem),
        syn::Type::Paren(inner) => type_name(&inner.elem),
        syn::Type::Group(inner) => type_name(&inner.elem),
        _ => "_".to_owned(),
    }
}

/// The list as [`ALLOWLIST`] writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFile {
    reason: String,
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One listed self type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(rename = "crate")]
    krate: String,
    #[serde(rename = "type")]
    ty: String,
    traits: Vec<Marker>,
}

/// The listed keys. A key listed twice is an error.
fn parse_list(text: &str) -> anyhow::Result<BTreeSet<Key>> {
    let file: ListFile = toml::from_str(text)?;
    anyhow::ensure!(
        !file.reason.trim().is_empty(),
        "the list's `reason` is empty"
    );
    let mut keys = BTreeSet::new();
    for entry in file.allow {
        for marker in entry.traits {
            let key = (entry.krate.clone(), entry.ty.clone(), marker);
            anyhow::ensure!(
                keys.insert(key),
                "{} `{}`: {marker} is listed twice",
                entry.krate,
                entry.ty
            );
        }
    }
    Ok(keys)
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// An unsafe impl the list does not name.
    Unlisted(Found),
    /// A listed impl the code no longer has.
    Stale(Key),
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted(found) => write!(
                f,
                "{}:{}: `unsafe impl {} for {}` in {} is not in {ALLOWLIST}: make the type {} \
                 by what it holds, or keep it on its owner thread (ADR-0136)",
                found.file, found.line, found.marker, found.ty, found.krate, found.marker
            ),
            Self::Stale((krate, ty, marker)) => write!(
                f,
                "{krate} `{ty}`: {marker} is listed, but the code has no such unsafe impl; \
                 remove it from {ALLOWLIST}"
            ),
        }
    }
}

/// Every unlisted impl and every stale entry.
fn check(found: &BTreeSet<Found>, list: &BTreeSet<Key>) -> Vec<Finding> {
    let keys: BTreeSet<Key> = found.iter().map(Found::key).collect();
    let mut findings: Vec<Finding> = found
        .iter()
        .filter(|found| !list.contains(&found.key()))
        .cloned()
        .map(Finding::Unlisted)
        .collect();
    findings.extend(
        list.iter()
            .filter(|key| !keys.contains(*key))
            .cloned()
            .map(Finding::Stale),
    );
    findings.sort();
    findings
}

/// The list the tree needs, in [`ALLOWLIST`]'s format.
fn seed(found: &BTreeSet<Found>) -> String {
    let mut types: BTreeMap<(&str, &str), Vec<Marker>> = BTreeMap::new();
    for found in found {
        types
            .entry((found.krate.as_str(), found.ty.as_str()))
            .or_default()
            .push(found.marker);
    }
    let mut out = String::from(
        "# Every `unsafe impl Send`/`Sync` in the workspace (ADR-0136), written by\n\
         # `cargo xtask send-impls --seed`. The list only shrinks: the scan fails on an impl it\n\
         # does not name and on an entry the code no longer has.\n\
         reason = \"an OS handle or a plugin library the platform lets another thread hold; each \
         impl carries its SAFETY comment\"\n",
    );
    for ((krate, ty), mut markers) in types {
        markers.dedup();
        let traits: Vec<String> = markers.iter().map(|m| format!("\"{m}\"")).collect();
        let _ = write!(
            out,
            "\n[[allow]]\ncrate = \"{krate}\"\ntype = \"{ty}\"\ntraits = [{}]\n",
            traits.join(", ")
        );
    }
    out
}

/// The self-test's crate root.
const FIXTURE_ROOT: &str = "fixture/src/lib.rs";

/// The self-test's crate: one planted impl per shape the scan must see, one
/// hidden by each cfg it must skip, and a list with one stale entry.
fn fixture() -> (Memory, BTreeSet<Key>) {
    let lib = r"
        mod host;
        #[cfg(test)]
        mod tests;

        pub struct Listed;
        unsafe impl Send for Listed {}
        unsafe impl Sync for Listed {}

        pub struct Planted;
        unsafe impl Send for Planted {}

        pub struct Generic<T>(T);
        unsafe impl<T> core::marker::Sync for Generic<T> {}

        pub fn inside() {
            struct Local;
            unsafe impl Send for Local {}
        }

        #[cfg(test)]
        unsafe impl Sync for Planted {}

        // Not unsafe, not an auto trait: neither counts.
        impl Clone for Listed { fn clone(&self) -> Self { Listed } }
        pub unsafe trait Raw {}
        unsafe impl Raw for Listed {}
    ";
    let host = r"
        pub struct Handle(*mut u8);
        unsafe impl Send for &Handle {}
        mod inner {
            pub struct Deep;
            unsafe impl Sync for Deep {}
        }
    ";
    let tests = r"
        struct Fake;
        unsafe impl Send for Fake {}
    ";
    let files = [
        (FIXTURE_ROOT, lib),
        ("fixture/src/host.rs", host),
        ("fixture/src/tests.rs", tests),
    ];
    let source = Memory(
        files
            .into_iter()
            .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
            .collect(),
    );
    let list = parse_list(
        r#"
        reason = "fixture"

        [[allow]]
        crate = "fixture"
        type = "Listed"
        traits = ["Send", "Sync"]

        [[allow]]
        crate = "fixture"
        type = "Removed"
        traits = ["Send"]
        "#,
    )
    .expect("BUG: the fixture list parses");
    (source, list)
}

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 6] = [
    "unsafe impl Send for Planted",
    "unsafe impl Sync for Generic",
    "unsafe impl Send for Local",
    "unsafe impl Send for Handle",
    "unsafe impl Sync for Deep",
    "fixture `Removed`: Send is listed",
];

/// Runs the rule over the fixture: what it missed and what it reported
/// beyond [`EXPECTED`].
fn self_test_diff() -> (Vec<String>, Vec<String>) {
    let (source, list) = fixture();
    let crates = [Krate {
        name: "fixture".to_owned(),
        targets: vec![Target {
            src: FIXTURE_ROOT.to_owned(),
            bin: None,
        }],
    }];
    let found = scan(&source, &crates).expect("BUG: the fixture's module tree parses");
    let reported: Vec<String> = check(&found, &list)
        .iter()
        .map(ToString::to_string)
        .collect();
    let missed = EXPECTED
        .iter()
        .filter(|head| !reported.iter().any(|line| line.contains(*head)))
        .map(|head| (*head).to_owned())
        .collect();
    let extra = reported
        .into_iter()
        .filter(|line| !EXPECTED.iter().any(|head| line.contains(head)))
        .collect();
    (missed, extra)
}

/// `cargo xtask send-impls --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff();
    for head in &missed {
        println!("self-test: MISSED {head}");
    }
    for line in &extra {
        println!("self-test: FALSE POSITIVE {line}");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "send-impls: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_impls_fixture_reports_exactly_the_planted_findings() {
        let (missed, extra) = self_test_diff();
        assert!(missed.is_empty(), "missed: {missed:?}");
        assert!(extra.is_empty(), "false positives: {extra:?}");
    }

    #[test]
    fn send_impls_list_rejects_a_key_listed_twice() {
        let error = parse_list(
            r#"
            reason = "r"
            [[allow]]
            crate = "a"
            type = "T"
            traits = ["Send", "Send"]
            "#,
        )
        .expect_err("a duplicate is an error");
        assert!(error.to_string().contains("listed twice"), "{error}");
    }

    #[test]
    fn send_impls_seed_round_trips_through_the_list() {
        let (source, _) = fixture();
        let crates = [Krate {
            name: "fixture".to_owned(),
            targets: vec![Target {
                src: FIXTURE_ROOT.to_owned(),
                bin: None,
            }],
        }];
        let found = scan(&source, &crates).expect("BUG: the fixture's module tree parses");
        let list = parse_list(&seed(&found)).expect("the seed parses");
        assert_eq!(check(&found, &list), Vec::new());
    }
}
