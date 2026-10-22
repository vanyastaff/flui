//! One file's OS entries and how each reaches the boundary: the entry
//! collector, the body rule, the adapter rule and the identity rules.
//!
//! The collector keeps owned copies of the bodies it judges, so the parsed
//! tokens of a `define_class!` invocation can be read like the file around
//! them.

use std::collections::BTreeMap;
use std::fmt;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Block, Expr, ImplItem, Item, Meta, Path, Stmt, Token, TraitItem, UseTree};

use super::{Finding, IDENTITY, UNREAD};
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

/// The types whose `new` hands a closure to the OS: block2's Objective-C
/// blocks, and the delegates of `windows::Foundation`, which WinRT calls
/// through an `extern "system"` `Invoke`.
const CALLBACK_TYPES: [&str; 11] = [
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
];

/// GCD's methods that run a closure behind libdispatch's `extern "C"`
/// trampoline: `dispatch` and `dispatch2` share the `exec_*` and `barrier_*`
/// names, and `after` is `dispatch2`'s delayed one. A method of the crate's
/// own under one of these names, given a closure, reads as one too, so the
/// crate keeps the names for GCD.
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
    /// A closure's body.
    Closure(Expr),
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
    body: Body,
}

/// How an entry reaches the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Its body is the boundary's call.
    Boundary,
    /// Its body calls this adapter, whose body is the boundary's call.
    Adapter(String),
    /// It does not reach the boundary; why.
    Unguarded(String),
    /// It does, but an argument of the call runs before the boundary; which.
    Argument(String),
}

/// What one file holds.
#[derive(Default)]
pub(super) struct FileScan {
    pub(super) entries: Vec<Entry>,
    /// Every function the file defines, by name: the adapter candidates.
    fns: BTreeMap<String, Vec<Block>>,
    /// Whether a `use` binds the bare boundary name to the boundary.
    binds_boundary: bool,
    /// Whether the file is the boundary's module.
    is_boundary: bool,
    /// Whether it defines the boundary at its top level.
    pub(super) defines_boundary: bool,
    pub(super) findings: Vec<Finding>,
}

impl FileScan {
    /// Every entry with its verdict.
    pub(super) fn judged(&self) -> impl Iterator<Item = (&Entry, Verdict)> {
        self.entries
            .iter()
            .map(|entry| (entry, self.verdict(&entry.body, true)))
    }

    /// `body`'s verdict; `adapters` is whether a call of an adapter counts
    /// (an adapter's own body must be the boundary's call).
    fn verdict(&self, body: &Body, adapters: bool) -> Verdict {
        let expr = match body {
            Body::Block(block) => match sole(block) {
                Some(expr) => expr,
                None => return Verdict::Unguarded(NOT_ONE.to_owned()),
            },
            Body::Closure(expr) => match strip(expr) {
                Expr::Block(block) if block.label.is_none() => match sole(&block.block) {
                    Some(expr) => expr,
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
        let (callee, args) = match strip(expr) {
            Expr::Call(call) => match strip(&call.func) {
                Expr::Path(path) if path.qself.is_none() => {
                    (Callee::Path(segments(&path.path)), &call.args)
                }
                _ => return Verdict::Unguarded("calls a computed function".to_owned()),
            },
            Expr::MethodCall(call) if is_self(&call.receiver) => (
                Callee::SelfMethod(call.method.unraw().to_string()),
                &call.args,
            ),
            _ => {
                return Verdict::Unguarded(format!(
                    "is not a call of `{MODULE}::{BOUNDARY}` or of an adapter"
                ));
            }
        };
        let through = match &callee {
            Callee::Path(path) if self.is_boundary_call(path) => Verdict::Boundary,
            _ if !adapters => {
                return Verdict::Unguarded(format!("calls `{callee}`, not `{MODULE}::{BOUNDARY}`"));
            }
            _ => match self.adapter(&callee) {
                Ok(name) => Verdict::Adapter(name),
                Err(why) => return Verdict::Unguarded(why),
            },
        };
        match args.iter().position(|arg| !inert(arg)) {
            Some(index) => Verdict::Argument(format!(
                "passes argument {} of `{callee}` an expression that runs before the boundary; \
                 bind nothing outside it",
                index + 1
            )),
            None => through,
        }
    }

    /// Whether `path` names the boundary.
    fn is_boundary_call(&self, path: &[String]) -> bool {
        is_boundary_path(path)
            || (path.len() == 1 && path[0] == BOUNDARY && (self.binds_boundary || self.is_boundary))
    }

    /// The adapter `callee` names, or why it names none.
    fn adapter(&self, callee: &Callee) -> Result<String, String> {
        let name = match callee {
            Callee::Path(path) => match path.as_slice() {
                [name] => name,
                [first, name] if first == "Self" => name,
                _ => {
                    return Err(format!(
                        "calls `{callee}`, which is neither `{MODULE}::{BOUNDARY}` nor an adapter \
                         of this file"
                    ));
                }
            },
            Callee::SelfMethod(name) => name,
        };
        match self.fns.get(name).map(Vec::as_slice) {
            None | Some([]) => Err(format!(
                "calls `{callee}`, which is neither `{MODULE}::{BOUNDARY}` nor an adapter of this \
                 file"
            )),
            Some([block]) => match self.verdict(&Body::Block(block.clone()), false) {
                Verdict::Boundary => Ok(name.clone()),
                Verdict::Unguarded(why) | Verdict::Argument(why) => {
                    Err(format!("calls `{callee}`, whose own body {why}"))
                }
                // judged without adapters, so it names none
                Verdict::Adapter(other) => Err(format!(
                    "calls `{callee}`, which calls `{other}`; an adapter calls the boundary itself"
                )),
            },
            Some(blocks) => Err(format!(
                "calls `{callee}`, which this file defines {} times; an adapter is defined once",
                blocks.len()
            )),
        }
    }
}

const NOT_ONE: &str = "is not one expression: whatever runs before or after the boundary's call \
                       can unwind into the OS";

/// What an entry's body calls.
enum Callee {
    /// A path, by its segments.
    Path(Vec<String>),
    /// `self.name(…)`.
    SelfMethod(String),
}

impl fmt::Display for Callee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => f.write_str(&path.join("::")),
            Self::SelfMethod(name) => write!(f, "self.{name}"),
        }
    }
}

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

fn is_self(expr: &Expr) -> bool {
    matches!(strip(expr), Expr::Path(path) if path.qself.is_none() && path.path.is_ident("self"))
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

/// An argument that cannot run code of its own: a literal, a path, a closure,
/// a reference, field, cast or parenthesis of one, or `AfterPanic::Answer`
/// of one.
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

fn line(span: Span) -> usize {
    span.start().line
}

/// The entries, functions, imports and identity findings of `file`.
pub(super) fn scan_file(file: &Parsed, is_boundary: bool) -> FileScan {
    let mut collector = Collector {
        rel: &file.rel,
        impl_kind: None,
        in_define_class: false,
        nesting: 0,
        out: FileScan {
            is_boundary,
            ..FileScan::default()
        },
    };
    collector.visit_file(&file.ast);
    collector.out
}

struct Collector<'f> {
    rel: &'f str,
    /// The kind and trait of the enclosing impl, when its methods are entries.
    impl_kind: Option<(Kind, String)>,
    /// Whether the items being read are a `define_class!` body.
    in_define_class: bool,
    /// How deep inside functions and inline modules the items being read are.
    nesting: usize,
    out: FileScan,
}

impl Collector<'_> {
    fn identity(&mut self, line: usize, what: String, detail: String) {
        self.out.findings.push(Finding {
            file: self.rel.to_owned(),
            line,
            what,
            kind: IDENTITY,
            detail,
        });
    }

    fn unread(&mut self, line: usize, what: String, detail: String) {
        self.out.findings.push(Finding {
            file: self.rel.to_owned(),
            line,
            what,
            kind: UNREAD,
            detail,
        });
    }

    fn entry(&mut self, kind: Kind, name: String, line: usize, body: Body) {
        self.out.entries.push(Entry {
            kind,
            name,
            line,
            body,
        });
    }

    fn function(&mut self, name: &syn::Ident, block: &Block) {
        let name = name.unraw().to_string();
        self.out.fns.entry(name).or_default().push(block.clone());
    }

    /// An item under the boundary's name, other than the boundary itself.
    fn named(&mut self, ident: &syn::Ident, what: &str) {
        let name = ident.unraw().to_string();
        if name == BOUNDARY {
            self.identity(
                line(ident.span()),
                format!("{what} {name}"),
                format!(
                    "only `{}::{BOUNDARY}` may have this name",
                    BOUNDARY_MODULE.join("::")
                ),
            );
        }
    }

    fn bind(&mut self, prefix: &[String], original: &syn::Ident, bound: &syn::Ident) {
        let original = original.unraw().to_string();
        let bound_name = bound.unraw().to_string();
        let mut path = prefix.to_vec();
        if original != "self" {
            path.push(original);
        }
        let ok = match bound_name.as_str() {
            BOUNDARY => {
                let ok = is_boundary_path(&path);
                self.out.binds_boundary |= ok;
                ok
            }
            MODULE => is_boundary_module_path(&path),
            _ => return,
        };
        if !ok {
            self.identity(
                line(bound.span()),
                format!("use {}", path.join("::")),
                format!(
                    "binds `{bound_name}` to something other than `{}`",
                    if bound_name == BOUNDARY {
                        format!("{}::{BOUNDARY}", BOUNDARY_MODULE.join("::"))
                    } else {
                        BOUNDARY_MODULE.join("::")
                    }
                ),
            );
        }
    }

    fn use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.unraw().to_string());
                self.use_tree(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => self.bind(prefix, &name.ident, &name.ident),
            UseTree::Rename(rename) => self.bind(prefix, &rename.ident, &rename.rename),
            UseTree::Glob(_) => {
                if is_boundary_module_path(prefix) {
                    self.out.binds_boundary = true;
                }
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(tree, prefix);
                }
            }
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
                self.unread(
                    at,
                    "define_class!".to_owned(),
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
                        self.unread(
                            line(function.span()),
                            format!("fn {function}"),
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
                        self.unread(
                            line(ident.span()),
                            format!("{ident}::new"),
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
                        self.unread(
                            line(method.span()),
                            format!(".{method}"),
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
    /// `args`.
    fn closure_entry(&mut self, name: String, at: usize, args: &Punctuated<Expr, Token![,]>) {
        let body = args
            .iter()
            .find_map(|arg| match strip(arg) {
                Expr::Closure(closure) => Some(Body::Closure((*closure.body).clone())),
                _ => None,
            })
            .unwrap_or(Body::Unseen);
        self.entry(Kind::Callback, name, at, body);
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

/// Whether a node with `attrs` exists in some build that is not a test.
fn built(attrs: &[Attribute]) -> bool {
    cfg_of(attrs).0 != Truth::False
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !built(item_attrs(item)) {
            return;
        }
        match item {
            Item::Const(item) => self.named(&item.ident, "const"),
            Item::Static(item) => self.named(&item.ident, "static"),
            Item::Use(item) => self.use_tree(&item.tree, &mut Vec::new()),
            Item::Mod(module) if module.ident.unraw() == MODULE && module.content.is_some() => {
                self.identity(
                    line(module.ident.span()),
                    format!("mod {MODULE}"),
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

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        self.nesting += 1;
        visit::visit_item_mod(self, module);
        self.nesting -= 1;
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
        self.function(&function.sig.ident, &function.block);
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
        let outer = std::mem::replace(&mut self.impl_kind, kind);
        visit::visit_item_impl(self, item);
        self.impl_kind = outer;
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
        self.function(&function.sig.ident, &function.block);
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
        let outer = self.impl_kind.take();
        self.nesting += 1;
        visit::visit_impl_item_fn(self, function);
        self.nesting -= 1;
        self.impl_kind = outer;
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if built(trait_item_attrs(item)) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if function.sig.ident.unraw() == BOUNDARY {
            self.named(&function.sig.ident, "fn");
        }
        if let Some(block) = &function.default {
            self.function(&function.sig.ident, block);
            if foreign(function.sig.abi.as_ref()) {
                self.entry(
                    Kind::ExternFn,
                    function.sig.ident.unraw().to_string(),
                    line(function.sig.ident.span()),
                    Body::Block(block.clone()),
                );
            }
        }
        self.nesting += 1;
        visit::visit_trait_item_fn(self, function);
        self.nesting -= 1;
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
            let mut last = path.path.segments.iter().rev();
            if let (Some(new), Some(ty)) = (last.next(), last.next())
                && new.ident == "new"
                && CALLBACK_TYPES.iter().any(|callback| ty.ident == callback)
            {
                self.closure_entry(
                    format!("{}::new", ty.ident),
                    line(ty.ident.span()),
                    &call.args,
                );
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
