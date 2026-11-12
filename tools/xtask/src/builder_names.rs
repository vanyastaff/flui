//! `cargo xtask builder-names`: a description type's builder method has no
//! `with_` prefix, and the list of the ones that still have it only shrinks
//! (ADR-0325).
//!
//! A widget, an `AppConfig`-style configuration or a platform request is a
//! description: the author assembles it and hands it to the framework, and
//! never reads it back. Its builder methods are the optional parameters of
//! the Flutter constructor, so they carry the parameter's bare name
//! (`.title("…")`, `.alignment(…)`). `with_x` keeps its own meanings
//! elsewhere: Flutter's `withX` and `copyWith` on a finished value
//! (`Color::with_alpha`, `TextStyle::with_color`), the chaining pair of a
//! setter on a read-write object (`Render*`, `Paint`, `*ParentData`), a
//! std-style constructor (`with_capacity`) and a closure run inside a scope.
//! Clippy has no lint for it (`wrong_self_convention` does not cover
//! `with_`), so this gate is a grep over the syntax tree.
//!
//! **What counts.** In the library and bin targets of every workspace member
//! ([`crate::source_tree`] follows each module tree the way rustc does; a
//! module or item under a cfg that is false in every build, `test`, is left
//! out), a method of an inherent `impl` named `with_*` that takes `self` by
//! value and returns `Self` (or the self type by name), on a self type that
//!
//! - is a view of its crate: an `impl` of a trait whose name ends in `View`
//!   (`View`, `StatelessView`, `RenderView`, …), an `impl_*_view!` macro call
//!   or a `#[derive(…View)]` names it; or
//! - is `Application`, or has a name ending in `Config`, `Options`,
//!   `Settings`, `Request`, `Builder`, `Params` or `Spec`.
//!
//! The other meanings of `with_` fall outside by construction, so the rule
//! needs no exception for them.
//!
//! **The list.** [`ALLOWLIST`] names, per crate, the methods that still carry
//! the prefix, each with the reason it is there. A method the list does not
//! name is a finding; so is a listed one the code no longer has, so the list
//! only shrinks. `--seed` prints the list the tree needs.
//!
//! **Limits.** The gate reads source, not expansions: a builder a macro emits
//! is out of its reach. A self type is named by its last path segment, so two
//! types of one name in one crate share their entries.
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).
//!

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};
use std::process::ExitCode;

use anyhow::Context;
use serde::Deserialize;
use syn::visit::{self, Visit};

use crate::source_tree::{Disk, Memory, Source, Target, Truth, cfg_of, walk};
use crate::try_new::{Krate, crates};
use crate::util;

/// The list, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/builder-names.toml";

/// The type-name suffixes of a description type.
const SUFFIXES: [&str; 7] = [
    "Config", "Options", "Settings", "Request", "Builder", "Params", "Spec",
];

/// Arguments for `cargo xtask builder-names`.
#[derive(Debug, clap::Args)]
pub(crate) struct BuilderNamesArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print the list the tree needs instead of checking it.
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask builder-names`. Exit code 1 lists every finding.
pub(crate) fn builder_names(args: &BuilderNamesArgs) -> anyhow::Result<ExitCode> {
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
            "builder-names: {} `with_` builder method(s) left, each listed in {ALLOWLIST}",
            list.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("builder-names: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// What the list keys a method by: its crate, its self type and its name.
type Key = (String, String, String);

/// One `with_` builder method the scan found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    krate: String,
    ty: String,
    method: String,
    file: String,
    line: usize,
}

impl Found {
    fn key(&self) -> Key {
        (self.krate.clone(), self.ty.clone(), self.method.clone())
    }
}

/// A `with_*(self) -> Self` method of an inherent `impl`, before the scan
/// knows whether its type is a description.
struct Candidate {
    ty: String,
    method: String,
    file: String,
    line: usize,
}

/// Every listed-or-not `with_` builder of a description type in the crates'
/// targets, each once.
fn scan(source: &dyn Source, crates: &[Krate]) -> anyhow::Result<BTreeSet<Found>> {
    let mut found = BTreeSet::new();
    for krate in crates {
        let mut seen = BTreeSet::new();
        let mut items = Items::default();
        for target in &krate.targets {
            for file in walk(source, target)? {
                // A library and a bin of one package may share modules.
                if !seen.insert(file.rel.clone()) {
                    continue;
                }
                items.file.clone_from(&file.rel);
                items.visit_file(&file.ast);
            }
        }
        for candidate in items.candidates {
            if items.views.contains(&candidate.ty) || is_description_name(&candidate.ty) {
                found.insert(Found {
                    krate: krate.name.clone(),
                    ty: candidate.ty,
                    method: candidate.method,
                    file: candidate.file,
                    line: candidate.line,
                });
            }
        }
    }
    Ok(found)
}

/// Whether a type's name alone makes it a description.
fn is_description_name(name: &str) -> bool {
    name == "Application" || SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

/// Collects, over one crate's files, the types that are views and the
/// `with_` builder methods, leaving out what a false cfg hides.
#[derive(Default)]
struct Items {
    file: String,
    views: BTreeSet<String>,
    candidates: Vec<Candidate>,
}

impl<'ast> Visit<'ast> for Items {
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

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if cfg_of(&item.attrs).0 != Truth::False && derives_a_view(&item.attrs) {
            self.views.insert(item.ident.to_string());
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if cfg_of(&item.attrs).0 != Truth::False && derives_a_view(&item.attrs) {
            self.views.insert(item.ident.to_string());
        }
        visit::visit_item_enum(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if cfg_of(&item.attrs).0 != Truth::False
            && let Some(name) = item.mac.path.segments.last()
        {
            let name = name.ident.to_string();
            if name.starts_with("impl_")
                && name.ends_with("_view")
                && let Some(proc_macro2::TokenTree::Ident(ty)) =
                    item.mac.tokens.clone().into_iter().next()
            {
                self.views.insert(ty.to_string());
            }
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if cfg_of(&item.attrs).0 == Truth::False {
            return;
        }
        let ty = type_name(&item.self_ty);
        match &item.trait_ {
            Some((path, _)) => {
                if path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident.to_string().ends_with("View"))
                {
                    self.views.insert(ty);
                }
            }
            None => {
                for impl_item in &item.items {
                    if let syn::ImplItem::Fn(function) = impl_item
                        && cfg_of(&function.attrs).0 != Truth::False
                        && function.sig.ident.to_string().starts_with("with_")
                        && takes_self_by_value(&function.sig)
                        && returns_self(&function.sig.output, &ty)
                    {
                        self.candidates.push(Candidate {
                            ty: ty.clone(),
                            method: function.sig.ident.to_string(),
                            file: self.file.clone(),
                            line: function.sig.ident.span().start().line,
                        });
                    }
                }
            }
        }
        visit::visit_item_impl(self, item);
    }
}

/// Whether a `#[derive(…)]` among the attributes names a trait ending in
/// `View` (`StatelessView`, `StatefulView`).
fn derives_a_view(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        let mut view = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.to_string().ends_with("View"))
            {
                view = true;
            }
            Ok(())
        });
        view
    })
}

/// Whether a method's receiver is `self` or `mut self`, by value.
fn takes_self_by_value(sig: &syn::Signature) -> bool {
    matches!(
        sig.inputs.first(),
        Some(syn::FnArg::Receiver(receiver)) if matches!(receiver.kind, syn::ReceiverKind::Value)
    )
}

/// Whether a return type is `Self` or the self type by name.
fn returns_self(output: &syn::ReturnType, ty: &str) -> bool {
    let syn::ReturnType::Type(_, returned) = output else {
        return false;
    };
    let name = type_name(returned);
    name == "Self" || name == ty
}

/// A type's name: the last segment of its path, through references; `_` for
/// a shape with no name.
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
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One listed method, with the reason it keeps its prefix.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(rename = "crate")]
    krate: String,
    #[serde(rename = "type")]
    ty: String,
    method: String,
    reason: String,
}

/// The listed keys. A key listed twice, or one without a reason, is an error.
fn parse_list(text: &str) -> anyhow::Result<BTreeSet<Key>> {
    let file: ListFile = toml::from_str(text)?;
    let mut keys = BTreeSet::new();
    for entry in file.allow {
        anyhow::ensure!(
            !entry.reason.trim().is_empty(),
            "{} `{}::{}` has an empty `reason`",
            entry.krate,
            entry.ty,
            entry.method
        );
        let key = (entry.krate, entry.ty, entry.method);
        anyhow::ensure!(
            !keys.contains(&key),
            "{} `{}::{}` is listed twice",
            key.0,
            key.1,
            key.2
        );
        keys.insert(key);
    }
    Ok(keys)
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// A `with_` builder the list does not name.
    Unlisted(Found),
    /// A listed method that is gone or no longer a `with_` builder.
    Stale(Key),
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted(found) => write!(
                f,
                "{}:{}: `{}::{}` in {} is a description's builder: name it `{}` (ADR-0325)",
                found.file,
                found.line,
                found.ty,
                found.method,
                found.krate,
                found.method.trim_start_matches("with_")
            ),
            Self::Stale((krate, ty, method)) => write!(
                f,
                "{krate} `{ty}::{method}` is listed, but it is no longer a `with_` builder; \
                 remove it from {ALLOWLIST}"
            ),
        }
    }
}

/// Every unlisted method and every stale entry.
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

/// The list the tree needs, in [`ALLOWLIST`]'s format; an entry that keeps its
/// prefix for an invariant gets its own reason by hand.
fn seed(found: &BTreeSet<Found>) -> String {
    let keys: BTreeSet<Key> = found.iter().map(Found::key).collect();
    let mut out = String::from(
        "# Every `with_` builder method of a description type (a view, `Application`, a\n\
         # `…Config/Options/Settings/Request/Builder/Params/Spec`), written by\n\
         # `cargo xtask builder-names --seed`. Such a method has the bare name (ADR-0325);\n\
         # the list only shrinks: the scan fails on a builder it does not name and on an\n\
         # entry that is no longer one.\n",
    );
    for (krate, ty, method) in &keys {
        let _ = write!(
            out,
            "\n[[allow]]\ncrate = \"{krate}\"\ntype = \"{ty}\"\nmethod = \"{method}\"\n\
             reason = \"written before the rule had a gate; its crate's rename removes it\"\n"
        );
    }
    out
}

/// The self-test's crate root.
const FIXTURE_ROOT: &str = "fixture/src/lib.rs";

/// The self-test's crate: one planted builder per shape the scan must see,
/// one hidden by each cfg it must skip, and the shapes it must leave alone.
fn fixture() -> (Memory, BTreeSet<Key>) {
    let lib = r"
        mod views;
        #[cfg(test)]
        mod tests;

        pub struct WindowConfig;
        impl WindowConfig {
            pub fn with_title(self, title: &str) -> Self { self }
            pub fn with_listed(mut self) -> WindowConfig { self }
            pub fn title(self, title: &str) -> Self { self }
            #[cfg(test)]
            fn with_hidden(self) -> Self { self }
        }
        pub struct Application<V>(V);
        impl<V> Application<V> {
            pub fn with_config(self, config: WindowConfig) -> Self { self }
        }

        // A value's `withX`, a setter's pair, a constructor, a scope: none counts.
        pub struct Color;
        impl Color { pub fn with_alpha(self, alpha: f32) -> Self { self } }
        pub struct RenderFlex;
        impl RenderFlex { pub fn with_gap(self, gap: f64) -> Self { self } }
        pub struct BufferBuilder;
        impl BufferBuilder {
            pub fn with_capacity(capacity: usize) -> Self { Self }
            pub fn with_scope(&mut self, f: impl FnOnce()) -> &mut Self { self }
            pub fn with_copy(&self) -> Self { Self }
            pub fn with_other(self) -> Color { Color }
        }
        pub trait Configure { fn with_default(self) -> Self; }
        impl Configure for WindowConfig { fn with_default(self) -> Self { self } }
    ";
    let views = r"
        pub struct Padding;
        impl Padding { pub fn with_padding(self, padding: f64) -> Self { self } }
        impl RenderView for Padding {}

        pub struct Overflow;
        impl Overflow { pub fn with_max_width(self, width: f64) -> Self { self } }
        impl_render_view!(Overflow, site = site);

        #[derive(Clone, StatelessView)]
        pub struct Greeting;
        impl Greeting { pub fn with_name(self, name: &str) -> Self { self } }
    ";
    let tests = r"
        pub struct TestConfig;
        impl TestConfig { pub fn with_seed(self) -> Self { self } }
    ";
    let files = [
        (FIXTURE_ROOT, lib),
        ("fixture/src/views.rs", views),
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
        [[allow]]
        crate = "fixture"
        type = "WindowConfig"
        method = "with_listed"
        reason = "fixture"

        [[allow]]
        crate = "fixture"
        type = "WindowConfig"
        method = "with_renamed"
        reason = "fixture"
        "#,
    )
    .expect("BUG: the fixture list parses");
    (source, list)
}

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 6] = [
    "`WindowConfig::with_title`",
    "`Application::with_config`",
    "`Padding::with_padding`",
    "`Overflow::with_max_width`",
    "`Greeting::with_name`",
    "fixture `WindowConfig::with_renamed` is listed",
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
        .collect::<Vec<String>>();
    let extra = reported
        .into_iter()
        .filter(|line| !EXPECTED.iter().any(|head| line.contains(head)))
        .collect::<Vec<String>>();
    (missed, extra)
}

/// `cargo xtask builder-names --self-test`.
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
        "builder-names: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_names_fixture_reports_exactly_the_planted_findings() {
        let (missed, extra) = self_test_diff();
        assert!(missed.is_empty(), "missed: {missed:?}");
        assert!(extra.is_empty(), "false positives: {extra:?}");
    }

    #[test]
    fn builder_names_list_rejects_an_entry_without_a_reason() {
        let error = parse_list(
            r#"
            [[allow]]
            crate = "a"
            type = "T"
            method = "with_x"
            reason = " "
            "#,
        )
        .expect_err("an empty reason is an error");
        assert!(error.to_string().contains("empty `reason`"), "{error}");
    }
}
