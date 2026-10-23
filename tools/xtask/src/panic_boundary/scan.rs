//! One file's OS entries and how each reaches the boundary: the import scan,
//! the entry collector, the body rule, the adapter rule and the identity
//! rules.
//!
//! The collector keeps owned copies of the bodies it judges, so the parsed
//! tokens of a `define_class!` invocation can be read like the file around
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ImplItem, Item, Meta, Path, Stmt, Token, TraitItem, Type, UseTree,
};

use super::{Finding, Problem};
use crate::source_tree::{
    Parsed, Truth, cfg_of, impl_item_attrs, item_attrs, stmt_attrs, trait_item_attrs,
};

/// The boundary's module path from the crate root.
pub(super) const BOUNDARY_MODULE: [&str; 2] = ["owner", "panic_boundary"];

/// The boundary's module name.
pub(super) const MODULE: &str = "panic_boundary";

/// The boundary.
pub(super) const BOUNDARY: &str = "os_entry";

/// The path segments that may lead to the boundary's module: `crate`,
/// `self`, `super` and the module that holds it.
const LEADS: [&str; 4] = ["crate", "self", "super", "owner"];

/// The crates that bind the operating system. A function or constructor
/// reached through one of them (by its path, or by a name imported from it)
/// that is handed a closure literal hands that closure to the OS: it is an
/// entry, whatever its name. `objc2_*` counts too.
const OS_CRATES: [&str; 14] = [
    "windows",
    "windows_core",
    "objc2",
    "block2",
    "dispatch",
    "dispatch2",
    "core_foundation",
    "core_graphics",
    "jni",
    "ndk",
    "android_activity",
    "accesskit_windows",
    "accesskit_macos",
    "accesskit_unix",
];

/// Functions of OS crates, by name, that run the closure they are given on
/// the caller's stack before they return and never hand it to the OS: no
/// entry. `autoreleasepool` runs it inside a pool objc2 pushes and pops in
/// Rust.
const SYNCHRONOUS: [&str; 1] = ["autoreleasepool"];

/// The types whose `new` hands a closure to the OS, matched by name where
/// their import cannot be read (a glob): block2's Objective-C blocks, and the
/// WinRT delegates of `windows::Foundation` and `windows::System::Threading`,
/// which WinRT calls through an `extern "system"` `Invoke`.
const CALLBACK_TYPES: [&str; 14] = [
    "RcBlock",
    "StackBlock",
    "TypedEventHandler",
    "EventHandler",
    "AsyncActionCompletedHandler",
    "AsyncActionProgressHandler",
    "AsyncActionWithProgressCompletedHandler",
    "AsyncOperationCompletedHandler",
    "AsyncOperationProgressHandler",
    "AsyncOperationWithProgressCompletedHandler",
    "DeferralCompletedHandler",
    "WorkItemHandler",
    "TimerElapsedHandler",
    "TimerDestroyedHandler",
];

/// GCD's methods that run a closure behind libdispatch's `extern "C"`
/// trampoline: `dispatch` and `dispatch2` share the `exec_*` and `barrier_*`
/// names, and `after` is `dispatch2`'s delayed one. A method's receiver has a
/// type the gate cannot read, so methods are matched by name: a new OS
/// method that takes a closure is added here, and the crate keeps these names
/// for GCD. GCD's `apply`, `for_each` and `Group::notify` are not listed:
/// the names collide with `Iterator::for_each` and the crate's own `apply`
/// and `notify`, and the crate does not use them.
const GCD_METHODS: [&str; 6] = [
    "exec_async",
    "exec_sync",
    "exec_after",
    "after",
    "barrier_async",
    "barrier_sync",
];

/// AccessKit's handler traits: an adapter calls them from inside its own UIA,
/// NSAccessibility or AT-SPI entry.
const HANDLER_TRAITS: [&str; 3] = ["ActionHandler", "ActivationHandler", "DeactivationHandler"];

/// What kind of entry the OS has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    /// A function with a foreign ABI.
    ExternFn,
    /// A `define_class!` method.
    ObjcMethod,
    /// A method of a windows-rs interface implementation.
    ComMethod,
    /// A method of an AccessKit handler.
    Handler,
    /// A closure handed to the OS.
    Callback,
}

impl Kind {
    /// Every kind, in the order the summary counts them.
    pub(super) const ALL: [Self; 5] = [
        Self::ExternFn,
        Self::ObjcMethod,
        Self::ComMethod,
        Self::Handler,
        Self::Callback,
    ];
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ExternFn => "extern fn",
            Self::ObjcMethod => "Objective-C method",
            Self::ComMethod => "COM method",
            Self::Handler => "AccessKit handler",
            Self::Callback => "OS callback",
        })
    }
}

/// What the gate judges of an entry.
#[derive(Clone)]
enum Body {
    /// A function's block.
    Block(Block),
    /// A closure's body; `moves` is whether its captures must move into the
    /// boundary's body (a GCD work item, run once and dropped by
    /// libdispatch).
    Closure { body: Expr, moves: bool },
    /// A callback constructor given no closure literal.
    Unseen,
}

/// One entry of a file.
#[derive(Clone)]
pub(super) struct Entry {
    pub(super) kind: Kind,
    /// The function, `Trait::method`, or the constructor or method a closure
    /// is handed to.
    pub(super) name: String,
    pub(super) line: usize,
    /// The self type of the impl around the entry, which `Self::` names.
    impl_type: Option<String>,
    body: Body,
}

/// How an entry reaches the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Its body is the boundary's call.
    Boundary,
    /// Its body calls this adapter, whose body is the boundary's call.
    Adapter(String),
    /// Its body is a constant: no code runs, so nothing can panic.
    Constant,
    /// It does not reach the boundary; why.
    Unguarded(String),
    /// It does, but an argument of the call runs before the boundary, or the
    /// boundary's body borrows what must move into it; which.
    Argument(String),
}

/// What the file's `use` items bind.
#[derive(Default)]
struct Imports {
    /// Every name a `use` binds, anywhere in the file.
    names: BTreeSet<String>,
    /// Those whose path starts at an OS crate.
    os: BTreeSet<String>,
    /// Whether a `use` binds the bare boundary name to the boundary.
    boundary: bool,
}

/// What one file holds.
#[derive(Default)]
pub(super) struct FileScan {
    pub(super) entries: Vec<Entry>,
    /// The free functions the file defines at module level, by name.
    free_fns: BTreeMap<String, Vec<Block>>,
    /// The methods its impls define, by self type and name.
    methods: BTreeMap<(String, String), Vec<Block>>,
    imports: Imports,
    /// Whether the file is the boundary's module.
    is_boundary: bool,
    /// Whether it defines the boundary at its top level.
    pub(super) defines_boundary: bool,
    pub(super) findings: Vec<Finding>,
}

impl FileScan {
    /// Every entry with its verdict.
    pub(super) fn judged(&self) -> impl Iterator<Item = (&Entry, Verdict)> {
        self.entries.iter().map(|entry| {
            let moves = matches!(entry.body, Body::Closure { moves: true, .. });
            (
                entry,
                self.verdict(&entry.body, entry.impl_type.as_deref(), true, moves),
            )
        })
    }

    /// `body`'s verdict. `impl_type` is what `Self` names; `adapters` is
    /// whether a call of an adapter counts (an adapter's own body must be the
    /// boundary's call); `moves` is whether the boundary's body must be a
    /// `move` closure or a value moved in.
    fn verdict(
        &self,
        body: &Body,
        impl_type: Option<&str>,
        adapters: bool,
        moves: bool,
    ) -> Verdict {
        let expr = match body {
            Body::Block(block) => match sole(block) {
                Some(expr) => expr,
                None if block.stmts.is_empty() => return Verdict::Constant,
                None => return Verdict::Unguarded(NOT_ONE.to_owned()),
            },
            Body::Closure { body, .. } => match strip(body) {
                Expr::Block(block) if block.label.is_none() => match sole(&block.block) {
                    Some(expr) => expr,
                    None if block.block.stmts.is_empty() => return Verdict::Constant,
                    None => return Verdict::Unguarded(NOT_ONE.to_owned()),
                },
                expr => expr,
            },
            Body::Unseen => {
                return Verdict::Unguarded(
                    "is handed no closure literal, so the gate cannot read its body; pass the \
                     closure itself"
                        .to_owned(),
                );
            }
        };
        if inert(expr) {
            return Verdict::Constant;
        }
        let (callee, args) = match strip(expr) {
            Expr::Call(call) => match strip(&call.func) {
                Expr::Path(path) if path.qself.is_none() => (segments(&path.path), &call.args),
                _ => return Verdict::Unguarded("calls a computed function".to_owned()),
            },
            Expr::MethodCall(call) => {
                return Verdict::Unguarded(format!(
                    "calls `.{}(…)`, which resolves by its receiver's type; call an adapter as \
                     `Type::{}(…)`",
                    call.method, call.method
                ));
            }
            _ => {
                return Verdict::Unguarded(format!(
                    "is not a call of `{MODULE}::{BOUNDARY}` or of an adapter"
                ));
            }
        };
        let shown = callee.join("::");
        let through = if self.is_boundary_call(&callee) {
            Verdict::Boundary
        } else if !adapters {
            return Verdict::Unguarded(format!("calls `{shown}`, not `{MODULE}::{BOUNDARY}`"));
        } else {
            match self.adapter(&callee, impl_type) {
                Ok(name) => Verdict::Adapter(name),
                Err(why) => return Verdict::Unguarded(why),
            }
        };
        if let Some(index) = args.iter().position(|arg| !inert(arg)) {
            return Verdict::Argument(format!(
                "passes argument {} of `{shown}` an expression that runs before the boundary; \
                 bind nothing outside it",
                index + 1
            ));
        }
        if moves && !args.last().is_some_and(moved_in) {
            return Verdict::Argument(format!(
                "hands `{shown}` a body that borrows its captures, so they drop after the \
                 boundary returns, in libdispatch's frames; make the body `move`"
            ));
        }
        through
    }

    /// Whether `path` names the boundary.
    fn is_boundary_call(&self, path: &[String]) -> bool {
        is_boundary_path(path)
            || (path.len() == 1
                && path[0] == BOUNDARY
                && (self.imports.boundary || self.is_boundary))
    }

    /// The adapter `callee` names, or why it names none: a free function of
    /// this file defined once at module level and not shadowed by an import,
    /// or a method of `Self` or a named type, defined once in this file.
    fn adapter(&self, callee: &[String], impl_type: Option<&str>) -> Result<String, String> {
        let shown = callee.join("::");
        let none = || {
            format!(
                "calls `{shown}`, which is neither `{MODULE}::{BOUNDARY}` nor an adapter of this \
                 file"
            )
        };
        let blocks = match callee {
            [name] => {
                if self.imports.names.contains(name) {
                    return Err(format!(
                        "calls `{shown}`, which an import of this file names, not an adapter"
                    ));
                }
                self.free_fns.get(name)
            }
            [ty, name] => {
                let ty = if ty == "Self" {
                    impl_type.ok_or_else(none)?
                } else {
                    ty
                };
                self.methods.get(&(ty.to_owned(), name.clone()))
            }
            _ => return Err(none()),
        };
        match blocks.map(Vec::as_slice) {
            None | Some([]) => Err(none()),
            Some([block]) => match self.verdict(&Body::Block(block.clone()), None, false, false) {
                Verdict::Boundary => Ok(shown),
                Verdict::Unguarded(why) | Verdict::Argument(why) => {
                    Err(format!("calls `{shown}`, whose own body {why}"))
                }
                // judged without adapters, so it names none; a constant
                // adapter is no boundary
                Verdict::Adapter(_) | Verdict::Constant => Err(format!(
                    "calls `{shown}`, which does not call the boundary itself"
                )),
            },
            Some(blocks) => Err(format!(
                "calls `{shown}`, which this file defines {} times; an adapter is defined once",
                blocks.len()
            )),
        }
    }
}

const NOT_ONE: &str = "is not one expression: whatever runs before or after the boundary's call \
                       can unwind into the OS";

/// The one expression of `block`, with or without its semicolon.
fn sole(block: &Block) -> Option<&Expr> {
    match block.stmts.as_slice() {
        [Stmt::Expr(expr, _)] => Some(expr),
        _ => None,
    }
}

/// `expr` without the parentheses and invisible groups around it.
fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => strip(&paren.expr),
        Expr::Group(group) => strip(&group.expr),
        _ => expr,
    }
}

fn segments(path: &Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect()
}

/// `…panic_boundary::os_entry`, led only by [`LEADS`].
pub(super) fn is_boundary_path(path: &[String]) -> bool {
    match path {
        [lead @ .., module, function] => {
            module == MODULE && function == BOUNDARY && leads_in_crate(lead)
        }
        _ => false,
    }
}

/// `…panic_boundary`, led only by [`LEADS`].
fn is_boundary_module_path(path: &[String]) -> bool {
    match path {
        [lead @ .., module] => module == MODULE && leads_in_crate(lead),
        [] => false,
    }
}

fn leads_in_crate(lead: &[String]) -> bool {
    lead.iter().all(|segment| LEADS.contains(&segment.as_str()))
}

/// Whether `name` is an OS crate's.
fn os_crate(name: &str) -> bool {
    OS_CRATES.contains(&name) || name.starts_with("objc2_")
}

/// An expression that cannot run code of its own: a literal, a path, a
/// closure, a tuple of them, a reference, field, cast or parenthesis of one,
/// or `AfterPanic::Answer` of one.
fn inert(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Path(_) | Expr::Closure(_) => true,
        Expr::Tuple(tuple) => tuple.elems.iter().all(inert),
        Expr::Reference(reference) => inert(&reference.expr),
        Expr::Paren(paren) => inert(&paren.expr),
        Expr::Group(group) => inert(&group.expr),
        Expr::Field(field) => inert(&field.base),
        Expr::Cast(cast) => inert(&cast.expr),
        Expr::Call(call) => {
            let answer = match strip(&call.func) {
                Expr::Path(path) if path.qself.is_none() => matches!(
                    segments(&path.path).as_slice(),
                    [.., ty, variant] if ty == "AfterPanic" && variant == "Answer"
                ),
                _ => false,
            };
            answer && call.args.iter().all(inert)
        }
        _ => false,
    }
}

/// A body that owns what it uses once it is handed over: a `move` closure,
/// or a value moved in by name.
fn moved_in(expr: &Expr) -> bool {
    match strip(expr) {
        Expr::Closure(closure) => closure.capture.is_some(),
        Expr::Path(_) => true,
        _ => false,
    }
}

/// Whether `abi` is a foreign one: anything but `extern "Rust"`.
fn foreign(abi: Option<&syn::Abi>) -> bool {
    abi.is_some_and(|abi| abi.name.as_ref().is_none_or(|name| name.value() != "Rust"))
}

/// `#[unsafe(method(…))]` or `#[unsafe(method_id(…))]`.
fn objc_method(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        Meta::List(list) if list.path.is_ident("unsafe") => matches!(
            list.tokens.clone().into_iter().next(),
            Some(TokenTree::Ident(ident)) if ident == "method" || ident == "method_id"
        ),
        _ => false,
    })
}

/// The last segment of a type's path: the name an impl's methods go under.
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.unraw().to_string()),
        Type::Group(group) => type_name(&group.elem),
        Type::Paren(paren) => type_name(&paren.elem),
        _ => None,
    }
}

fn line(span: Span) -> usize {
    span.start().line
}

/// Whether a node with `attrs` exists in some build that is not a test.
fn built(attrs: &[Attribute]) -> bool {
    cfg_of(attrs).0 != Truth::False
}

/// The entries, functions, imports and identity findings of `file`.
pub(super) fn scan_file(file: &Parsed, is_boundary: bool) -> FileScan {
    let mut uses = UseScan {
        rel: &file.rel,
        imports: Imports::default(),
        findings: Vec::new(),
    };
    uses.visit_file(&file.ast);
    let mut collector = Collector {
        rel: &file.rel,
        impl_kind: None,
        impl_type: None,
        in_define_class: false,
        nesting: 0,
        out: FileScan {
            imports: uses.imports,
            findings: uses.findings,
            is_boundary,
            ..FileScan::default()
        },
    };
    collector.visit_file(&file.ast);
    collector.out
}

/// The first pass: what the file's `use` items bind, and whether a binding
/// of the boundary's names points elsewhere.
struct UseScan<'f> {
    rel: &'f str,
    imports: Imports,
    findings: Vec<Finding>,
}

impl UseScan<'_> {
    fn bind(&mut self, prefix: &[String], original: &syn::Ident, bound: &syn::Ident) {
        let original = original.unraw().to_string();
        let bound_name = bound.unraw().to_string();
        let mut path = prefix.to_vec();
        if original != "self" {
            path.push(original);
        }
        if path.first().is_some_and(|root| os_crate(root)) {
            self.imports.os.insert(bound_name.clone());
        }
        self.imports.names.insert(bound_name.clone());
        let ok = match bound_name.as_str() {
            BOUNDARY => {
                let ok = is_boundary_path(&path);
                self.imports.boundary |= ok;
                ok
            }
            MODULE => is_boundary_module_path(&path),
            _ => return,
        };
        if !ok {
            self.findings.push(Finding {
                file: self.rel.to_owned(),
                line: line(bound.span()),
                what: format!("use {}", path.join("::")),
                kind: Problem::Identity,
                detail: format!(
                    "binds `{bound_name}` to something other than `{}`",
                    if bound_name == BOUNDARY {
                        format!("{}::{BOUNDARY}", BOUNDARY_MODULE.join("::"))
                    } else {
                        BOUNDARY_MODULE.join("::")
                    }
                ),
            });
        }
    }

    fn tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.unraw().to_string());
                self.tree(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => self.bind(prefix, &name.ident, &name.ident),
            UseTree::Rename(rename) => self.bind(prefix, &rename.ident, &rename.rename),
            UseTree::Glob(_) => {
                if is_boundary_module_path(prefix) {
                    self.imports.boundary = true;
                }
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.tree(tree, prefix);
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for UseScan<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !built(item_attrs(item)) {
            return;
        }
        if let Item::Use(item) = item {
            self.tree(&item.tree, &mut Vec::new());
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if built(impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if built(stmt_attrs(stmt)) {
            visit::visit_stmt(self, stmt);
        }
    }
}

struct Collector<'f> {
    rel: &'f str,
    /// The kind and trait of the enclosing impl, when its methods are entries.
    impl_kind: Option<(Kind, String)>,
    /// The self type of the enclosing impl.
    impl_type: Option<String>,
    /// Whether the items being read are a `define_class!` body.
    in_define_class: bool,
    /// How deep inside functions and impls the items being read are: a free
    /// function at depth 0 is at module level.
    nesting: usize,
    out: FileScan,
}

impl Collector<'_> {
    fn finding(&mut self, line: usize, what: String, kind: Problem, detail: String) {
        self.out.findings.push(Finding {
            file: self.rel.to_owned(),
            line,
            what,
            kind,
            detail,
        });
    }

    fn entry(&mut self, kind: Kind, name: String, line: usize, body: Body) {
        let impl_type = self.impl_type.clone();
        self.out.entries.push(Entry {
            kind,
            name,
            line,
            impl_type,
            body,
        });
    }

    /// An item under the boundary's name, other than the boundary itself.
    fn named(&mut self, ident: &syn::Ident, what: &str) {
        let name = ident.unraw().to_string();
        if name == BOUNDARY {
            self.finding(
                line(ident.span()),
                format!("{what} {name}"),
                Problem::Identity,
                format!(
                    "only `{}::{BOUNDARY}` may have this name",
                    BOUNDARY_MODULE.join("::")
                ),
            );
        }
    }

    /// A `define_class!` body: read like the file, its marked methods entries.
    fn define_class(&mut self, mac: &syn::Macro) {
        match syn::parse2::<syn::File>(class_impls(mac.tokens.clone())) {
            Ok(body) => {
                let outer = std::mem::replace(&mut self.in_define_class, true);
                for item in &body.items {
                    self.visit_item(item);
                }
                self.in_define_class = outer;
            }
            Err(error) => {
                let at = mac
                    .path
                    .segments
                    .last()
                    .map_or(0, |segment| line(segment.ident.span()));
                self.finding(
                    at,
                    "define_class!".to_owned(),
                    Problem::Unread,
                    format!("its body does not parse, so its methods cannot be read: {error}"),
                );
            }
        }
    }

    /// The tokens of a macro the gate does not parse: an entry in them is a
    /// finding.
    fn macro_tokens(&mut self, name: &str, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        for (at, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Group(group) => self.macro_tokens(name, group.stream()),
                // `extern "…" fn name` or `extern fn name`; a pointer type has no name
                TokenTree::Ident(ident) if ident == "extern" => {
                    let rest = &trees[at + 1..];
                    let rest = match rest.first() {
                        Some(TokenTree::Literal(_)) => &rest[1..],
                        _ => rest,
                    };
                    if let [TokenTree::Ident(word), TokenTree::Ident(function), ..] = rest
                        && word == "fn"
                    {
                        self.finding(
                            line(function.span()),
                            format!("fn {function}"),
                            Problem::Unread,
                            format!(
                                "an extern fn defined in the tokens of `{name}!`, whose body the \
                                 gate cannot read; define it outside the macro"
                            ),
                        );
                    }
                }
                TokenTree::Ident(ident) if CALLBACK_TYPES.iter().any(|ty| ident == ty) => {
                    if let [
                        TokenTree::Punct(first),
                        TokenTree::Punct(second),
                        TokenTree::Ident(new),
                        ..,
                    ] = &trees[at + 1..]
                        && first.as_char() == ':'
                        && second.as_char() == ':'
                        && new == "new"
                    {
                        self.finding(
                            line(ident.span()),
                            format!("{ident}::new"),
                            Problem::Unread,
                            format!(
                                "a callback made in the tokens of `{name}!`, whose closure the \
                                 gate cannot read; make it outside the macro"
                            ),
                        );
                    }
                }
                TokenTree::Punct(dot) if dot.as_char() == '.' => {
                    if let Some(TokenTree::Ident(method)) = trees.get(at + 1)
                        && GCD_METHODS.iter().any(|gcd| method == gcd)
                    {
                        self.finding(
                            line(method.span()),
                            format!(".{method}"),
                            Problem::Unread,
                            format!(
                                "a GCD work item in the tokens of `{name}!`, whose closure the \
                                 gate cannot read; dispatch it outside the macro"
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// A closure handed to the OS by `name`: the first closure literal among
    /// `args`. `moves`: its captures must move into the boundary's body.
    fn closure_entry(
        &mut self,
        name: String,
        at: usize,
        args: &Punctuated<Expr, Token![,]>,
        moves: bool,
    ) {
        let body = args
            .iter()
            .find_map(|arg| match strip(arg) {
                Expr::Closure(closure) => Some(Body::Closure {
                    body: (*closure.body).clone(),
                    moves,
                }),
                _ => None,
            })
            .unwrap_or(Body::Unseen);
        self.entry(Kind::Callback, name, at, body);
    }

    /// Whether `path` reaches into an OS crate: written from its root, or
    /// starting at a name an import of this file takes from it.
    fn os_path(&self, path: &[String]) -> bool {
        path.first()
            .is_some_and(|first| os_crate(first) || self.out.imports.os.contains(first))
    }
}

/// The impls of a `define_class!` body: what follows its `struct Name;`. The
/// class attributes before it (`#[ivars = Weak<T>]`) are not Rust attribute
/// syntax and hold no code.
fn class_impls(tokens: TokenStream) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let declared = trees
        .iter()
        .position(|tree| matches!(tree, TokenTree::Ident(ident) if ident == "struct"))
        .and_then(|at| {
            trees[at..]
                .iter()
                .position(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ';'))
                .map(|semi| at + semi + 1)
        });
    match declared {
        Some(after) => trees[after..].iter().cloned().collect(),
        None => tokens,
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !built(item_attrs(item)) {
            return;
        }
        match item {
            Item::Const(item) => self.named(&item.ident, "const"),
            Item::Static(item) => self.named(&item.ident, "static"),
            Item::Mod(module) if module.ident.unraw() == MODULE && module.content.is_some() => {
                self.finding(
                    line(module.ident.span()),
                    format!("mod {MODULE}"),
                    Problem::Identity,
                    format!(
                        "a second module named `{MODULE}`; the boundary is `{}`",
                        BOUNDARY_MODULE.join("::")
                    ),
                );
            }
            _ => {}
        }
        visit::visit_item(self, item);
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let name = function.sig.ident.unraw().to_string();
        if name == BOUNDARY {
            if self.out.is_boundary && self.nesting == 0 {
                self.out.defines_boundary = true;
            } else {
                self.named(&function.sig.ident, "fn");
            }
        }
        if self.nesting == 0 {
            self.out
                .free_fns
                .entry(name.clone())
                .or_default()
                .push((*function.block).clone());
        }
        if foreign(function.sig.abi.as_ref()) {
            self.entry(
                Kind::ExternFn,
                name,
                line(function.sig.ident.span()),
                Body::Block((*function.block).clone()),
            );
        }
        self.nesting += 1;
        visit::visit_item_fn(self, function);
        self.nesting -= 1;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let kind = item.trait_.as_ref().and_then(|(path, _)| {
            let name = path.segments.last()?.ident.unraw().to_string();
            if name.ends_with("_Impl") {
                Some((Kind::ComMethod, name))
            } else if HANDLER_TRAITS.contains(&name.as_str()) {
                Some((Kind::Handler, name))
            } else {
                None
            }
        });
        let outer_kind = std::mem::replace(&mut self.impl_kind, kind);
        let outer_type = std::mem::replace(&mut self.impl_type, type_name(&item.self_ty));
        self.nesting += 1;
        visit::visit_item_impl(self, item);
        self.nesting -= 1;
        self.impl_kind = outer_kind;
        self.impl_type = outer_type;
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if built(impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        let name = function.sig.ident.unraw().to_string();
        if name == BOUNDARY {
            self.named(&function.sig.ident, "fn");
        }
        if let Some(ty) = &self.impl_type {
            self.out
                .methods
                .entry((ty.clone(), name.clone()))
                .or_default()
                .push(function.block.clone());
        }
        let kind = if foreign(function.sig.abi.as_ref()) {
            Some((Kind::ExternFn, name))
        } else if self.in_define_class && objc_method(&function.attrs) {
            Some((Kind::ObjcMethod, name))
        } else {
            self.impl_kind
                .as_ref()
                .map(|(kind, of)| (*kind, format!("{of}::{name}")))
        };
        if let Some((kind, entry)) = kind {
            self.entry(
                kind,
                entry,
                line(function.sig.ident.span()),
                Body::Block(function.block.clone()),
            );
        }
        // what a method's body holds belongs to no impl
        let outer_kind = self.impl_kind.take();
        let outer_type = self.impl_type.take();
        visit::visit_impl_item_fn(self, function);
        self.impl_kind = outer_kind;
        self.impl_type = outer_type;
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if built(trait_item_attrs(item)) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.nesting += 1;
        visit::visit_item_trait(self, item);
        self.nesting -= 1;
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if function.sig.ident.unraw() == BOUNDARY {
            self.named(&function.sig.ident, "fn");
        }
        if let Some(block) = &function.default
            && foreign(function.sig.abi.as_ref())
        {
            self.entry(
                Kind::ExternFn,
                function.sig.ident.unraw().to_string(),
                line(function.sig.ident.span()),
                Body::Block(block.clone()),
            );
        }
        visit::visit_trait_item_fn(self, function);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if built(stmt_attrs(stmt)) {
            visit::visit_stmt(self, stmt);
        }
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if built(&arm.attrs) {
            visit::visit_arm(self, arm);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = strip(&call.func) {
            let callee = segments(&path.path);
            let named_callback = matches!(callee.as_slice(), [.., ty, new]
                if new == "new" && CALLBACK_TYPES.contains(&ty.as_str()));
            let closure_to_os = self.os_path(&callee)
                && call
                    .args
                    .iter()
                    .any(|arg| matches!(strip(arg), Expr::Closure(_)))
                && !callee
                    .last()
                    .is_some_and(|name| SYNCHRONOUS.contains(&name.as_str()));
            if named_callback || closure_to_os {
                let at = path
                    .path
                    .segments
                    .last()
                    .map_or(0, |segment| line(segment.ident.span()));
                self.closure_entry(callee.join("::"), at, &call.args, false);
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if GCD_METHODS.iter().any(|gcd| call.method == gcd) {
            self.closure_entry(
                format!(".{}", call.method),
                line(call.method.span()),
                &call.args,
                true,
            );
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.unraw().to_string());
        if name == "define_class" {
            self.define_class(mac);
        } else {
            self.macro_tokens(&name, mac.tokens.clone());
        }
    }
}
