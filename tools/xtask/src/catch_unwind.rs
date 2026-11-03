//! `cargo xtask catch-unwind`: hand-written `catch_unwind` in production code is
//! counted per file, and the count only goes down (ADR-0287).
//!
//! An owner that changes its state and calls code it does not own goes through
//! `flui_foundation::panic` (`commit`, `FirstPanic`, `PanicRecovery`): the order
//! and the containment live there once. Any other `catch_unwind` is an owner
//! ordering that by hand, the shape ADR-0287 retires one seam at a time, so the
//! sites are counted per file and a new one is a finding until its code goes
//! through the primitives.
//!
//! **What counts.** In the production module trees of the workspace packages
//! under `crates/`, `packages/` and `src/` (every target a non-test build
//! compiles; examples and targets behind a testing feature left out): a path
//! whose last segment is `catch_unwind` (`std::panic::catch_unwind(..)`, an
//! imported `catch_unwind(..)`, the function passed as a value), a
//! `.catch_unwind()` method call (`FutureExt`), and a `catch_unwind` identifier
//! in a macro's tokens (a `macro_rules!` body, an `assert!`), which `syn` does
//! not parse as code. A `use` of the name is not a site. Test code is left out:
//! a `#[cfg(test)]` module or item, an item behind a `testing` feature, a
//! `#[test]` function, and a file only such a module includes
//! ([`crate::file_length::modules`] walks the trees).
//!
//! **Where catching is the job.** [`BOUNDARIES`] are left out: the primitives
//! themselves and the OS entry of `flui-native`. So are the kits
//! (`flui-testing`, a crate's `testing` module), which are test support.
//!
//! **The list.** [`ALLOWLIST`] counts exactly per file, with one reason and one
//! exit for every entry ([`crate::ratchet`]): a count above the tree is a
//! finding ("grew"), one below it is ("lower it"), and so is a listed path with
//! no site left. `--seed` prints the list the tree needs.
//!
//! Limits, not coverage: a module declared inside a macro invocation is not
//! walked, and a `#[cfg(test)]` statement or expression inside production code
//! is not an item, so its sites count.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use proc_macro2::{TokenStream, TokenTree};
use serde::Deserialize;
use syn::visit::{self, Visit};
use syn::{ExprMethodCall, ExprPath, ImplItem, Item, Macro, TraitItem};

use crate::file_length::PRODUCTION_KINDS;
use crate::file_length::modules::{self, item_attrs};
use crate::ratchet::Exits;
use crate::testing::{TESTING_FEATURES, implies_testing_attrs, in_kit_file, relative};
use crate::util::repo_root;

/// The allowlist, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/catch-unwind.toml";

/// Directories the scan reads.
const ROOTS: [&str; 3] = ["crates/", "packages/", "src/"];

/// Where catching a panic is the code's job, and the decision that puts it there. A path ending
/// in `/` is a directory.
const BOUNDARIES: [(&str, &str); 3] = [
    (
        "crates/flui-foundation/src/panic.rs",
        "the containment primitives (ADR-0281, ADR-0287)",
    ),
    (
        "crates/flui-foundation/src/panic/",
        "the containment primitives (ADR-0281, ADR-0287)",
    ),
    (
        "crates/flui-native/src/owner/panic_boundary.rs",
        "the one OS entry into flui-native (ADR-0151 §16)",
    ),
];

/// Arguments for `cargo xtask catch-unwind`.
#[derive(Debug, clap::Args)]
pub(crate) struct CatchUnwindArgs {
    /// Print the allowlist the workspace needs (to stdout; no file is written).
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask catch-unwind`. Exit code 1 lists every finding.
pub(crate) fn catch_unwind(args: &CatchUnwindArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let meta = crate::util::metadata(&root)?;
    let tree = modules::walk_scoped(production_targets(&meta), implies_testing_attrs);
    let mut counts = BTreeMap::new();
    for path in tree.modules.keys() {
        let rel = relative(&root, path);
        if !in_scope(&root, &rel) {
            continue;
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {rel}"))?;
        let count = count_sites(&text);
        if count > 0 {
            counts.insert(rel, count);
        }
    }
    if args.seed {
        print!("{}", seed(&counts));
        return Ok(ExitCode::SUCCESS);
    }
    let allow: Allowlist = toml::from_str(&crate::util::read(ALLOWLIST)?)
        .with_context(|| format!("parsing {ALLOWLIST}"))?;
    // the walk's paths are absolute; show them as the rest of the report does
    let prefix = format!("{}{}", root.display(), std::path::MAIN_SEPARATOR);
    let mut findings: Vec<Finding> = tree
        .problems
        .iter()
        .map(|problem| Finding::Walk(problem.to_string().replace(&prefix, "").replace('\\', "/")))
        .collect();
    findings.extend(check(&counts, &allow, &Exits::from_repo(&root)?));
    if findings.is_empty() {
        println!(
            "catch-unwind: {} hand-written `catch_unwind` site(s) in {} file(s), each counted in \
             {ALLOWLIST}",
            counts.values().sum::<usize>(),
            counts.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("catch-unwind: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// The root file of every target a non-test build compiles, examples and targets that need a
/// testing feature left out.
fn production_targets(meta: &cargo_metadata::Metadata) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for package in meta.workspace_packages() {
        for target in &package.targets {
            let gated = target
                .required_features
                .iter()
                .any(|feature| TESTING_FEATURES.contains(&feature.as_str()));
            let production = target
                .kind
                .iter()
                .any(|kind| PRODUCTION_KINDS.contains(&kind.to_string().as_str()));
            if production && !gated {
                roots.push(PathBuf::from(target.src_path.as_std_path()));
            }
        }
    }
    roots
}

/// Whether the scan counts the production file at `rel`.
fn in_scope(root: &Path, rel: &str) -> bool {
    ROOTS.iter().any(|dir| rel.starts_with(dir))
        && !BOUNDARIES.iter().any(|(boundary, _)| {
            if boundary.ends_with('/') {
                rel.starts_with(boundary)
            } else {
                rel == *boundary
            }
        })
        && !in_kit_file(root, rel)
}

/// The `catch_unwind` sites of one file's production items; a file that does not parse is the
/// build's problem, not this gate's.
fn count_sites(text: &str) -> usize {
    let count = syn::parse_file(text).map_or(0, |file| {
        let mut sites = Sites(0);
        if !implies_testing_attrs(&file.attrs) {
            for item in &file.items {
                sites.visit_item(item);
            }
        }
        sites.0
    });
    // spans are per-thread and grow with every parse; nothing holds one past the visit
    proc_macro2::extra::invalidate_current_thread_spans();
    count
}

/// Counts the sites of the items it visits, skipping test code.
struct Sites(usize);

impl<'ast> Visit<'ast> for Sites {
    fn visit_item(&mut self, item: &'ast Item) {
        if !implies_testing_attrs(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs = match item {
            ImplItem::Const(item) => &item.attrs,
            ImplItem::Fn(item) => &item.attrs,
            ImplItem::Type(item) => &item.attrs,
            ImplItem::Macro(item) => &item.attrs,
            _ => return visit::visit_impl_item(self, item),
        };
        if !implies_testing_attrs(attrs) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let attrs = match item {
            TraitItem::Const(item) => &item.attrs,
            TraitItem::Fn(item) => &item.attrs,
            TraitItem::Type(item) => &item.attrs,
            TraitItem::Macro(item) => &item.attrs,
            _ => return visit::visit_trait_item(self, item),
        };
        if !implies_testing_attrs(attrs) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_expr_path(&mut self, expr: &'ast ExprPath) {
        if expr
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "catch_unwind")
        {
            self.0 += 1;
        }
        visit::visit_expr_path(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if call.method == "catch_unwind" {
            self.0 += 1;
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        self.0 += tokens_named(mac.tokens.clone());
        visit::visit_macro(self, mac);
    }
}

/// The `catch_unwind` identifiers in `tokens`, nested groups included.
fn tokens_named(tokens: TokenStream) -> usize {
    tokens
        .into_iter()
        .map(|token| match token {
            TokenTree::Ident(ident) => usize::from(ident == "catch_unwind"),
            TokenTree::Group(group) => tokens_named(group.stream()),
            TokenTree::Punct(_) | TokenTree::Literal(_) => 0,
        })
        .sum()
}

/// The allowlist file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    /// Why every listed file still catches by hand.
    reason: String,
    /// The ADR whose migration removes the entries.
    exit: String,
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One file's exact count.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Repository-relative, `/`-separated.
    path: String,
    /// `catch_unwind` sites in the file today.
    count: usize,
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    Walk(String),
    NoReason,
    BadExit {
        exit: String,
        why: String,
    },
    Duplicate {
        path: String,
    },
    Unlisted {
        path: String,
        count: usize,
    },
    Grew {
        path: String,
        count: usize,
        allowed: usize,
    },
    LowerIt {
        path: String,
        count: usize,
        allowed: usize,
    },
    Gone {
        path: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Walk(problem) => write!(f, "{problem}"),
            Self::NoReason => write!(f, "{ALLOWLIST} has no reason"),
            Self::BadExit { exit, why } => write!(f, "{ALLOWLIST}: the exit {exit:?}: {why}"),
            Self::Duplicate { path } => write!(f, "{path}: listed twice in {ALLOWLIST}"),
            Self::Unlisted { path, count } => write!(
                f,
                "{path}: {count} hand-written `catch_unwind` site(s), not in {ALLOWLIST}: change \
                 the state with `flui_foundation::panic::commit` and contain the calls with \
                 `FirstPanic` (ADR-0287)"
            ),
            Self::Grew {
                path,
                count,
                allowed,
            } => write!(
                f,
                "{path}: {count} hand-written `catch_unwind` site(s), the allowlist grants \
                 {allowed}: it grew; route the new one through `flui_foundation::panic` \
                 (ADR-0287)"
            ),
            Self::LowerIt {
                path,
                count,
                allowed,
            } => write!(
                f,
                "{path}: {count} hand-written `catch_unwind` site(s), the allowlist grants \
                 {allowed}: lower it to {count} in {ALLOWLIST}"
            ),
            Self::Gone { path } => write!(
                f,
                "{path}: listed, but it has no hand-written `catch_unwind` left or is not \
                 production code; remove it from {ALLOWLIST}"
            ),
        }
    }
}

/// Every finding of `counts` against `allow`.
fn check(counts: &BTreeMap<String, usize>, allow: &Allowlist, exits: &Exits) -> Vec<Finding> {
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
    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &allow.allow {
        let path = entry.path.clone();
        if listed.insert(&entry.path, entry.count).is_some() {
            findings.push(Finding::Duplicate { path: path.clone() });
        }
        let allowed = entry.count;
        findings.extend(match counts.get(&entry.path).copied() {
            None => Some(Finding::Gone { path }),
            Some(count) if count > allowed => Some(Finding::Grew {
                path,
                count,
                allowed,
            }),
            Some(count) if count < allowed => Some(Finding::LowerIt {
                path,
                count,
                allowed,
            }),
            Some(_) => None,
        });
    }
    for (path, &count) in counts {
        if !listed.contains_key(path.as_str()) {
            findings.push(Finding::Unlisted {
                path: path.clone(),
                count,
            });
        }
    }
    findings.sort();
    findings.dedup();
    findings
}

/// The allowlist `counts` needs, as TOML; `reason` and `exit` left for a person.
fn seed(counts: &BTreeMap<String, usize>) -> String {
    let mut out = String::from(
        "# Hand-written `catch_unwind` in production code (ADR-0287), written by\n\
         # `cargo xtask catch-unwind --seed`. Counts are exact and only go down.\n\
         reason = \"\"\nexit = \"\"\n",
    );
    for (path, count) in counts {
        let _ = write!(out, "\n[[allow]]\npath = \"{path}\"\ncount = {count}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exits() -> Exits {
        Exits::from_adrs(["0287".to_owned()])
    }

    fn allowlist(entries: &[(&str, usize)]) -> Allowlist {
        Allowlist {
            reason: "hand-ordered before the primitives".to_owned(),
            exit: "ADR-0287".to_owned(),
            allow: entries
                .iter()
                .map(|(path, count)| Entry {
                    path: (*path).to_owned(),
                    count: *count,
                })
                .collect(),
        }
    }

    fn findings(counts: &[(&str, usize)], allow: &Allowlist) -> Vec<Finding> {
        let counts = counts
            .iter()
            .map(|(path, count)| ((*path).to_owned(), *count))
            .collect();
        check(&counts, allow, &exits())
    }

    fn counts_calls_values_methods_and_macro_tokens() {
        let text = r#"
            use std::panic::{AssertUnwindSafe, catch_unwind};
            /// `catch_unwind` in a doc comment is not a site.
            fn owner() {
                let _ = catch_unwind(AssertUnwindSafe(|| ()));
                let _ = std::panic::catch_unwind(|| ());
                let _ = [|| ()].map(std::panic::catch_unwind);
                let _ = future.catch_unwind();
                assert!(std::panic::catch_unwind(|| ()).is_ok());
                tracing::warn!("catch_unwind in a string is not a site");
                // catch_unwind in a comment is not a site
            }
            macro_rules! contained {
                ($body:expr) => { std::panic::catch_unwind($body) };
            }
        "#;
        assert_eq!(count_sites(text), 6);
    }

    fn skips_test_code() {
        let text = r#"
            fn owner() { let _ = std::panic::catch_unwind(|| ()); }
            #[cfg(test)]
            mod tests { fn t() { let _ = std::panic::catch_unwind(|| ()); } }
            #[cfg(feature = "testing")]
            pub fn kit() { let _ = std::panic::catch_unwind(|| ()); }
            #[test]
            fn case() { let _ = std::panic::catch_unwind(|| ()); }
            struct Owner;
            impl Owner {
                #[cfg(test)]
                fn probe(&self) { let _ = std::panic::catch_unwind(|| ()); }
                fn run(&self) { let _ = std::panic::catch_unwind(|| ()); }
            }
        "#;
        assert_eq!(count_sites(text), 2);
        assert_eq!(
            count_sites("#![cfg(test)]\nfn t() { let _ = std::panic::catch_unwind(|| ()); }"),
            0
        );
        assert_eq!(count_sites("fn broken( {"), 0);
    }

    fn scopes_paths() {
        let root = repo_root();
        assert!(in_scope(&root, "crates/flui-view/src/persist/persisted.rs"));
        assert!(in_scope(&root, "packages/flui-material/src/lib.rs"));
        assert!(in_scope(&root, "src/lib.rs"));
        assert!(!in_scope(&root, "crates/flui-foundation/src/panic.rs"));
        assert!(!in_scope(
            &root,
            "crates/flui-foundation/src/panic/commit.rs"
        ));
        assert!(!in_scope(
            &root,
            "crates/flui-native/src/owner/panic_boundary.rs"
        ));
        assert!(!in_scope(&root, "crates/flui-testing/src/lib.rs"));
        assert!(!in_scope(&root, "crates/flui-view/src/testing.rs"));
        assert!(!in_scope(&root, "tools/xtask/src/catch_unwind.rs"));
        // every boundary is a path that exists, so a move cannot silently widen the scan
        for (boundary, _) in BOUNDARIES {
            assert!(root.join(boundary).exists(), "{boundary} does not exist");
        }
    }

    fn exact_count_passes() {
        assert_eq!(findings(&[("a.rs", 2)], &allowlist(&[("a.rs", 2)])), vec![]);
    }

    fn grew_lower_it_and_gone_are_findings() {
        assert_eq!(
            findings(
                &[("a.rs", 3), ("b.rs", 1)],
                &allowlist(&[("a.rs", 2), ("b.rs", 2), ("c.rs", 1)])
            ),
            vec![
                Finding::Grew {
                    path: "a.rs".into(),
                    count: 3,
                    allowed: 2
                },
                Finding::LowerIt {
                    path: "b.rs".into(),
                    count: 1,
                    allowed: 2
                },
                Finding::Gone {
                    path: "c.rs".into()
                },
            ]
        );
    }

    fn unlisted_and_bad_lists_are_findings() {
        assert_eq!(
            findings(&[("b.rs", 1)], &allowlist(&[])),
            vec![Finding::Unlisted {
                path: "b.rs".into(),
                count: 1
            }]
        );
        let mut bad = allowlist(&[("a.rs", 1), ("a.rs", 1)]);
        bad.reason = " ".into();
        bad.exit = "ADR-9999".into();
        let found = findings(&[("a.rs", 1)], &bad);
        assert!(found.contains(&Finding::NoReason));
        assert!(found.contains(&Finding::Duplicate {
            path: "a.rs".into()
        }));
        assert!(
            found
                .iter()
                .any(|finding| matches!(finding, Finding::BadExit { .. }))
        );
    }

    fn the_seed_parses_back() {
        let counts = BTreeMap::from([("a.rs".to_owned(), 2), ("b.rs".to_owned(), 1)]);
        let parsed: Allowlist = toml::from_str(&seed(&counts)).expect("seed parses");
        assert_eq!(parsed.allow.len(), 2);
        assert_eq!(
            (parsed.allow[0].path.as_str(), parsed.allow[0].count),
            ("a.rs", 2)
        );
    }

    #[test]
    fn catch_unwind_contract() {
        crate::table_test::run_table(
            "catch_unwind_contract",
            &[
                (
                    "counts_calls_values_methods_and_macro_tokens",
                    counts_calls_values_methods_and_macro_tokens as fn(),
                ),
                ("skips_test_code", skips_test_code as fn()),
                ("scopes_paths", scopes_paths as fn()),
                ("exact_count_passes", exact_count_passes as fn()),
                (
                    "grew_lower_it_and_gone_are_findings",
                    grew_lower_it_and_gone_are_findings as fn(),
                ),
                (
                    "unlisted_and_bad_lists_are_findings",
                    unlisted_and_bad_lists_are_findings as fn(),
                ),
                ("the_seed_parses_back", the_seed_parses_back as fn()),
            ],
        );
    }
}
