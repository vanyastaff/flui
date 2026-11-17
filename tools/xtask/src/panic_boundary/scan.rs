//! One file's OS entries and how each reaches the boundary: the import scan,
//! the entry collector, the body rule, the adapter rule, the ivars rule and
//! the identity rules.
//!
//! The collector keeps owned copies of the bodies it judges, so the parsed
//! tokens of a `define_class!` invocation can be read like the file around
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
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

/// The boundary's ivars: the type a class keeps its Rust state in, whose
/// destructor is the boundary's call, so that objc2's `dealloc` destroys the
/// state inside it.
pub(super) const IVARS: &str = "ContainedIvars";

/// The path segments that may lead to the boundary's module: `crate`,
/// `self`, `super` and the module that holds it.
const LEADS: [&str; 4] = ["crate", "self", "super", "owner"];

/// The module path of the adapter every JNI native method goes through.
pub(super) const JNI_MODULE: [&str; 3] = ["owner", "android", "native_call"];

/// That adapter: besides containing the body's panic, it clears the Java
/// exception the body leaves pending, which Java would throw from the
/// framework callback that called the method.
pub(super) const JNI_ADAPTER: &str = "jni_native";

/// The path segments that may lead to [`JNI_MODULE`]'s last segment.
const JNI_LEADS: [&str; 5] = ["crate", "self", "super", "owner", "android"];

/// The types a JNI native method's first parameter, its environment, is
/// written as, by their last segment.
const JNI_ENVS: [&str; 2] = ["EnvUnowned", "JNIEnv"];

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

/// The ndk's methods that hand a closure to an Android looper, which calls it
/// behind an `extern "C"` trampoline that aborts the process on a panic
/// (`ThreadLooper` and `ForeignLooper::add_fd_with_callback`). A method's
/// receiver has a type the gate cannot read, so they are matched by name, as
/// GCD's are; unlike a GCD work item, a looper calls its closure again and
/// again, so its captures cannot move into the boundary's body. The crate
/// hands a looper no closure today; the name is listed so the first is read.
const LOOPER_METHODS: [&str; 1] = ["add_fd_with_callback"];

/// android-activity's method that runs a closure on Android's Java main
/// thread under its own guard, which only logs a panic and loses what the
/// closure owed. Only the posting module calls it: everything else posts
/// through its `post`, which runs the closure inside the boundary and owes a
/// report on each exit.
const JAVA_MAIN_THREAD: &str = "run_on_java_main_thread";

/// The one file that may call [`JAVA_MAIN_THREAD`].
const JAVA_MAIN_THREAD_POSTER: &str = "owner/android/main_thread.rs";

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
    /// What objc2's generated `dealloc` runs for a `define_class!` class: the
    /// class's own `Drop`, then the destructor of its ivars. The `Drop` of
    /// the boundary's ivars is one too.
    Destructor,
    /// A method of a windows-rs interface implementation.
    ComMethod,
    /// A method of an AccessKit handler.
    Handler,
    /// A closure handed to the OS.
    Callback,
}

impl Kind {
    /// Every kind, in the order the summary counts them.
    pub(super) const ALL: [Self; 6] = [
        Self::ExternFn,
        Self::ObjcMethod,
        Self::Destructor,
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
            Self::Destructor => "Objective-C destructor",
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
    /// The type a class declares its ivars as (`#[ivars = …]`), which objc2's
    /// `dealloc` drops in place.
    Ivars(Type),
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
    /// Whether it is a JNI native method, which goes through
    /// [`JNI_ADAPTER`] alone.
    jni: bool,
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
    /// Whether a `use` binds the bare name of the boundary's ivars to them.
    ivars: bool,
    /// Whether a `use` binds the bare name of the JNI adapter to it.
    jni: bool,
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
    /// Whether the file is the JNI adapter's module.
    is_jni_module: bool,
    /// The JNI adapter's line and body, where this file, its module, defines
    /// it at its top level.
    jni_adapter: Option<(usize, Block)>,
    /// Where it defines the boundary's ivars at its top level.
    defines_ivars: Option<usize>,
    /// The classes its `define_class!` invocations declare, by name.
    classes: BTreeSet<String>,
    /// The `drop` of each `impl Drop for …` it holds: the type, the method's
    /// line and its body.
    drops: Vec<(String, usize, Block)>,
    pub(super) findings: Vec<Finding>,
}

impl FileScan {
    /// Every entry with its verdict.
    pub(super) fn judged(&self) -> impl Iterator<Item = (&Entry, Verdict)> {
        self.entries.iter().map(|entry| {
            if entry.jni {
                return (entry, self.jni_verdict(&entry.body));
            }
            let moves = matches!(entry.body, Body::Closure { moves: true, .. });
            (
                entry,
                self.verdict(&entry.body, entry.impl_type.as_deref(), true, moves),
            )
        })
    }

    /// A JNI native method's verdict: its body is one call of the JNI
    /// adapter, whose arguments run no code; the boundary called directly or
    /// through another adapter leaves the exception pending.
    fn jni_verdict(&self, body: &Body) -> Verdict {
        let refused = || {
            Verdict::Unguarded(format!(
                "is a JNI native method, which is one call of `{}::{JNI_ADAPTER}`: Java calls it \
                 from a framework callback that catches nothing, and only that adapter clears \
                 the exception its body leaves pending",
                JNI_MODULE.join("::")
            ))
        };
        let Body::Block(block) = body else {
            return refused();
        };
        let Some(Expr::Call(call)) = sole(block).map(strip) else {
            return refused();
        };
        let Expr::Path(path) = strip(&call.func) else {
            return refused();
        };
        let callee = segments(&path.path);
        if path.qself.is_some() || !self.is_jni_adapter_call(&callee) {
            return refused();
        }
        if let Some(index) = call.args.iter().position(|arg| !inert(arg)) {
            return Verdict::Argument(format!(
                "passes argument {} of `{JNI_ADAPTER}` an expression that runs before the \
                 boundary; bind nothing outside it",
                index + 1
            ));
        }
        Verdict::Adapter(JNI_ADAPTER.to_owned())
    }

    /// Whether `path` names the JNI adapter.
    fn is_jni_adapter_call(&self, path: &[String]) -> bool {
        is_jni_adapter_path(path)
            || (path.len() == 1
                && path[0] == JNI_ADAPTER
                && (self.imports.jni || self.is_jni_module))
    }

    /// What is wrong with the JNI adapter, when this file is its module: it
    /// is missing, or its body is not the boundary's call.
    pub(super) fn jni_adapter_problem(&self) -> Option<(usize, String)> {
        if !self.is_jni_module {
            return None;
        }
        let Some((at, block)) = &self.jni_adapter else {
            return Some((
                1,
                format!(
                    "the JNI adapter's module does not define `{JNI_ADAPTER}` at its top level"
                ),
            ));
        };
        match self.verdict(&Body::Block(block.clone()), None, false, false) {
            Verdict::Boundary => None,
            Verdict::Unguarded(why) | Verdict::Argument(why) => {
                Some((*at, format!("`{JNI_ADAPTER}`'s own body {why}")))
            }
            Verdict::Adapter(_) | Verdict::Constant => Some((
                *at,
                format!("`{JNI_ADAPTER}` does not call the boundary itself"),
            )),
        }
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
            Body::Ivars(ty) => {
                return if self.is_boundary_ivars(ty) {
                    Verdict::Boundary
                } else {
                    Verdict::Unguarded(format!(
                        "keeps ivars that objc2's `dealloc` destroys outside the boundary, \
                         whoever releases the object last; declare them `{MODULE}::{IVARS}<…>`"
                    ))
                };
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

    /// Whether `ty` is the boundary's ivars: `…panic_boundary::ContainedIvars<…>`,
    /// or the bare name where the file imports it from there.
    fn is_boundary_ivars(&self, ty: &Type) -> bool {
        let path = match ty {
            Type::Path(path) if path.qself.is_none() => segments(&path.path),
            Type::Group(group) => return self.is_boundary_ivars(&group.elem),
            Type::Paren(paren) => return self.is_boundary_ivars(&paren.elem),
            _ => return false,
        };
        is_boundary_item_path(&path, IVARS)
            || (path.len() == 1 && path[0] == IVARS && (self.imports.ivars || self.is_boundary))
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
    is_boundary_item_path(path, BOUNDARY)
}

/// `…panic_boundary::<item>`, led only by [`LEADS`].
fn is_boundary_item_path(path: &[String], item: &str) -> bool {
    match path {
        [lead @ .., module, name] => module == MODULE && name == item && leads_in_crate(lead),
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

/// `…native_call::jni_native`, led only by [`JNI_LEADS`].
fn is_jni_adapter_path(path: &[String]) -> bool {
    match path {
        [module @ .., name] => name == JNI_ADAPTER && is_jni_module_path(module),
        [] => false,
    }
}

/// `…native_call`, led only by [`JNI_LEADS`].
fn is_jni_module_path(path: &[String]) -> bool {
    match path {
        [lead @ .., module] => {
            module == JNI_MODULE[JNI_MODULE.len() - 1]
                && lead
                    .iter()
                    .all(|segment| JNI_LEADS.contains(&segment.as_str()))
        }
        [] => false,
    }
}

/// Whether a foreign function with `sig` is a JNI native method: its first
/// parameter is the environment, by value or behind a pointer.
fn takes_jni_env(sig: &syn::Signature) -> bool {
    let Some(syn::FnArg::Typed(first)) = sig.inputs.first() else {
        return false;
    };
    let mut ty = &*first.ty;
    while let Type::Ptr(pointer) = ty {
        ty = &pointer.elem;
    }
    type_name(ty).is_some_and(|name| JNI_ENVS.contains(&name.as_str()))
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
pub(super) fn scan_file(file: &Parsed, is_boundary: bool, is_jni_module: bool) -> FileScan {
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
            is_jni_module,
            ..FileScan::default()
        },
    };
    collector.visit_file(&file.ast);
    collector.destructors();
    let mut posts = MainThreadPosts {
        rel: &file.rel,
        item: String::new(),
        findings: Vec::new(),
    };
    posts.visit_file(&file.ast);
    collector.out.findings.extend(posts.findings);
    collector.out
}

/// Every [`JAVA_MAIN_THREAD`] identifier in a build that is not a test,
/// wherever it is written: a method or path call, a function value, an
/// import, or the tokens of a macro call or a `macro_rules!` body, which syn
/// leaves unparsed. Only the posting module's `fn post` may name it.
struct MainThreadPosts<'f> {
    rel: &'f str,
    /// The top-level item being read, which a finding names.
    item: String,
    findings: Vec<Finding>,
}

impl MainThreadPosts<'_> {
    fn found(&mut self, at: Span) {
        self.findings.push(Finding {
            file: self.rel.to_owned(),
            line: line(at),
            what: format!("{JAVA_MAIN_THREAD} in {}", self.item),
            kind: Problem::Unguarded,
            detail: format!(
                "a closure posted to Android's Java main thread outside `fn post` of \
                 `{JAVA_MAIN_THREAD_POSTER}`, which runs it inside the boundary and reports \
                 every exit but success"
            ),
        });
    }

    fn tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Ident(ident) if ident == JAVA_MAIN_THREAD => {
                    self.found(ident.span());
                }
                TokenTree::Group(group) => self.tokens(group.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for MainThreadPosts<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        for item in &file.items {
            if !built(item_attrs(item)) {
                continue;
            }
            let (name, is_post) = match item {
                Item::Fn(function) => {
                    let name = function.sig.ident.unraw().to_string();
                    let is_post = name == "post";
                    (format!("fn {name}"), is_post)
                }
                Item::Macro(mac) => (
                    mac.ident.as_ref().map_or_else(
                        || "a macro call".to_owned(),
                        |name| format!("macro_rules! {name}"),
                    ),
                    false,
                ),
                Item::Mod(module) => (format!("mod {}", module.ident), false),
                Item::Impl(_) => ("an impl".to_owned(), false),
                Item::Use(_) => ("a use".to_owned(), false),
                _ => ("an item".to_owned(), false),
            };
            if is_post && self.rel.ends_with(JAVA_MAIN_THREAD_POSTER) {
                continue;
            }
            self.item = name;
            self.visit_item(item);
        }
    }

    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if ident == JAVA_MAIN_THREAD {
            self.found(ident.span());
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(mac.tokens.clone());
        visit::visit_macro(self, mac);
    }
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
            IVARS => {
                let ok = is_boundary_item_path(&path, IVARS);
                self.imports.ivars |= ok;
                ok
            }
            MODULE => is_boundary_module_path(&path),
            JNI_ADAPTER => {
                let ok = is_jni_adapter_path(&path);
                self.imports.jni |= ok;
                ok
            }
            _ => return,
        };
        if !ok {
            let meant = match bound_name.as_str() {
                MODULE => BOUNDARY_MODULE.join("::"),
                JNI_ADAPTER => format!("{}::{JNI_ADAPTER}", JNI_MODULE.join("::")),
                _ => format!("{}::{bound_name}", BOUNDARY_MODULE.join("::")),
            };
            self.findings.push(Finding {
                file: self.rel.to_owned(),
                line: line(bound.span()),
                what: format!("use {}", path.join("::")),
                kind: Problem::Identity,
                detail: format!("binds `{bound_name}` to something other than `{meant}`"),
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
                    self.imports.ivars = true;
                }
                if is_jni_module_path(prefix) {
                    self.imports.jni = true;
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
            jni: false,
        });
    }

    /// A foreign function's entry, a JNI native method when its first
    /// parameter is the environment.
    fn extern_entry(&mut self, sig: &syn::Signature, block: Block) {
        self.entry(
            Kind::ExternFn,
            sig.ident.unraw().to_string(),
            line(sig.ident.span()),
            Body::Block(block),
        );
        if let Some(entry) = self.out.entries.last_mut() {
            entry.jni = takes_jni_env(sig);
        }
    }

    /// An item named like the JNI adapter: the adapter itself, at its
    /// module's top level, or a second one.
    fn jni_adapter_named(&mut self, function: &syn::ItemFn) {
        if function.sig.ident.unraw() != JNI_ADAPTER {
            return;
        }
        if self.out.is_jni_module && self.nesting == 0 && self.out.jni_adapter.is_none() {
            self.out.jni_adapter =
                Some((line(function.sig.ident.span()), (*function.block).clone()));
        } else {
            self.jni_impostor(&function.sig.ident);
        }
    }

    /// A function under the JNI adapter's name that is not the adapter.
    fn jni_impostor(&mut self, ident: &syn::Ident) {
        self.finding(
            line(ident.span()),
            format!("fn {JNI_ADAPTER}"),
            Problem::Identity,
            format!(
                "only `{}::{JNI_ADAPTER}` may have this name",
                JNI_MODULE.join("::")
            ),
        );
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

    /// An item under the name of the boundary's ivars: their definition, at
    /// the boundary module's top level, or a second type of that name.
    fn ivars_named(&mut self, ident: &syn::Ident, what: &str) {
        if ident.unraw() != IVARS {
            return;
        }
        if self.out.is_boundary && self.nesting == 0 && self.out.defines_ivars.is_none() {
            self.out.defines_ivars = Some(line(ident.span()));
        } else {
            self.finding(
                line(ident.span()),
                format!("{what} {IVARS}"),
                Problem::Identity,
                format!(
                    "only `{}::{IVARS}` may have this name",
                    BOUNDARY_MODULE.join("::")
                ),
            );
        }
    }

    /// Keep the `drop` of an `impl Drop for …`: whether it is an entry is
    /// known once every class of the file is.
    fn drop_of(&mut self, item: &syn::ItemImpl) {
        let is_drop = item.trait_.as_ref().is_some_and(|(path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Drop")
        });
        let (true, Some(ty)) = (is_drop, type_name(&item.self_ty)) else {
            return;
        };
        for member in &item.items {
            if let ImplItem::Fn(function) = member
                && function.sig.ident == "drop"
                && built(&function.attrs)
            {
                self.out.drops.push((
                    ty.clone(),
                    line(function.sig.ident.span()),
                    function.block.clone(),
                ));
            }
        }
    }

    /// The `Drop` impls objc2's `dealloc` runs, as entries: a class's own,
    /// and the one of the boundary's ivars, which every class's ivars are
    /// destroyed through.
    fn destructors(&mut self) {
        let mut contained = false;
        for (ty, at, body) in std::mem::take(&mut self.out.drops) {
            let ivars = self.out.is_boundary && ty == IVARS;
            contained |= ivars;
            if ivars || self.out.classes.contains(&ty) {
                self.out.entries.push(Entry {
                    kind: Kind::Destructor,
                    name: format!("{ty}::drop"),
                    line: at,
                    impl_type: Some(ty),
                    body: Body::Block(body),
                    jni: false,
                });
            }
        }
        if let Some(at) = self.out.defines_ivars
            && !contained
        {
            self.finding(
                at,
                format!("struct {IVARS}"),
                Problem::Identity,
                "the boundary's ivars implement no `Drop`, so nothing destroys what they hold \
                 inside the boundary"
                    .to_owned(),
            );
        }
    }

    /// A `define_class!` invocation: its body read like the file, its marked
    /// methods entries, and the ivars it declares an entry of their own.
    fn define_class(&mut self, mac: &syn::Macro) {
        let class = ClassTokens::of(mac.tokens.clone());
        if let Some(name) = &class.name {
            self.out.classes.insert(name.clone());
        }
        if let Some((at, ivars)) = class.ivars {
            let name = format!(
                "{}::dealloc",
                class.name.as_deref().unwrap_or("define_class!")
            );
            match syn::parse2::<Type>(ivars) {
                Ok(ty) => self.entry(Kind::Destructor, name, at, Body::Ivars(ty)),
                Err(error) => self.finding(
                    at,
                    name,
                    Problem::Unread,
                    format!(
                        "its ivars do not parse as a type, so their destructor cannot be \
                         judged: {error}"
                    ),
                ),
            }
        }
        match syn::parse2::<syn::File>(class.impls) {
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
                        && GCD_METHODS
                            .iter()
                            .chain(&LOOPER_METHODS)
                            .any(|named| method == named)
                    {
                        self.finding(
                            line(method.span()),
                            format!(".{method}"),
                            Problem::Unread,
                            format!(
                                "a closure handed to GCD or a looper in the tokens of `{name}!`, \
                                 which the gate cannot read; hand it over outside the macro"
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

/// What a `define_class!` invocation holds. The class attributes before its
/// `struct Name;` are not Rust attribute syntax (`#[ivars = Weak<T>]`), so
/// they are read as tokens.
struct ClassTokens {
    /// The class the invocation declares.
    name: Option<String>,
    /// The line of its `#[ivars = …]` and the tokens of the type.
    ivars: Option<(usize, TokenStream)>,
    /// What follows the declaration: the class's impls.
    impls: TokenStream,
}

impl ClassTokens {
    fn of(tokens: TokenStream) -> Self {
        let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let Some(declared) = trees
            .iter()
            .position(|tree| matches!(tree, TokenTree::Ident(ident) if ident == "struct"))
        else {
            return Self {
                name: None,
                ivars: None,
                impls: tokens,
            };
        };
        let name = match trees.get(declared + 1) {
            Some(TokenTree::Ident(name)) => Some(name.unraw().to_string()),
            _ => None,
        };
        let ivars = trees[..declared].iter().find_map(|tree| {
            let TokenTree::Group(attribute) = tree else {
                return None;
            };
            if attribute.delimiter() != Delimiter::Bracket {
                return None;
            }
            let mut inner = attribute.stream().into_iter();
            match (inner.next(), inner.next()) {
                (Some(TokenTree::Ident(key)), Some(TokenTree::Punct(equals)))
                    if key == "ivars" && equals.as_char() == '=' =>
                {
                    Some((line(key.span()), inner.collect::<TokenStream>()))
                }
                _ => None,
            }
        });
        let impls = trees[declared..]
            .iter()
            .position(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ';'))
            .map_or(tokens, |semi| {
                trees[declared + semi + 1..]
                    .iter()
                    .cloned()
                    .collect::<TokenStream>()
            });
        Self { name, ivars, impls }
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
            Item::Struct(item) => self.ivars_named(&item.ident, "struct"),
            Item::Enum(item) => self.ivars_named(&item.ident, "enum"),
            Item::Union(item) => self.ivars_named(&item.ident, "union"),
            Item::Type(item) => self.ivars_named(&item.ident, "type"),
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
        self.jni_adapter_named(function);
        if self.nesting == 0 {
            self.out
                .free_fns
                .entry(name)
                .or_default()
                .push((*function.block).clone());
        }
        if foreign(function.sig.abi.as_ref()) {
            self.extern_entry(&function.sig, (*function.block).clone());
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
        self.drop_of(item);
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
        if name == JNI_ADAPTER {
            self.jni_impostor(&function.sig.ident);
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
            if kind == Kind::ExternFn
                && let Some(entry) = self.out.entries.last_mut()
            {
                entry.jni = takes_jni_env(&function.sig);
            }
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
        if function.sig.ident.unraw() == JNI_ADAPTER {
            self.jni_impostor(&function.sig.ident);
        }
        if let Some(block) = &function.default
            && foreign(function.sig.abi.as_ref())
        {
            self.extern_entry(&function.sig, block.clone());
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
        let gcd = GCD_METHODS.iter().any(|gcd| call.method == gcd);
        if gcd || LOOPER_METHODS.iter().any(|looper| call.method == looper) {
            self.closure_entry(
                format!(".{}", call.method),
                line(call.method.span()),
                &call.args,
                gcd,
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
