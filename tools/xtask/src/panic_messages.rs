//! `cargo xtask panic-messages`: a production `expect` or `panic!` names the
//! invariant it guards, and the count of the ones that do not only goes down.
//!
//! [`docs/PANIC-POLICY.md`] holds production panics to one bar: an `expect`
//! message starts with `BUG: ` and names the invariant, and so does a
//! `panic!`, unless it is the convenience twin of a `try_` form and names that
//! form. A user who hits such a panic then knows the fault is FLUI's, and the
//! panic report marks it `internal_invariant`. Clippy has no lint that reads a
//! message (`expect_used` is off on purpose), so this gate reads the syntax
//! tree.
//!
//! **What counts.** In the library and bin targets of every workspace member
//! ([`crate::source_tree`] follows each module tree the way rustc does; a
//! module or item under a cfg that is false in every build, `test`, is left
//! out, and so is a `#[test]` function; test support is left out too: an item
//! or file under `feature = "testing"` (ADR-0243), the `flui-testing` crate,
//! examples and `tools/`, where docs/PANIC-POLICY.md lets code panic freely):
//!
//! - a method call `.expect(…)` with one argument, unless the argument is a
//!   string literal that starts with `BUG: ` or names a `try_` form;
//! - a `panic!(…)` (any path ending in `panic`), unless its first token is
//!   such a string literal.
//!
//! A non-literal message (`expect(&message)`, `panic!(error)`) counts: the
//! gate cannot read it, and neither can the reader of the panic.
//!
//! **The list.** [`ALLOWLIST`] counts exactly, per file: a file above its
//! count is a finding ("grew"), one below it is a finding ("lower it"), and a
//! listed file with no site left is one. `--seed` prints the list the tree
//! needs.
//!
//! **Limits.** The gate reads source, not expansions: a site inside another
//! macro's arguments (`format!`, `assert!`, `thread_local!`) is out of its
//! reach, and so is an `expect` reached through a function pointer. Any
//! method named `expect` with one argument counts, whatever its receiver.
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).
//!
//! [`docs/PANIC-POLICY.md`]: ../../../docs/PANIC-POLICY.md

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::process::ExitCode;

use anyhow::Context;
use serde::Deserialize;
use syn::visit::{self, Visit};

use crate::source_tree::{Disk, Memory, Source, Target, Truth, cfg_of, walk};
use crate::try_new::{Krate, crates};
use crate::util;

/// The list, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/panic-messages.toml";

/// Arguments for `cargo xtask panic-messages`.
#[derive(Debug, clap::Args)]
pub(crate) struct PanicMessagesArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print the list the tree needs instead of checking it.
    #[arg(long, conflicts_with = "self_test")]
    seed: bool,
}

/// `cargo xtask panic-messages`. Exit code 1 lists every finding.
pub(crate) fn panic_messages(args: &PanicMessagesArgs) -> anyhow::Result<ExitCode> {
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
        let total: usize = list.values().sum();
        println!(
            "panic-messages: {total} unnamed panic message(s) left in {} file(s), each counted \
             in {ALLOWLIST}",
            list.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("panic-messages: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// One site whose message does not name its invariant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    line: usize,
    kind: Kind,
}

/// Which call the site is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Expect,
    Panic,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Expect => "`expect`",
            Self::Panic => "`panic!`",
        })
    }
}

/// The sites of every file that has one, keyed by repository-relative path.
type Found = BTreeMap<String, Vec<Site>>;

/// Every site in the crates' targets, each file once.
fn scan(source: &dyn Source, crates: &[Krate]) -> anyhow::Result<Found> {
    let mut found = Found::new();
    for krate in crates.iter().filter(|krate| !exempt_crate(krate)) {
        for target in &krate.targets {
            for file in walk(source, target)? {
                // A library and a bin of one package may share modules.
                // A file mounted under the `testing` feature is test support.
                if found.contains_key(&file.rel) || test_support(&file.cfgs) {
                    continue;
                }
                let mut visitor = Sites { sites: Vec::new() };
                visitor.visit_file(&file.ast);
                if !visitor.sites.is_empty() {
                    visitor.sites.sort();
                    found.insert(file.rel.clone(), visitor.sites);
                }
            }
        }
    }
    Ok(found)
}

/// Collects one file's sites, leaving out what a false cfg or `#[test]` hides.
struct Sites {
    sites: Vec<Site>,
}

/// Whether the attributes hide an item from every production build.
fn hidden(attrs: &[syn::Attribute]) -> bool {
    let (truth, texts) = cfg_of(attrs);
    truth == Truth::False
        || test_support(&texts)
        || attrs.iter().any(|attr| attr.path().is_ident("test"))
}

/// Whether cfg predicates mount test support: a crate's `testing` kit sits
/// behind `feature = "testing"` (ADR-0243), and test code panics freely
/// (docs/PANIC-POLICY.md).
fn test_support(cfgs: &[String]) -> bool {
    cfgs.iter()
        .any(|cfg| cfg.contains("feature = \"testing\"") && !cfg.contains("not"))
}

/// Whether a crate is test support or tooling as a whole: the `flui-testing`
/// kit, an example or a repository tool.
fn exempt_crate(krate: &Krate) -> bool {
    krate.name == "flui-testing"
        || krate
            .targets
            .iter()
            .all(|target| target.src.starts_with("examples/") || target.src.starts_with("tools/"))
}

impl<'ast> Visit<'ast> for Sites {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !hidden(&module.attrs) {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !hidden(&function.attrs) {
            visit::visit_item_fn(self, function);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !hidden(&item.attrs) {
            visit::visit_item_impl(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        if !hidden(&function.attrs) {
            visit::visit_impl_item_fn(self, function);
        }
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if !hidden(&function.attrs) {
            visit::visit_trait_item_fn(self, function);
        }
    }

    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        if !hidden(crate::source_tree::stmt_attrs(stmt)) {
            visit::visit_stmt(self, stmt);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "expect" && call.args.len() == 1 {
            let named = match call.args.first() {
                Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(message),
                    ..
                })) => names_invariant(&message.value()),
                _ => false,
            };
            if !named {
                self.sites.push(Site {
                    line: call.method.span().start().line,
                    kind: Kind::Expect,
                });
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "panic")
        {
            let named = mac
                .tokens
                .clone()
                .into_iter()
                .next()
                .and_then(|token| match token {
                    proc_macro2::TokenTree::Literal(literal) => match syn::Lit::new(literal) {
                        syn::Lit::Str(message) => Some(names_invariant(&message.value())),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap_or(false);
            if !named {
                self.sites.push(Site {
                    line: mac
                        .path
                        .segments
                        .last()
                        .map_or(0, |segment| segment.ident.span().start().line),
                    kind: Kind::Panic,
                });
            }
        }
        visit::visit_macro(self, mac);
    }
}

/// Whether a message meets PANIC-POLICY: an invariant (`BUG: `) or the `try_`
/// twin it is the convenience form of.
fn names_invariant(message: &str) -> bool {
    message.starts_with("BUG: ") || message.contains("try_")
}

/// The list as [`ALLOWLIST`] writes it: a count per file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFile {
    reason: String,
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One listed file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    count: usize,
}

/// The listed counts. A path listed twice or a zero count is an error.
fn parse_list(text: &str) -> anyhow::Result<BTreeMap<String, usize>> {
    let file: ListFile = toml::from_str(text)?;
    anyhow::ensure!(
        !file.reason.trim().is_empty(),
        "the list's `reason` is empty"
    );
    let mut counts = BTreeMap::new();
    for entry in file.allow {
        anyhow::ensure!(entry.count > 0, "`{}` is listed with count 0", entry.path);
        anyhow::ensure!(
            counts.insert(entry.path.clone(), entry.count).is_none(),
            "`{}` is listed twice",
            entry.path
        );
    }
    Ok(counts)
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// A file with more sites than its entry allows (none, if unlisted).
    Grew {
        path: String,
        listed: usize,
        sites: Vec<Site>,
    },
    /// A file with fewer sites than its entry, or none left.
    Lower {
        path: String,
        listed: usize,
        found: usize,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grew {
                path,
                listed,
                sites,
            } => {
                write!(
                    f,
                    "{path}: {} panic message(s) that name no invariant, {listed} listed; start \
                     each with `BUG: <invariant>` or name the `try_` twin (docs/PANIC-POLICY.md):",
                    sites.len()
                )?;
                for site in sites {
                    write!(f, " {}:{} {}", path, site.line, site.kind)?;
                }
                Ok(())
            }
            Self::Lower {
                path,
                listed,
                found,
            } => write!(
                f,
                "{path}: {found} unnamed panic message(s) left, {listed} listed; lower the count \
                 in {ALLOWLIST}"
            ),
        }
    }
}

/// Every file that grew and every entry above the tree.
fn check(found: &Found, list: &BTreeMap<String, usize>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (path, sites) in found {
        let listed = list.get(path).copied().unwrap_or(0);
        if sites.len() > listed {
            findings.push(Finding::Grew {
                path: path.clone(),
                listed,
                sites: sites.clone(),
            });
        }
    }
    for (path, &listed) in list {
        let found = found.get(path).map_or(0, Vec::len);
        if found < listed {
            findings.push(Finding::Lower {
                path: path.clone(),
                listed,
                found,
            });
        }
    }
    findings.sort();
    findings
}

/// The list the tree needs, in [`ALLOWLIST`]'s format.
fn seed(found: &Found) -> String {
    let mut out = String::from(
        "# Production `expect` and `panic!` sites whose message names no invariant, written by\n\
         # `cargo xtask panic-messages --seed`. A message starts with `BUG: <invariant>` or names\n\
         # the `try_` twin (docs/PANIC-POLICY.md). Counts are exact and only go down: the scan\n\
         # fails when a file grows past its entry or shrinks below it.\n\
         reason = \"written before the rule had a gate; each site gets a `BUG:` message or a typed error\"\n",
    );
    for (path, sites) in found {
        let _ = write!(
            out,
            "\n[[allow]]\npath = \"{path}\"\ncount = {}\n",
            sites.len()
        );
    }
    out
}

/// The self-test's crate root.
const FIXTURE_ROOT: &str = "fixture/src/lib.rs";

/// The self-test's crate: one planted site per shape the scan must see, one
/// hidden by each cfg and attribute it must skip, the messages it must accept,
/// and a list that undercounts one file and overcounts another.
fn fixture() -> (Memory, BTreeMap<String, usize>) {
    let lib = r#"
        mod host;
        #[cfg(test)]
        mod tests;

        pub fn planted(value: Option<u8>, result: Result<u8, ()>, message: &str) {
            value.expect("value is set");
            result.expect(message);
            core::panic!("boom {}", 1);
            panic!();
        }

        pub fn named(value: Option<u8>) {
            value.expect("BUG: the caller set the value");
            value.expect("no value; use try_value");
            panic!("BUG: unreachable state");
            panic!(r"handle is stale; try_get returns the error");
            // `expect` with two arguments is someone else's method.
            Other.expect(1, 2);
        }

        #[test]
        fn a_test() { None::<u8>.expect("in a test"); }

        #[cfg(test)]
        fn helper() { panic!("in a helper"); }

        pub fn statement() {
            #[cfg(test)]
            None::<u8>.expect("in a test statement");
        }

        #[cfg(feature = "testing")]
        pub mod kit { pub fn helper() { panic!("in the testing kit"); } }

        #[cfg(any(test, feature = "testing"))]
        pub fn kit_fn() { None::<u8>.expect("in a testing fn"); }

        struct Other;
        impl Other { fn expect(&self, _: u8, _: u8) {} }
    "#;
    let host = r#"
        pub fn listed() { None::<u8>.expect("listed"); }
    "#;
    let tests = r#"
        fn fake() { None::<u8>.expect("in tests"); }
    "#;
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
        path = "fixture/src/lib.rs"
        count = 3

        [[allow]]
        path = "fixture/src/host.rs"
        count = 2
        "#,
    )
    .expect("BUG: the fixture list parses");
    (source, list)
}

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 2] = [
    "fixture/src/lib.rs: 4 panic message(s) that name no invariant, 3 listed",
    "fixture/src/host.rs: 1 unnamed panic message(s) left, 2 listed",
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
        .filter(|head| !reported.iter().any(|line| line.starts_with(*head)))
        .map(|head| (*head).to_owned())
        .collect();
    let extra = reported
        .into_iter()
        .filter(|line| !EXPECTED.iter().any(|head| line.starts_with(head)))
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

/// `cargo xtask panic-messages --self-test`.
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
        "panic-messages: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_messages_fixture_reports_exactly_the_planted_findings() {
        let (missed, extra) = self_test_diff();
        assert!(missed.is_empty(), "missed: {missed:?}");
        assert!(extra.is_empty(), "false positives: {extra:?}");
    }

    #[test]
    fn panic_messages_fixture_names_each_planted_line() {
        let (source, _) = fixture();
        let found =
            scan(&source, &fixture_crates()).expect("BUG: the fixture's module tree parses");
        let lib = &found["fixture/src/lib.rs"];
        assert_eq!(
            lib.iter().map(|site| site.kind).collect::<Vec<_>>(),
            [Kind::Expect, Kind::Expect, Kind::Panic, Kind::Panic]
        );
    }

    #[test]
    fn panic_messages_seed_round_trips_through_the_list() {
        let (source, _) = fixture();
        let found =
            scan(&source, &fixture_crates()).expect("BUG: the fixture's module tree parses");
        let list = parse_list(&seed(&found)).expect("the seed parses");
        assert_eq!(check(&found, &list), Vec::new());
    }

    #[test]
    fn panic_messages_list_rejects_a_path_listed_twice() {
        let error = parse_list(
            r#"
            reason = "r"
            [[allow]]
            path = "a.rs"
            count = 1
            [[allow]]
            path = "a.rs"
            count = 2
            "#,
        )
        .expect_err("a duplicate is an error");
        assert!(error.to_string().contains("listed twice"), "{error}");
    }
}
