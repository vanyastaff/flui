//! One testing kit per layer, one switch name, exceptions kept as counted data (ADR-0243 D7).
//!
//! The gate reads test code and reports:
//!
//! - a Cargo feature named `test-support` or `test-utils` (the switch is `testing`);
//! - a `fn pump`, `fn advance` or `fn tick` in test code outside a kit;
//! - a type named `Mock*`, `Fake*`, `Scripted*`, `Recording*` or `Probe*` in test code outside a kit.
//!
//! Test code is a `tests/` or `benches/` target, a `#[cfg(test)]` module or item, an item behind a
//! `testing` feature, or a file only such a module includes ([`crate::file_length::modules`] walks
//! the module trees). Unconditional production items are never scanned, so a production type that
//! happens to be called `RecordingBudget` is not a finding. A kit is the `flui-testing` crate or a
//! module named `testing` under a crate's `src/`, and what it holds is exempt: it is where the
//! shared helpers belong.
//!
//! What exists today sits in [`ALLOWLIST`] as exact helper names per file and kind, with one reason and
//! one exit for all of them ([`crate::ratchet`]). A name the tree has and the entry lacks ("grew"), and one
//! the entry has and the tree lacks ("lower it"), are both findings, as are an entry whose file no longer has the kind and an
//! unknown kind. Exit 1 with one line per finding. `--self-test` runs the planted rows of
//! [`rows`]; `--seed` prints the allowlist the tree needs.
//!
//! Limits, not coverage: modules declared inside a macro invocation are invisible to `syn`, and a
//! type produced by a macro is not seen.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use serde::Deserialize;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ImplItem, Item, Local, Meta, TraitItem};

use crate::file_length::modules::{self, is_test_only, item_attrs};
use crate::ratchet::Exits;
use crate::util::{ScratchDir, repo_root};

/// The allowlist, relative to the repository root.
pub(crate) const ALLOWLIST: &str = "tools/xtask/allowlists/testing.toml";

/// Feature names that expose a crate's test support and are not `testing`.
const OLD_FEATURES: [&str; 2] = ["test-support", "test-utils"];

/// Feature names read as "the testing feature" when deciding whether an item is test code.
const TESTING_FEATURES: [&str; 3] = ["testing", "test-support", "test-utils"];

/// Function names that drive time or frames by hand.
const PUMP_NAMES: [&str; 3] = ["pump", "advance", "tick"];

/// Type-name prefixes of test doubles and probes.
const DOUBLE_PREFIXES: [&str; 5] = ["Mock", "Fake", "Scripted", "Recording", "Probe"];

/// Arguments for `cargo xtask testing`.
#[derive(Debug, clap::Args)]
pub(crate) struct TestingArgs {
    /// Run the planted rows instead of scanning the workspace.
    #[arg(long, conflicts_with = "seed")]
    self_test: bool,
    /// Print the allowlist the workspace needs (to stdout; no file is written).
    #[arg(long)]
    seed: bool,
}

/// What an excepted thing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    /// A `fn pump|advance|tick`.
    Pump,
    /// A `Mock*|Fake*|Scripted*|Recording*|Probe*` type.
    Double,
    /// A Cargo feature named `test-support` or `test-utils`.
    Feature,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Pump => "pump",
            Self::Double => "double",
            Self::Feature => "feature",
        }
    }
}

/// One thing the gate cares about, found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hit {
    /// Repository-relative, `/`-separated.
    path: String,
    kind: Kind,
    name: String,
    line: usize,
}

/// `cargo xtask testing`.
pub(crate) fn testing(args: &TestingArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = repo_root();
    let meta = crate::util::metadata(&root)?;
    let (mut hits, problems) =
        collect_sources(&root, &targets(&meta, false), &targets(&meta, true));
    hits.extend(feature_hits(&root, &meta));
    if args.seed {
        print!("{}", seed(&hits));
        return Ok(ExitCode::SUCCESS);
    }
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST)?)
        .with_context(|| format!("parsing {ALLOWLIST}"))?;
    let exits = Exits::from_repo(&root)?;
    let mut findings: Vec<Finding> = problems.into_iter().map(Finding::Walk).collect();
    findings.extend(judge(&hits, &allow, &exits));
    for finding in &findings {
        println!("{finding}");
    }
    if !findings.is_empty() {
        eprintln!(
            "testing: {} finding(s); allowlist {ALLOWLIST}",
            findings.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "testing: {} excepted item(s) in {} allowlist entries, none unlisted",
        hits.len(),
        allow.allow.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// The target root files of the workspace: test and bench targets when `tests`, else every target
/// a non-test build compiles, examples included (their `#[cfg(test)]` modules are test code).
fn targets(meta: &cargo_metadata::Metadata, tests: bool) -> Vec<PathBuf> {
    const PRODUCTION: [&str; 9] = [
        "lib",
        "rlib",
        "dylib",
        "cdylib",
        "staticlib",
        "proc-macro",
        "bin",
        "example",
        "custom-build",
    ];
    let mut roots = Vec::new();
    for package in meta.workspace_packages() {
        for target in &package.targets {
            let gated = target
                .required_features
                .iter()
                .any(|feature| TESTING_FEATURES.contains(&feature.as_str()));
            let wanted = target.kind.iter().any(|kind| {
                let kind = kind.to_string();
                if tests {
                    kind == "test" || kind == "bench" || gated
                } else {
                    PRODUCTION.contains(&kind.as_str()) && !gated
                }
            });
            if wanted {
                roots.push(PathBuf::from(target.src_path.as_std_path()));
            }
        }
    }
    roots
}

/// The `test-support` / `test-utils` features the workspace packages declare.
fn feature_hits(root: &Path, meta: &cargo_metadata::Metadata) -> Vec<Hit> {
    let mut hits = Vec::new();
    for package in meta.workspace_packages() {
        for name in package.features.keys() {
            if OLD_FEATURES.contains(&name.as_str()) {
                hits.push(Hit {
                    path: relative(root, package.manifest_path.as_std_path()),
                    kind: Kind::Feature,
                    name: name.clone(),
                    line: 0,
                });
            }
        }
    }
    hits
}

/// Hits in the test code reachable from the `production` and `test` target roots, and the module
/// problems the walks met.
fn collect_sources(
    root: &Path,
    production: &[PathBuf],
    test: &[PathBuf],
) -> (Vec<Hit>, Vec<String>) {
    let prod_tree = modules::walk_scoped(production.iter().cloned(), implies_testing_attrs);
    let test_tree = modules::walk_scoped(test.iter().cloned(), implies_testing_attrs);
    // path -> whether the whole file is test code
    let mut files: BTreeMap<PathBuf, bool> = BTreeMap::new();
    for path in prod_tree.modules.keys() {
        files.entry(path.clone()).or_insert(false);
    }
    for path in &prod_tree.test_files {
        files.insert(path.clone(), true);
    }
    // a file a test target shares with a production target is production code there: only its
    // test-scoped items count
    for path in test_tree.modules.keys().chain(&test_tree.test_files) {
        files.entry(path.clone()).or_insert(true);
    }
    let prefix = format!("{}{}", root.display(), std::path::MAIN_SEPARATOR);
    let mut problems: Vec<String> = test_tree
        .problems
        .iter()
        .chain(&prod_tree.problems)
        .map(|problem| problem.to_string().replace(&prefix, "").replace('\\', "/"))
        .collect();
    problems.sort();
    problems.dedup();
    let mut hits = Vec::new();
    for (path, whole) in &files {
        let rel = relative(root, path);
        if in_kit_file(root, &rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        hits.extend(scan_file(root, &rel, &text, *whole));
    }
    (hits, problems)
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What follows the `src/` of the package that owns `rel`: `Some` only when the directory before
/// that `src/` holds a `Cargo.toml`, so a `src/` folder inside a test target is not a source root.
fn package_source_rest<'a>(root: &Path, rel: &'a str) -> Option<&'a str> {
    let mut start = 0;
    loop {
        let (prefix, rest) = if start == 0 && rel.starts_with("src/") {
            ("", &rel["src/".len()..])
        } else {
            let at = rel[start..].find("/src/")? + start;
            (&rel[..at], &rel[at + "/src/".len()..])
        };
        if root.join(prefix).join("Cargo.toml").is_file() {
            return Some(rest);
        }
        start = rel.len() - rest.len();
    }
}

/// Whether the whole file is a kit: the `flui-testing` crate or a `testing` module in the `src/`
/// of a package.
fn in_kit_file(root: &Path, rel: &str) -> bool {
    rel.starts_with("crates/flui-testing/")
        || package_source_rest(root, rel)
            .is_some_and(|rest| rest == "testing.rs" || rest.starts_with("testing/"))
}
/// The hits in one file; `whole` when every item in it is test code.
fn scan_file(root: &Path, rel: &str, text: &str, whole: bool) -> Vec<Hit> {
    let parsed = syn::parse_file(text);
    // spans are per-thread and grow with every parse; nothing holds one past the visit
    let hits = match parsed {
        Ok(file) => {
            let mut scan = Scan {
                path: rel.to_owned(),
                in_scope: whole || implies_testing_attrs(&file.attrs),
                in_kit: false,
                kit_modules: package_source_rest(root, rel).is_some(),
                hits: Vec::new(),
            };
            for item in &file.items {
                scan.visit_item(item);
            }
            scan.hits
        }
        // a file that does not parse is the build's problem, not this gate's
        Err(_) => Vec::new(),
    };
    proc_macro2::extra::invalidate_current_thread_spans();
    hits
}

/// Collects the hits of one file's test-code items.
struct Scan {
    path: String,
    /// Inside test code.
    in_scope: bool,
    /// Whether a module named `testing` is a kit here: only under a crate's `src/`, never in a
    /// test target, whose local `testing` module is just a module.
    kit_modules: bool,
    /// Inside a kit module.
    in_kit: bool,
    hits: Vec<Hit>,
}

impl Scan {
    fn record(&mut self, kind: Kind, ident: &syn::Ident) {
        if !self.in_scope || self.in_kit {
            return;
        }
        let name = ident.unraw().to_string();
        let matched = match kind {
            Kind::Pump => PUMP_NAMES.contains(&name.as_str()),
            Kind::Double => is_double_name(&name),
            Kind::Feature => false,
        };
        if matched {
            self.hits.push(Hit {
                path: self.path.clone(),
                kind,
                name,
                line: ident.span().start().line,
            });
        }
    }

    /// Runs `body` with the scope widened by `attrs`.
    fn scoped(&mut self, attrs: &[Attribute], body: impl FnOnce(&mut Self)) {
        let before = self.in_scope;
        self.in_scope |= implies_testing_attrs(attrs);
        body(self);
        self.in_scope = before;
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item(&mut self, item: &'ast Item) {
        self.scoped(item_attrs(item), |this| {
            match item {
                Item::Struct(i) => this.record(Kind::Double, &i.ident),
                Item::Enum(i) => this.record(Kind::Double, &i.ident),
                Item::Union(i) => this.record(Kind::Double, &i.ident),
                Item::Trait(i) => this.record(Kind::Double, &i.ident),
                Item::Type(i) => this.record(Kind::Double, &i.ident),
                Item::Fn(i) => this.record(Kind::Pump, &i.sig.ident),
                _ => {}
            }
            if let Item::Mod(module) = item
                && this.kit_modules
                && module.ident.unraw() == "testing"
            {
                let before = this.in_kit;
                this.in_kit = true;
                visit::visit_item(this, item);
                this.in_kit = before;
            } else {
                visit::visit_item(this, item);
            }
        });
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        let attrs: &[Attribute] = match expr {
            Expr::Block(e) => &e.attrs,
            Expr::Unsafe(e) => &e.attrs,
            _ => &[],
        };
        self.scoped(attrs, |this| visit::visit_expr(this, expr));
    }

    fn visit_local(&mut self, local: &'ast Local) {
        self.scoped(&local.attrs, |this| visit::visit_local(this, local));
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs: &[Attribute] = match item {
            ImplItem::Fn(i) => &i.attrs,
            ImplItem::Type(i) => &i.attrs,
            ImplItem::Const(i) => &i.attrs,
            _ => &[],
        };
        self.scoped(attrs, |this| {
            match item {
                ImplItem::Fn(function) => this.record(Kind::Pump, &function.sig.ident),
                ImplItem::Type(ty) => this.record(Kind::Double, &ty.ident),
                _ => {}
            }
            visit::visit_impl_item(this, item);
        });
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let attrs: &[Attribute] = match item {
            TraitItem::Fn(i) => &i.attrs,
            TraitItem::Type(i) => &i.attrs,
            TraitItem::Const(i) => &i.attrs,
            _ => &[],
        };
        self.scoped(attrs, |this| {
            match item {
                TraitItem::Fn(function) => this.record(Kind::Pump, &function.sig.ident),
                TraitItem::Type(ty) => this.record(Kind::Double, &ty.ident),
                _ => {}
            }
            visit::visit_trait_item(this, item);
        });
    }
}

/// `Mock`, `FakeWindow`, `Probe2`: a prefix followed by the end, an uppercase letter, a digit or `_`.
fn is_double_name(name: &str) -> bool {
    DOUBLE_PREFIXES.iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
    })
}

/// Whether one of `attrs` is a `cfg` that holds only in test builds or with a testing feature.
fn implies_testing_attrs(attrs: &[Attribute]) -> bool {
    is_test_only(attrs)
        || attrs.iter().any(|attr| {
            is_test_function_attr(attr.path())
                || attr.path().is_ident("cfg")
                    && attr
                        .parse_args::<Meta>()
                        .is_ok_and(|predicate| implies_testing_feature(&predicate))
        })
}

/// `#[test]`, `#[bench]` and the runtime-flavoured forms (`#[tokio::test]`): the item is a test
/// function, so what it declares inside is test code.
fn is_test_function_attr(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == "test" || segment.ident == "bench")
}

/// Whether the `cfg` predicate can hold only with a testing feature on (or under `cfg(test)`).
fn implies_testing_feature(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::NameValue(nv) => {
            nv.path.is_ident("feature")
                && matches!(
                    &nv.value,
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })
                        if TESTING_FEATURES.contains(&s.value().as_str())
                )
        }
        Meta::List(list) => {
            let Ok(operands) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                operands.iter().any(implies_testing_feature)
            } else if list.path.is_ident("any") {
                !operands.is_empty() && operands.iter().all(implies_testing_feature)
            } else {
                false
            }
        }
    }
}

/// The allowlist file: one reason and one exit for every entry.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    /// Why the entries exist.
    #[serde(default)]
    reason: String,
    /// The ADR whose change removes them.
    #[serde(default)]
    exit: String,
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One file's exact helpers of one kind.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Repository-relative, `/`-separated.
    path: String,
    kind: Kind,
    /// The names of that kind the file has today, one per occurrence.
    names: Vec<String>,
}

impl Allowlist {
    fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }
}

/// One way the tree and the allowlist disagree with the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    Unlisted {
        path: String,
        kind: Kind,
        count: usize,
        names: String,
    },
    Grew {
        path: String,
        kind: Kind,
        added: String,
    },
    Shrank {
        path: String,
        kind: Kind,
        removed: String,
    },
    Gone {
        path: String,
        kind: Kind,
    },
    Duplicate {
        path: String,
        kind: Kind,
    },
    NoReason,
    BadExit {
        exit: String,
        why: String,
    },
    Walk(String),
}

impl Finding {
    /// `(path, kind of finding)`: what the rows compare.
    fn identity(&self) -> (String, &'static str) {
        match self {
            Self::Unlisted { path, .. } => (path.clone(), "unlisted"),
            Self::Grew { path, .. } => (path.clone(), "grew"),
            Self::Shrank { path, .. } => (path.clone(), "shrank"),
            Self::Gone { path, .. } => (path.clone(), "gone"),
            Self::Duplicate { path, .. } => (path.clone(), "duplicate"),
            Self::NoReason => (String::new(), "no-reason"),
            Self::BadExit { .. } => (String::new(), "bad-exit"),
            Self::Walk(line) => (
                line.split(':').next().unwrap_or_default().to_owned(),
                "walk",
            ),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted {
                path,
                kind,
                count,
                names,
            } => write!(
                f,
                "{path}: {count} {} not in a kit and not allowlisted ({names}): take it from \
                 the layer's kit (ADR-0243), or move it into the kit",
                kind.label()
            ),
            Self::Grew { path, kind, added } => write!(
                f,
                "{path}: {} {added} not granted by the allowlist: use the kit instead of \
                 adding to the entry",
                kind.label()
            ),
            Self::Shrank {
                path,
                kind,
                removed,
            } => write!(
                f,
                "{path}: {} {removed} no longer found: remove it from `names` in {ALLOWLIST}",
                kind.label()
            ),
            Self::Gone { path, kind } => write!(
                f,
                "{path}: no {} left: remove its entry from {ALLOWLIST}",
                kind.label()
            ),
            Self::Duplicate { path, kind } => write!(
                f,
                "{path}: {} allowlisted twice in {ALLOWLIST}",
                kind.label()
            ),
            Self::NoReason => write!(f, "{ALLOWLIST}: `reason` is empty"),
            Self::BadExit { exit, why } => write!(f, "{ALLOWLIST}: exit {exit:?}: {why}"),
            Self::Walk(problem) => write!(f, "{problem}"),
        }
    }
}

/// Every finding of `hits` against `allow`.
fn judge(hits: &[Hit], allow: &Allowlist, exits: &Exits) -> Vec<Finding> {
    let mut findings = Vec::new();
    if allow.reason.trim().is_empty() {
        findings.push(Finding::NoReason);
    }
    if let Err(why) = exits.check(&allow.exit) {
        findings.push(Finding::BadExit {
            exit: allow.exit.clone(),
            why,
        });
    }
    let mut seen: BTreeMap<(&str, Kind), Vec<&Hit>> = BTreeMap::new();
    for hit in hits {
        seen.entry((hit.path.as_str(), hit.kind))
            .or_default()
            .push(hit);
    }
    let mut entries: BTreeSet<(&str, Kind)> = BTreeSet::new();
    for entry in &allow.allow {
        if !entries.insert((entry.path.as_str(), entry.kind)) {
            findings.push(Finding::Duplicate {
                path: entry.path.clone(),
                kind: entry.kind,
            });
            continue;
        }
        let path = entry.path.clone();
        let kind = entry.kind;
        let Some(found) = seen.get(&(entry.path.as_str(), entry.kind)) else {
            findings.push(Finding::Gone { path, kind });
            continue;
        };
        let today: Vec<String> = found.iter().map(|hit| hit.name.clone()).collect();
        let added = missing_from(&today, &entry.names);
        let removed = missing_from(&entry.names, &today);
        if !added.is_empty() {
            findings.push(Finding::Grew {
                path: path.clone(),
                kind,
                added: added.join(", "),
            });
        }
        if !removed.is_empty() {
            findings.push(Finding::Shrank {
                path,
                kind,
                removed: removed.join(", "),
            });
        }
    }
    for ((path, kind), found) in &seen {
        if !entries.contains(&(*path, *kind)) {
            findings.push(Finding::Unlisted {
                path: (*path).to_owned(),
                kind: *kind,
                count: found.len(),
                names: names(found),
            });
        }
    }
    findings
}

/// The first few `name:line` of `found`, for a report line.
fn names(found: &[&Hit]) -> String {
    let mut shown: Vec<String> = found
        .iter()
        .take(3)
        .map(|hit| {
            if hit.line == 0 {
                hit.name.clone()
            } else {
                format!("{}:{}", hit.name, hit.line)
            }
        })
        .collect();
    if found.len() > 3 {
        shown.push("…".to_owned());
    }
    shown.join(", ")
}

/// The allowlist `hits` need, as TOML; `reason` and `exit` left for a person.
fn seed(hits: &[Hit]) -> String {
    let mut names: BTreeMap<(&str, Kind), Vec<&str>> = BTreeMap::new();
    for hit in hits {
        names
            .entry((hit.path.as_str(), hit.kind))
            .or_default()
            .push(hit.name.as_str());
    }
    let mut out = String::from(
        "# Test helpers outside a kit (ADR-0243), written by `cargo xtask testing --seed`.\n\
         # The names are exact and only leave. `reason` and `exit` apply to every entry.\n\
         reason = \"\"\nexit = \"\"\n",
    );
    for ((path, kind), mut names) in names {
        names.sort_unstable();
        let names: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
        let _ = write!(
            out,
            "\n[[allow]]\npath = \"{path}\"\nkind = \"{}\"\nnames = [{}]\n",
            kind.label(),
            names.join(", ")
        );
    }
    out
}

/// The elements of `left` that `right` does not account for, as multisets.
fn missing_from(left: &[String], right: &[String]) -> Vec<String> {
    let mut spare: BTreeMap<&str, usize> = BTreeMap::new();
    for name in right {
        *spare.entry(name.as_str()).or_default() += 1;
    }
    let mut missing: Vec<String> = left
        .iter()
        .filter(|name| match spare.get_mut(name.as_str()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect();
    missing.sort_unstable();
    missing
}

/// The exits the rows resolve against.
fn row_exits() -> Exits {
    Exits::from_adrs(["0243".to_owned()])
}

/// Writes `files` under a fresh directory and scans them as a workspace would:
/// `src/lib.rs` is the production root, each `tests/*.rs` a test target.
fn findings_for(
    files: &[(&str, &str)],
    features: &[(&str, &str)],
    allowlist: &str,
) -> Vec<(String, &'static str)> {
    let dir = ScratchDir::try_new("testing").expect("scratch");
    let root = dir.path();
    std::fs::write(root.join("Cargo.toml"), "").expect("writes");
    for (path, text) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().expect("BUG: rows plant files in directories"))
            .expect("creates");
        std::fs::write(&full, text).expect("writes");
    }
    let tests: Vec<PathBuf> = files
        .iter()
        .filter(|(path, _)| path.starts_with("tests/") && path.matches('/').count() == 1)
        .map(|(path, _)| root.join(path))
        .collect();
    let (mut hits, problems) = collect_sources(root, &[root.join("src/lib.rs")], &tests);
    hits.extend(features.iter().map(|&(path, name)| Hit {
        path: path.to_owned(),
        kind: Kind::Feature,
        name: name.to_owned(),
        line: 0,
    }));
    let allow = Allowlist::parse(allowlist).expect("the row's allowlist parses");
    let mut findings: Vec<Finding> = problems.into_iter().map(Finding::Walk).collect();
    findings.extend(judge(&hits, &allow, &row_exits()));
    findings.iter().map(Finding::identity).collect()
}

/// A valid empty allowlist header.
const HEADER: &str = "reason = \"planted\"\nexit = \"ADR-0243\"\n";

fn allowlist(entries: &str) -> String {
    format!("{HEADER}{entries}")
}

const PUMP_IN_TESTS: &str = "fn pump() {}\n#[test]\nfn works() {}\n";

fn row_listed_helper_passes() {
    let allow =
        allowlist("[[allow]]\npath = \"tests/a.rs\"\nkind = \"pump\"\nnames = [\"pump\"]\n");
    let found = findings_for(
        &[("src/lib.rs", ""), ("tests/a.rs", PUMP_IN_TESTS)],
        &[],
        &allow,
    );
    assert_eq!(found, []);
}

fn row_unlisted_helper_in_tests_fails() {
    let found = findings_for(
        &[("src/lib.rs", ""), ("tests/a.rs", PUMP_IN_TESTS)],
        &[],
        HEADER,
    );
    assert_eq!(found, [("tests/a.rs".to_owned(), "unlisted")]);
}

fn row_unlisted_helper_in_cfg_test_module_fails() {
    let lib = "#[cfg(test)]\nmod tests {\n    fn advance() {}\n    struct FakeClock;\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(
        found,
        [
            ("src/lib.rs".to_owned(), "unlisted"),
            ("src/lib.rs".to_owned(), "unlisted"),
        ]
    );
}

fn row_test_module_in_its_own_file_fails() {
    let found = findings_for(
        &[
            ("src/lib.rs", "#[cfg(test)]\nmod tests;\n"),
            ("src/tests.rs", "struct ScriptedSink;\n"),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, [("src/tests.rs".to_owned(), "unlisted")]);
}

fn row_module_behind_the_testing_feature_in_its_own_file_fails() {
    let found = findings_for(
        &[
            (
                "src/lib.rs",
                "#[cfg(feature = \"testing\")]\nmod fixtures;\n",
            ),
            ("src/fixtures.rs", "fn pump() {}\n"),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, [("src/fixtures.rs".to_owned(), "unlisted")]);
}

fn row_old_feature_name_fails() {
    let found = findings_for(
        &[("src/lib.rs", "")],
        &[("crates/x/Cargo.toml", "test-support")],
        HEADER,
    );
    assert_eq!(found, [("crates/x/Cargo.toml".to_owned(), "unlisted")]);
}

fn row_double_behind_the_testing_feature_fails() {
    let lib = "#[cfg(feature = \"testing\")]\npub struct RecordingSink;\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(found, [("src/lib.rs".to_owned(), "unlisted")]);
}

fn row_unconditional_production_type_is_ignored() {
    let lib = "pub(crate) struct RecordingBudget;\nfn tick() {}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(found, []);
}

fn row_the_kit_is_exempt() {
    let lib = "#[cfg(feature = \"testing\")]\npub mod testing;\n\
               #[cfg(test)]\nmod inline {\n    pub mod testing {\n        pub struct FakeClock;\n    }\n}\n";
    let found = findings_for(
        &[
            ("src/lib.rs", lib),
            (
                "src/testing.rs",
                "pub struct ScriptedSink;\npub fn pump() {}\n",
            ),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, []);
}

fn row_names_match_on_a_word_boundary() {
    let lib = "#[cfg(test)]\nmod tests {\n    struct Probed;\n    struct Mockingbird;\n    struct Fake;\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    // only `Fake` is a double; `Probed` and `Mockingbird` merely start with a prefix
    assert_eq!(found, [("src/lib.rs".to_owned(), "unlisted")]);
}

fn row_swapping_a_listed_helper_for_another_is_a_finding() {
    let allow =
        allowlist("[[allow]]\npath = \"tests/a.rs\"\nkind = \"pump\"\nnames = [\"pump\"]\n");
    let found = findings_for(
        &[("src/lib.rs", ""), ("tests/a.rs", "fn tick() {}\n")],
        &[],
        &allow,
    );
    assert_eq!(
        found,
        [
            ("tests/a.rs".to_owned(), "grew"),
            ("tests/a.rs".to_owned(), "shrank")
        ]
    );
}

fn row_a_testing_module_in_a_test_target_is_not_a_kit() {
    let found = findings_for(
        &[
            ("src/lib.rs", ""),
            ("tests/a.rs", "mod testing {\n    fn pump() {}\n}\n"),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, [("tests/a.rs".to_owned(), "unlisted")]);
}

fn row_items_inside_a_test_function_are_test_code() {
    let lib = "#[test]\nfn case() {\n    fn pump() {}\n    struct FakeClock;\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(
        found,
        [
            ("src/lib.rs".to_owned(), "unlisted"),
            ("src/lib.rs".to_owned(), "unlisted"),
        ]
    );
}

fn row_raw_identifiers_match_their_plain_names() {
    let lib = "#[cfg(test)]\nmod t {\n    fn r#pump() {}\n    struct r#FakeClock;\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(
        found,
        [
            ("src/lib.rs".to_owned(), "unlisted"),
            ("src/lib.rs".to_owned(), "unlisted"),
        ]
    );
}

fn row_items_in_a_test_only_block_are_test_code() {
    let lib = "fn run() {\n    #[cfg(test)]\n    {\n        fn pump() {}\n        struct FakeClock;\n    }\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert_eq!(
        found,
        [
            ("src/lib.rs".to_owned(), "unlisted"),
            ("src/lib.rs".to_owned(), "unlisted"),
        ]
    );
}

fn row_a_file_shared_with_production_keeps_production_scope() {
    let found = findings_for(
        &[
            ("src/lib.rs", "mod shared;\n"),
            (
                "src/shared.rs",
                "fn tick() {}\n#[cfg(test)]\nmod t {\n    fn pump() {}\n}\n",
            ),
            (
                "tests/a.rs",
                "#[path = \"../src/shared.rs\"]\nmod shared;\n",
            ),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, [("src/shared.rs".to_owned(), "unlisted")]);
}

fn row_a_raw_testing_module_is_a_kit() {
    let lib = "#[cfg(test)]\nmod r#testing {\n    struct FakeClock;\n}\n";
    let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
    assert!(found.is_empty(), "{found:?}");
}

fn row_associated_types_are_doubles() {
    let in_trait =
        "#[cfg(test)]\nmod t {\n    trait Fixture {\n        type FakeClock;\n    }\n}\n";
    let in_impl = "#[cfg(test)]\nmod t {\n    struct S;\n    impl Iterator for S {\n        type Item = u8;\n        type FakeClock = u8;\n    }\n}\n";
    for lib in [in_trait, in_impl] {
        let found = findings_for(&[("src/lib.rs", lib)], &[], HEADER);
        assert!(
            found.iter().any(|(_, kind)| *kind == "unlisted"),
            "{lib}: {found:?}"
        );
    }
}

fn row_a_src_folder_inside_a_test_target_is_not_a_kit() {
    let found = findings_for(
        &[
            ("src/lib.rs", ""),
            (
                "tests/a.rs",
                "#[path = \"a/src/testing.rs\"]\nmod helpers;\n",
            ),
            ("tests/a/src/testing.rs", "fn advance() {}\n"),
        ],
        &[],
        HEADER,
    );
    assert_eq!(found, [("tests/a/src/testing.rs".to_owned(), "unlisted")]);
}

fn row_count_grew() {
    let allow =
        allowlist("[[allow]]\npath = \"tests/a.rs\"\nkind = \"pump\"\nnames = [\"pump\"]\n");
    let two = "fn pump() {}\nfn tick() {}\n";
    let found = findings_for(&[("src/lib.rs", ""), ("tests/a.rs", two)], &[], &allow);
    assert_eq!(found, [("tests/a.rs".to_owned(), "grew")]);
}

fn row_count_fell() {
    let allow = allowlist(
        "[[allow]]\npath = \"tests/a.rs\"\nkind = \"pump\"\nnames = [\"pump\", \"tick\"]\n",
    );
    let found = findings_for(
        &[("src/lib.rs", ""), ("tests/a.rs", PUMP_IN_TESTS)],
        &[],
        &allow,
    );
    assert_eq!(found, [("tests/a.rs".to_owned(), "shrank")]);
}

fn row_entry_with_nothing_left_is_removed() {
    let allow =
        allowlist("[[allow]]\npath = \"tests/a.rs\"\nkind = \"double\"\nnames = [\"FakeClock\"]\n");
    let found = findings_for(
        &[("src/lib.rs", ""), ("tests/a.rs", PUMP_IN_TESTS)],
        &[],
        &allow,
    );
    assert_eq!(
        found,
        [
            ("tests/a.rs".to_owned(), "gone"),
            ("tests/a.rs".to_owned(), "unlisted")
        ]
    );
}

fn row_allowlist_without_a_reason_or_exit_is_refused() {
    let found = findings_for(&[("src/lib.rs", "")], &[], "reason = \"\"\nexit = \"\"\n");
    assert_eq!(
        found,
        [(String::new(), "no-reason"), (String::new(), "bad-exit")]
    );
}

/// The planted rows: `cargo xtask testing --self-test` and `testing_gate_fixture_rows` run them.
fn rows() -> Vec<(&'static str, fn())> {
    vec![
        ("listed_helper_passes", row_listed_helper_passes as fn()),
        (
            "unlisted_helper_in_tests_fails",
            row_unlisted_helper_in_tests_fails,
        ),
        (
            "unlisted_helper_in_cfg_test_module_fails",
            row_unlisted_helper_in_cfg_test_module_fails,
        ),
        (
            "test_module_in_its_own_file_fails",
            row_test_module_in_its_own_file_fails,
        ),
        (
            "module_behind_the_testing_feature_in_its_own_file_fails",
            row_module_behind_the_testing_feature_in_its_own_file_fails,
        ),
        ("old_feature_name_fails", row_old_feature_name_fails),
        (
            "double_behind_the_testing_feature_fails",
            row_double_behind_the_testing_feature_fails,
        ),
        (
            "unconditional_production_type_is_ignored",
            row_unconditional_production_type_is_ignored,
        ),
        ("the_kit_is_exempt", row_the_kit_is_exempt),
        (
            "names_match_on_a_word_boundary",
            row_names_match_on_a_word_boundary,
        ),
        (
            "swapping_a_listed_helper_for_another_is_a_finding",
            row_swapping_a_listed_helper_for_another_is_a_finding,
        ),
        (
            "a_testing_module_in_a_test_target_is_not_a_kit",
            row_a_testing_module_in_a_test_target_is_not_a_kit,
        ),
        (
            "items_inside_a_test_function_are_test_code",
            row_items_inside_a_test_function_are_test_code,
        ),
        (
            "raw_identifiers_match_their_plain_names",
            row_raw_identifiers_match_their_plain_names,
        ),
        (
            "items_in_a_test_only_block_are_test_code",
            row_items_in_a_test_only_block_are_test_code,
        ),
        (
            "a_file_shared_with_production_keeps_production_scope",
            row_a_file_shared_with_production_keeps_production_scope,
        ),
        (
            "a_raw_testing_module_is_a_kit",
            row_a_raw_testing_module_is_a_kit,
        ),
        (
            "associated_types_are_doubles",
            row_associated_types_are_doubles,
        ),
        (
            "a_src_folder_inside_a_test_target_is_not_a_kit",
            row_a_src_folder_inside_a_test_target_is_not_a_kit,
        ),
        ("count_grew", row_count_grew),
        ("count_fell", row_count_fell),
        (
            "entry_with_nothing_left_is_removed",
            row_entry_with_nothing_left_is_removed,
        ),
        (
            "allowlist_without_a_reason_or_exit_is_refused",
            row_allowlist_without_a_reason_or_exit_is_refused,
        ),
    ]
}

/// `cargo xtask testing --self-test`.
fn self_test() -> ExitCode {
    let rows = rows();
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(*row).is_err())
        .map(|(name, _)| *name)
        .collect();
    for name in &failed {
        println!("self-test: row {name} FAILED");
    }
    if !failed.is_empty() {
        return ExitCode::FAILURE;
    }
    println!("testing: self-test ok ({} rows)", rows.len());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testing_gate_fixture_rows() {
        crate::table_test::run_table("testing_gate_fixture_rows", &rows());
    }

    #[test]
    fn the_real_allowlist_parses_and_names_a_real_exit() {
        let allow =
            Allowlist::parse(&crate::util::read(ALLOWLIST).expect("reads")).expect("parses");
        let exits = Exits::from_repo(&repo_root()).expect("exits");
        assert_eq!(exits.check(&allow.exit), Ok(()));
        assert!(allow.allow.iter().all(|entry| !entry.names.is_empty()));
    }
}
