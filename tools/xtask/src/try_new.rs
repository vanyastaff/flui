//! `cargo xtask try-new`: a constructor that can fail is named `try_new`, and
//! the list of the `new`s that still fail only shrinks.
//!
//! `Type::new` reads as a constructor that always gives a value; a reader who
//! sees it does not look for the `?`. AGENTS.md names a `new` returning
//! `Result` or `Option` as a defect: that constructor is `try_new`. Clippy has
//! no lint for it (`new_ret_no_self` accepts `Result<Self, _>`), so this gate
//! is a grep over the syntax tree.
//!
//! **What counts.** In the library and bin targets of every workspace member
//! ([`crate::source_tree`] follows each module tree the way rustc does; a
//! module or item under a cfg that is false in every build, `test`, is left
//! out), an associated function named `new` of an inherent `impl` whose
//! return type's path ends in `Result` or `Option` (`io::Result`,
//! `anyhow::Result` and a crate's own `Result` alias included). A trait's
//! `new` is the trait's to name and is left out.
//!
//! **The list.** [`ALLOWLIST`] names, per crate, the self types whose `new`
//! still fails. A fallible `new` the list does not name is a finding; so is a
//! listed one the code no longer has, so the list only shrinks. `--seed`
//! prints the list the tree needs.
//!
//! **Limits.** The gate reads source, not expansions: a `new` a macro emits
//! is out of its reach. A self type is named by its last path segment, so two
//! types of one name in one crate share an entry.
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).

use std::collections::BTreeSet;
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
const ALLOWLIST: &str = "tools/xtask/allowlists/try-new.toml";

/// Arguments for `cargo xtask try-new`.
#[derive(Debug, clap::Args)]
pub(crate) struct TryNewArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print the list the tree needs instead of checking it.
    #[arg(long, conflicts_with = "self_test")]
    seed: bool,
}

/// `cargo xtask try-new`. Exit code 1 lists every finding.
pub(crate) fn try_new(args: &TryNewArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let crates = crates(&root, &util::metadata(&root)?)?;
    let found = scan(&Disk(root.clone()), &crates)?;
    if args.seed {
        print!("{}", seed(&found));
        return Ok(ExitCode::SUCCESS);
    }
    let path = root.join(ALLOWLIST);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let list = parse_list(&text).with_context(|| format!("parsing {ALLOWLIST}"))?;
    let findings = check(&found, &list);
    if findings.is_empty() {
        println!(
            "try-new: {} fallible `new` constructor(s) left, each listed in {ALLOWLIST}",
            list.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("try-new: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// What the list keys a constructor by: its crate and its self type.
type Key = (String, String);

/// One fallible `new` the scan found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    krate: String,
    ty: String,
    file: String,
    line: usize,
}

impl Found {
    fn key(&self) -> Key {
        (self.krate.clone(), self.ty.clone())
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

/// Every fallible `new` in the crates' targets, each once.
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
                let mut visitor = Constructors {
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

/// Collects the fallible `new`s of one file, leaving out what a false cfg
/// hides.
struct Constructors<'a> {
    krate: &'a str,
    file: &'a str,
    found: &'a mut BTreeSet<Found>,
}

impl<'ast> Visit<'ast> for Constructors<'_> {
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
        if item.trait_.is_none() {
            for impl_item in &item.items {
                if let syn::ImplItem::Fn(function) = impl_item
                    && function.sig.ident == "new"
                    && cfg_of(&function.attrs).0 != Truth::False
                    && fallible(&function.sig.output)
                {
                    self.found.insert(Found {
                        krate: self.krate.to_owned(),
                        ty: type_name(&item.self_ty),
                        file: self.file.to_owned(),
                        line: function.sig.ident.span().start().line,
                    });
                }
            }
        }
        visit::visit_item_impl(self, item);
    }
}

/// Whether a return type's path ends in `Result` or `Option`.
fn fallible(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    let syn::Type::Path(path) = ty.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Result" || segment.ident == "Option")
}

/// A self type's name: the last segment of its path, through references;
/// `_` for a shape with no name.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_owned(), |segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => type_name(&reference.elem),
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
        let key = (entry.krate, entry.ty);
        anyhow::ensure!(
            !keys.contains(&key),
            "{} `{}` is listed twice",
            key.0,
            key.1
        );
        keys.insert(key);
    }
    Ok(keys)
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// A fallible `new` the list does not name.
    Unlisted(Found),
    /// A listed type whose `new` no longer fails, or is gone.
    Stale(Key),
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted(found) => write!(
                f,
                "{}:{}: `{}::new` in {} returns `Result` or `Option`: name it `try_new` \
                 (AGENTS.md)",
                found.file, found.line, found.ty, found.krate
            ),
            Self::Stale((krate, ty)) => write!(
                f,
                "{krate} `{ty}` is listed, but its `new` no longer fails; remove it from \
                 {ALLOWLIST}"
            ),
        }
    }
}

/// Every unlisted constructor and every stale entry.
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
    let keys: BTreeSet<Key> = found.iter().map(Found::key).collect();
    let mut out = String::from(
        "# Every `new` in the workspace that returns `Result` or `Option`, written by\n\
         # `cargo xtask try-new --seed`. Such a constructor is named `try_new` (AGENTS.md); the\n\
         # list only shrinks: the scan fails on a fallible `new` it does not name and on an\n\
         # entry whose `new` no longer fails.\n\
         reason = \"written before the rule had a gate; each entry is renamed to `try_new`\"\n",
    );
    for (krate, ty) in &keys {
        let _ = write!(out, "\n[[allow]]\ncrate = \"{krate}\"\ntype = \"{ty}\"\n");
    }
    out
}

/// The self-test's crate root.
const FIXTURE_ROOT: &str = "fixture/src/lib.rs";

/// The self-test's crate: one planted constructor per shape the scan must
/// see, one hidden by each cfg it must skip, the shapes it must leave alone,
/// and a list with one stale entry.
fn fixture() -> (Memory, BTreeSet<Key>) {
    let lib = r"
        mod host;
        #[cfg(test)]
        mod tests;

        pub struct Listed;
        impl Listed { pub fn new() -> Result<Self, ()> { Ok(Self) } }

        pub struct Planted;
        impl Planted { pub fn new() -> Option<Self> { Some(Self) } }

        pub struct Io;
        impl Io { pub const fn new() -> std::io::Result<Self> { Ok(Self) } }

        pub fn inside() {
            struct Local;
            impl Local { fn new() -> Result<Self, ()> { Ok(Self) } }
        }

        pub struct Hidden;
        impl Hidden {
            #[cfg(test)]
            fn new() -> Option<Self> { None }
        }

        // An infallible `new`, a fallible `try_new`, a trait's `new` and a
        // `new` returning another type that only wraps one: none counts.
        pub struct Fine;
        impl Fine {
            pub fn new() -> Self { Self }
            pub fn try_new() -> Result<Self, ()> { Ok(Self) }
        }
        pub trait Make { fn new() -> Option<Self> where Self: Sized; }
        impl Make for Fine { fn new() -> Option<Self> { None } }
        pub struct Boxed;
        impl Boxed { pub fn new() -> Box<Result<Self, ()>> { Box::new(Ok(Self)) } }
    ";
    let host = r"
        mod inner {
            pub struct Deep;
            impl Deep { pub(crate) fn new() -> crate::Result<Self> { todo!() } }
        }
    ";
    let tests = r"
        struct Fake;
        impl Fake { fn new() -> Option<Self> { None } }
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

        [[allow]]
        crate = "fixture"
        type = "Renamed"
        "#,
    )
    .expect("BUG: the fixture list parses");
    (source, list)
}

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 5] = [
    "`Planted::new`",
    "`Io::new`",
    "`Local::new`",
    "`Deep::new`",
    "fixture `Renamed` is listed",
];

/// Runs the rule over the fixture: what it missed and what it reported
/// beyond [`EXPECTED`].
fn self_test_diff() -> (Vec<String>, Vec<String>) {
    let (source, list) = fixture();
    let found = scan(&source, &fixture_crates()).expect("BUG: the fixture's module tree parses");
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

/// The fixture as the one crate the scan reads.
fn fixture_crates() -> [Krate; 1] {
    [Krate {
        name: "fixture".to_owned(),
        targets: vec![Target {
            src: FIXTURE_ROOT.to_owned(),
            bin: None,
        }],
    }]
}

/// `cargo xtask try-new --self-test`.
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
        "try-new: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_new_fixture_reports_exactly_the_planted_findings() {
        let (missed, extra) = self_test_diff();
        assert!(missed.is_empty(), "missed: {missed:?}");
        assert!(extra.is_empty(), "false positives: {extra:?}");
    }

    #[test]
    fn try_new_list_rejects_a_type_listed_twice() {
        let error = parse_list(
            r#"
            reason = "r"
            [[allow]]
            crate = "a"
            type = "T"
            [[allow]]
            crate = "a"
            type = "T"
            "#,
        )
        .expect_err("a duplicate is an error");
        assert!(error.to_string().contains("listed twice"), "{error}");
    }

    #[test]
    fn try_new_seed_round_trips_through_the_list() {
        let (source, _) = fixture();
        let found =
            scan(&source, &fixture_crates()).expect("BUG: the fixture's module tree parses");
        let list = parse_list(&seed(&found)).expect("the seed parses");
        assert_eq!(check(&found, &list), Vec::new());
    }
}
