//! `cargo xtask context-names`: a context is bound as `ctx`, never as `cx`
//! (ADR-0324).
//!
//! A reader who meets `cx` and `ctx` for the same role cannot tell from the
//! name which context a closure has, and the next change copies either. The
//! rule is about a binding's name and its type together: clippy's
//! `disallowed_names` sees the name alone and would also refuse the `cx` that
//! means a centre or a cubic's coefficient in geometry code. So this gate is a
//! grep over the syntax tree.
//!
//! **What counts.** In the library and bin targets of every workspace member
//! ([`crate::source_tree`] follows each module tree the way rustc does; a
//! module or item under a cfg that is false in every build, `test`, is left
//! out):
//!
//! - a typed binding — a function or method parameter, a typed closure
//!   parameter, a typed `let` — named `cx` or `_cx` whose type, through
//!   references, `dyn` and `impl`, is a context: its name ends in `Context`,
//!   `Cx` or `Ctx`;
//! - an untyped closure parameter named `cx` or `_cx`: a closure the
//!   framework hands a context to (an event callback, `poll_fn`) is where the
//!   name is copied from, and no closure in the tree binds a coordinate so.
//!
//! There is no list: the tree has no exception.
//!
//! **Limits.** The gate reads source, not expansions, and a generic parameter
//! (`cx: &S` with `S: ReadScope`) is named by its letter, not its bound.
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4).

use std::collections::BTreeSet;
use std::fmt;
use std::process::ExitCode;

use syn::visit::{self, Visit};

use crate::source_tree::{Disk, Memory, Source, Target, Truth, cfg_of, walk};
use crate::try_new::{Krate, crates};
use crate::util;

/// Arguments for `cargo xtask context-names`.
#[derive(Debug, clap::Args)]
pub(crate) struct ContextNamesArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
}

/// `cargo xtask context-names`. Exit code 1 lists every finding.
pub(crate) fn context_names(args: &ContextNamesArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let crates = crates(&root, &util::metadata(&root)?)?;
    let found = scan(&Disk(root), &crates)?;
    if found.is_empty() {
        println!("context-names: every context is bound as `ctx`");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("context-names: {} problem(s)", found.len());
    for finding in &found {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// One context bound as `cx`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    file: String,
    line: usize,
    /// The context type's name, or `_` for an untyped closure parameter.
    ty: String,
}

impl fmt::Display for Found {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: `cx: {}`: a context is bound as `ctx` (ADR-0324)",
            self.file, self.line, self.ty
        )
    }
}

/// Every misnamed binding in the crates' targets, each once.
fn scan(source: &dyn Source, crates: &[Krate]) -> anyhow::Result<BTreeSet<Found>> {
    let mut found = BTreeSet::new();
    for krate in crates {
        for target in &krate.targets {
            for file in walk(source, target)? {
                let mut visitor = Bindings {
                    file: &file.rel,
                    found: &mut found,
                };
                visitor.visit_file(&file.ast);
            }
        }
    }
    Ok(found)
}

/// Collects the misnamed bindings of one file, leaving out what a false cfg
/// hides.
struct Bindings<'a> {
    file: &'a str,
    found: &'a mut BTreeSet<Found>,
}

impl Bindings<'_> {
    fn report(&mut self, name: &syn::Ident, ty: String) {
        self.found.insert(Found {
            file: self.file.to_owned(),
            line: name.span().start().line,
            ty,
        });
    }
}

impl<'ast> Visit<'ast> for Bindings<'_> {
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
        if cfg_of(&item.attrs).0 != Truth::False {
            visit::visit_item_impl(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        if cfg_of(&function.attrs).0 != Truth::False {
            visit::visit_impl_item_fn(self, function);
        }
    }

    fn visit_pat_type(&mut self, binding: &'ast syn::PatType) {
        if let Some(name) = cx_name(&binding.pat)
            && let Some(ty) = last_segment(&binding.ty)
            && is_context(&ty)
        {
            self.report(name, ty);
        }
        visit::visit_pat_type(self, binding);
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        for input in &closure.inputs {
            if let Some(name) = cx_name(input) {
                self.report(name, "_".to_owned());
            }
        }
        visit::visit_expr_closure(self, closure);
    }
}

/// The identifier of a pattern that binds `cx` or `_cx` by itself.
fn cx_name(pat: &syn::Pat) -> Option<&syn::Ident> {
    match pat {
        syn::Pat::Ident(name) if name.ident == "cx" || name.ident == "_cx" => Some(&name.ident),
        _ => None,
    }
}

/// Whether a type name reads as a context: it ends in `Context`, `Cx` or
/// `Ctx`. A centre `cx: f64` is not one.
fn is_context(name: &str) -> bool {
    ["Context", "Cx", "Ctx"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// The last path segment of a type, through references, `dyn` and `impl`;
/// `None` for a shape with no name.
fn last_segment(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => last_segment(&reference.elem),
        syn::Type::Paren(inner) => last_segment(&inner.elem),
        syn::Type::Group(inner) => last_segment(&inner.elem),
        syn::Type::TraitObject(object) => bound_segment(&object.bounds),
        syn::Type::ImplTrait(bounds) => bound_segment(&bounds.bounds),
        _ => None,
    }
}

/// The last segment of the first trait among bounds.
fn bound_segment<'a>(bounds: impl IntoIterator<Item = &'a syn::TypeParamBound>) -> Option<String> {
    bounds.into_iter().find_map(|bound| match bound {
        syn::TypeParamBound::Trait(bound) => bound
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    })
}

/// The self-test's crate root.
const FIXTURE_ROOT: &str = "fixture/src/lib.rs";

/// The self-test's crate: one planted binding per shape the scan must see,
/// one hidden by each cfg it must skip, and the shapes it must leave alone.
fn fixture() -> Memory {
    let lib = r"
        mod inner;
        #[cfg(test)]
        mod tests;

        pub fn build(cx: &dyn BuildContext) {}
        pub struct S;
        impl S {
            pub fn layout(&self, _cx: &mut BoxLayoutCtx<'_>) {}
            #[cfg(test)]
            fn hidden(cx: &dyn BuildContext) {}
        }
        pub fn closure() {
            let _ = |cx: &RenderObjectContext<'_>| {};
            let cx: &dyn crate::LifecycleContext = todo!();
            let _ = on_tap(move |cx| cx);
        }

        // A `ctx`, a coordinate, a destructured pair and a field: none counts.
        pub fn fine(ctx: &mut EventCx<'_>) {}
        pub fn centre(cx: f64, cy: f64) { let (cx, cy) = (cx, cy); }
        pub fn size(size: Size) -> i32 { size.cx }
    ";
    let inner = r"
        pub trait Provide { fn provide(&self, cx: &ProviderContext); }
        pub fn poll(cx: &mut std::task::Context<'_>) {}
    ";
    let tests = r"
        fn helper(cx: &dyn BuildContext) {}
    ";
    let files = [
        (FIXTURE_ROOT, lib),
        ("fixture/src/inner.rs", inner),
        ("fixture/src/tests.rs", tests),
    ];
    Memory(
        files
            .into_iter()
            .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
            .collect(),
    )
}

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 7] = [
    "`cx: BuildContext`",
    "`cx: BoxLayoutCtx`",
    "`cx: RenderObjectContext`",
    "`cx: LifecycleContext`",
    "`cx: _`",
    "`cx: ProviderContext`",
    "`cx: Context`",
];

/// Runs the rule over the fixture: what it missed and what it reported
/// beyond [`EXPECTED`].
fn self_test_diff() -> (Vec<String>, Vec<String>) {
    let crates = [Krate {
        name: "fixture".to_owned(),
        targets: vec![Target {
            src: FIXTURE_ROOT.to_owned(),
            bin: None,
        }],
    }];
    let found = scan(&fixture(), &crates).expect("BUG: the fixture's module tree parses");
    let reported: Vec<String> = found.iter().map(ToString::to_string).collect();
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

/// `cargo xtask context-names --self-test`.
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
        "context-names: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}
