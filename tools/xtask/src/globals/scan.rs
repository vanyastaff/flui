//! Every static and thread-local one target defines: the module-tree walk,
//! the three-valued cfg evaluator, the item collector, the `thread_local!`
//! body parser, the macro token scan and the counter-use analysis.
//!
//! The walk starts at the target's root file and follows `mod x;` the way
//! rustc does (`x.rs` or `x/mod.rs`, the non-mod-rs directory rule, inline
//! modules, `#[path]`). A `mod` whose cfg is false is not followed, so test
//! files mounted under `#[cfg(test)]` are never read. Everything else fails
//! closed: a declaration that resolves to no file, a file syn cannot parse, a
//! `mod x;` inside a fn, impl or block body, and an `include!` of anything but
//! `concat!(env!("OUT_DIR"), …)` are errors.

use std::collections::BTreeSet;

use anyhow::{Context, anyhow, bail};
use proc_macro2::{Spacing, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::parse::{Parse, ParseStream};
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ImplItem, Item, Lit, Meta, StaticMutability, Stmt, TraitItem, Type,
    Visibility,
};

/// Where the scan reads source: the repository on disk, or the self-test's
/// in-memory crates. Paths are repository-relative and `/`-separated.
pub(super) trait Source {
    /// The file's text, or `None` when there is no such file.
    fn read(&self, rel: &str) -> Option<String>;

    /// Whether the file exists.
    fn exists(&self, rel: &str) -> bool {
        self.read(rel).is_some()
    }
}

/// The atomic integer types a monotonic ID counter may have.
pub(super) const ATOMIC_INTS: [&str; 10] = [
    "AtomicU8",
    "AtomicU16",
    "AtomicU32",
    "AtomicU64",
    "AtomicUsize",
    "AtomicI8",
    "AtomicI16",
    "AtomicI32",
    "AtomicI64",
    "AtomicIsize",
];

/// The primitive types that are immutable data on their own.
const PRIMITIVES: [&str; 17] = [
    "bool", "char", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128",
    "isize", "f32", "f64", "str",
];

/// One crate target to scan.
#[derive(Debug, Clone)]
pub(super) struct Target {
    /// The root file, repository-relative.
    pub(super) src: String,
    /// `Some(name)` for a bin target: its items are keyed `name::…`.
    pub(super) bin: Option<String>,
}

/// How a global is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// `static NAME: T = …;`
    Static,
    /// `static mut NAME: T = …;`
    StaticMut,
    /// An entry of a `thread_local!` invocation.
    ThreadLocal,
    /// A `static` found in the tokens of a macro invocation or a
    /// `macro_rules!` body: FLUI's own macros emit it into whatever crate
    /// invokes them.
    Macro,
}

/// A static's visibility, as far as the counter rule cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Vis {
    /// No `pub`: visible in its module and the module's descendants.
    Private,
    /// `pub(crate)`, `pub(super)`, `pub(in …)`, `pub(self)`.
    Restricted,
    /// Plain `pub`: another crate could reach it.
    Public,
}

/// What the scan knows about a global's type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TypeInfo {
    /// Every identifier in the type, in source order.
    pub(super) idents: Vec<String>,
    /// The last path segment of the outermost type, when it is a path.
    pub(super) outer: Option<String>,
    /// A primitive, a shared reference to immutable data, an array, slice or
    /// tuple of immutable data, or a bare `fn` pointer.
    pub(super) immutable_data: bool,
    /// One of [`ATOMIC_INTS`] under any path prefix.
    pub(super) atomic_int: bool,
}

impl TypeInfo {
    fn of(ty: &Type) -> Self {
        let mut idents = Idents::default();
        idents.visit_type(ty);
        let outer = outer_name(ty);
        Self {
            idents: idents.0,
            atomic_int: outer
                .as_deref()
                .is_some_and(|name| ATOMIC_INTS.contains(&name)),
            outer,
            immutable_data: immutable_data(ty),
        }
    }

    /// From the tokens after `static NAME:` in a macro body; a type naming a
    /// macro metavariable only yields its identifiers.
    fn of_tokens(tokens: TokenStream) -> Self {
        if let Ok(ty) = syn::parse2::<Type>(tokens.clone()) {
            return Self::of(&ty);
        }
        let mut idents = Vec::new();
        token_idents(tokens, &mut idents);
        Self {
            idents,
            ..Self::default()
        }
    }
}

/// How an atomic static is used where it is visible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Uses {
    /// Occurrences of the name that are neither the declaration, nor
    /// `NAME.fetch_add(<integer literal ≥ 1>, …)`, nor a `use` path segment
    /// without `as`. A macro argument naming it counts.
    pub(super) disallowed: usize,
    /// Every method called directly on the name.
    pub(super) methods: BTreeSet<String>,
}

/// One definition of a global.
#[derive(Debug, Clone)]
pub(super) struct Def {
    /// The identity key: the module path inside the target, fn, impl
    /// self-type, trait and macro (`name!`) segments included.
    pub(super) item: String,
    /// The file, repository-relative.
    pub(super) file: String,
    pub(super) shape: Shape,
    pub(super) vis: Vis,
    pub(super) ty: TypeInfo,
    /// The initializer is `<atomic integer>::new(<integer literal>)`.
    pub(super) counter_init: bool,
    /// Every enclosing `#[cfg(…)]` predicate, joined with ` & `.
    pub(super) cfg: String,
    /// Those predicates are false in every build without `debug_assertions`.
    pub(super) debug_only: bool,
    /// For a non-`mut` atomic `static`: how it is used in its visibility
    /// scope.
    pub(super) uses: Option<Uses>,
}

/// What one target defines.
#[derive(Debug)]
pub(super) struct Scanned {
    /// The files the module walk reached and parsed.
    pub(super) files: usize,
    pub(super) defs: Vec<Def>,
}

// ---------------------------------------------------------------------------
// cfg

/// A cfg predicate's value when only `test` is known (false): everything else
/// may be either, so the scan result does not depend on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

/// Evaluates the predicate inside `cfg(…)`.
pub(super) fn eval_cfg(tokens: TokenStream) -> Truth {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    eval_predicate(&trees, Truth::Unknown)
}

/// Whether the conjunction of the cfg predicates `cfgs` is false in every
/// build without `debug_assertions`: the item exists only in debug builds.
fn debug_only<'a>(cfgs: impl IntoIterator<Item = &'a String>) -> bool {
    cfgs.into_iter()
        .map(|text| {
            text.parse::<TokenStream>()
                .map_or(Truth::Unknown, |tokens| {
                    let trees: Vec<TokenTree> = tokens.into_iter().collect();
                    eval_predicate(&trees, Truth::False)
                })
        })
        .fold(Truth::True, Truth::and)
        == Truth::False
}

/// `debug` is the value `debug_assertions` takes.
fn eval_predicate(trees: &[TokenTree], debug: Truth) -> Truth {
    let Some(TokenTree::Ident(name)) = trees.first() else {
        return Truth::Unknown;
    };
    let name = name.to_string();
    match trees.get(1) {
        Some(TokenTree::Group(group)) => {
            let args: Vec<Truth> = split_commas(group.stream())
                .iter()
                .map(|arg| eval_predicate(arg, debug))
                .collect();
            match name.as_str() {
                "all" => args.into_iter().fold(Truth::True, Truth::and),
                "any" => args.into_iter().fold(Truth::False, Truth::or),
                "not" if args.len() == 1 => args[0].not(),
                _ => Truth::Unknown,
            }
        }
        None if name == "test" => Truth::False,
        None if name == "debug_assertions" => debug,
        _ => Truth::Unknown,
    }
}

fn split_commas(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut parts = vec![Vec::new()];
    for tree in tokens {
        match &tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => parts.push(Vec::new()),
            _ => parts
                .last_mut()
                .expect("BUG: parts starts non-empty")
                .push(tree),
        }
    }
    parts.retain(|part| !part.is_empty());
    parts
}

/// The conjunction of the `#[cfg]` attributes in `attrs`, and each
/// predicate's text.
fn cfg_of(attrs: &[Attribute]) -> (Truth, Vec<String>) {
    let mut truth = Truth::True;
    let mut texts = Vec::new();
    for attr in attrs {
        if let Meta::List(list) = &attr.meta
            && list.path.is_ident("cfg")
        {
            truth = truth.and(eval_cfg(list.tokens.clone()));
            texts.push(list.tokens.to_string());
        }
    }
    (truth, texts)
}

/// `#[path = "…"]` on a module declaration.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(pair) if pair.path.is_ident("path") => match &pair.value {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(text) => Some(text.value()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// types

#[derive(Default)]
struct Idents(Vec<String>);

impl Visit<'_> for Idents {
    fn visit_ident(&mut self, ident: &proc_macro2::Ident) {
        self.0.push(ident.unraw().to_string());
    }
}

fn token_idents(tokens: TokenStream, out: &mut Vec<String>) {
    for tree in tokens {
        match tree {
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Group(group) => token_idents(group.stream(), out),
            _ => {}
        }
    }
}

fn outer_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.unraw().to_string()),
        Type::Group(group) => outer_name(&group.elem),
        Type::Paren(paren) => outer_name(&paren.elem),
        _ => None,
    }
}

/// ADR-0097 §2's immutable data.
fn immutable_data(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].arguments.is_none()
                && PRIMITIVES.contains(&path.path.segments[0].ident.to_string().as_str())
        }
        Type::Reference(reference) => {
            reference.mutability.is_none()
                && match &*reference.elem {
                    Type::Slice(slice) => immutable_data(&slice.elem),
                    elem => immutable_data(elem),
                }
        }
        Type::Array(array) => immutable_data(&array.elem),
        Type::Tuple(tuple) => tuple.elems.iter().all(immutable_data),
        Type::FnPtr(_) => true,
        Type::Group(group) => immutable_data(&group.elem),
        Type::Paren(paren) => immutable_data(&paren.elem),
        _ => false,
    }
}

/// `<atomic integer>::new(<integer literal>)`.
fn counter_init(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else { return false };
    let Expr::Path(func) = &*call.func else {
        return false;
    };
    let segments: Vec<String> = func
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let [.., ty, new] = segments.as_slice() else {
        return false;
    };
    new == "new"
        && ATOMIC_INTS.contains(&ty.as_str())
        && call.args.len() == 1
        && matches!(&call.args[0], Expr::Lit(lit) if matches!(lit.lit, Lit::Int(_)))
}

fn vis_of(vis: &Visibility) -> Vis {
    match vis {
        Visibility::Public(_) => Vis::Public,
        Visibility::Restricted(_) => Vis::Restricted,
        Visibility::Inherited => Vis::Private,
    }
}

/// The impl self-type's name, for the key.
fn type_segment(ty: &Type) -> String {
    match ty {
        Type::Reference(reference) => type_segment(&reference.elem),
        Type::Group(group) => type_segment(&group.elem),
        Type::Paren(paren) => type_segment(&paren.elem),
        _ => outer_name(ty).unwrap_or_else(|| "impl".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// attributes of the nodes that carry them

pub(super) fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn expr_attrs(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Assign(expr) => &expr.attrs,
        Expr::Async(expr) => &expr.attrs,
        Expr::Block(expr) => &expr.attrs,
        Expr::Call(expr) => &expr.attrs,
        Expr::Closure(expr) => &expr.attrs,
        Expr::Const(expr) => &expr.attrs,
        Expr::ForLoop(expr) => &expr.attrs,
        Expr::If(expr) => &expr.attrs,
        Expr::Loop(expr) => &expr.attrs,
        Expr::Macro(expr) => &expr.attrs,
        Expr::Match(expr) => &expr.attrs,
        Expr::MethodCall(expr) => &expr.attrs,
        Expr::Path(expr) => &expr.attrs,
        Expr::Return(expr) => &expr.attrs,
        Expr::Unsafe(expr) => &expr.attrs,
        Expr::While(expr) => &expr.attrs,
        _ => &[],
    }
}

fn stmt_attrs(stmt: &Stmt) -> &[Attribute] {
    match stmt {
        Stmt::Local(local) => &local.attrs,
        Stmt::Macro(mac) => &mac.attrs,
        Stmt::Expr(expr, _) => expr_attrs(expr),
        Stmt::Item(_) => &[],
    }
}

// ---------------------------------------------------------------------------
// thread_local!

/// One entry of a `thread_local!` body.
struct LocalEntry {
    attrs: Vec<Attribute>,
    ident: proc_macro2::Ident,
    ty: Type,
}

/// `(Attribute* Visibility static Ident : Type = Expr ;?)*`; `const { … }`
/// initializers parse as a const-block expression.
struct LocalBody(Vec<LocalEntry>);

impl Parse for LocalBody {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut entries = Vec::new();
        while !input.is_empty() {
            let attrs = input.call(Attribute::parse_outer)?;
            let _: Visibility = input.parse()?;
            let _: syn::Token![static] = input.parse()?;
            let ident = input.call(proc_macro2::Ident::parse_any)?;
            let _: syn::Token![:] = input.parse()?;
            let ty: Type = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let _: Expr = input.parse()?;
            entries.push(LocalEntry { attrs, ident, ty });
            if input.is_empty() {
                break;
            }
            let _: syn::Token![;] = input.parse()?;
        }
        Ok(Self(entries))
    }
}

/// One `thread_local!` entry as the collector records it.
struct Local {
    name: String,
    ty: TypeInfo,
    /// The entry's own cfg, and its predicates.
    truth: Truth,
    cfgs: Vec<String>,
}

/// The entries of a `thread_local!` body.
fn thread_local_entries(tokens: TokenStream) -> syn::Result<Vec<Local>> {
    let body: LocalBody = syn::parse2(tokens)?;
    Ok(body
        .0
        .into_iter()
        .map(|entry| {
            let (truth, cfgs) = cfg_of(&entry.attrs);
            Local {
                name: entry.ident.unraw().to_string(),
                ty: TypeInfo::of(&entry.ty),
                truth,
                cfgs,
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// macro tokens

/// Every `static [mut|ref] NAME: <type>` in `tokens` at any depth: `(NAME,
/// type)`, with `$name` for a `macro_rules!` metavariable and `#name` for a
/// `quote!` interpolation. `'static` is a lifetime, not an item.
pub(super) fn macro_statics(tokens: TokenStream) -> Vec<(String, TypeInfo)> {
    let mut found = Vec::new();
    scan_macro_tokens(tokens, &mut found);
    found
}

/// Whether `include!`'s argument is `concat!(env!("OUT_DIR"), …)`: build-script
/// output, which the scan does not claim to cover (ADR-0097, "Limits").
fn includes_out_dir(tokens: TokenStream) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let [
        TokenTree::Ident(concat),
        TokenTree::Punct(bang),
        TokenTree::Group(args),
    ] = &trees[..]
    else {
        return false;
    };
    if concat != "concat" || bang.as_char() != '!' {
        return false;
    }
    let args: Vec<TokenTree> = args.stream().into_iter().take(3).collect();
    matches!(&args[..], [TokenTree::Ident(env), TokenTree::Punct(bang), TokenTree::Group(name)]
        if env == "env"
            && bang.as_char() == '!'
            && name.stream().to_string() == "\"OUT_DIR\"")
}

fn scan_macro_tokens(tokens: TokenStream, found: &mut Vec<(String, TypeInfo)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => scan_macro_tokens(group.stream(), found),
            TokenTree::Ident(ident) if ident == "static" => {
                let lifetime = at > 0
                    && matches!(&trees[at - 1], TokenTree::Punct(punct)
                        if punct.as_char() == '\'' && punct.spacing() == Spacing::Joint);
                if lifetime {
                    continue;
                }
                let mut next = at + 1;
                if matches!(trees.get(next), Some(TokenTree::Ident(word)) if word == "mut" || word == "ref")
                {
                    next += 1;
                }
                let name = match trees.get(next) {
                    Some(TokenTree::Ident(name)) => name.to_string(),
                    // `$name` in `macro_rules!`, `#name` in `quote!`
                    Some(TokenTree::Punct(sigil)) if matches!(sigil.as_char(), '$' | '#') => {
                        next += 1;
                        match trees.get(next) {
                            Some(TokenTree::Ident(name)) => format!("{}{name}", sigil.as_char()),
                            _ => continue,
                        }
                    }
                    _ => continue,
                };
                next += 1;
                let colon = matches!(trees.get(next), Some(TokenTree::Punct(punct))
                    if punct.as_char() == ':' && punct.spacing() == Spacing::Alone);
                if !colon {
                    continue;
                }
                let ty: TokenStream = trees[next + 1..]
                    .iter()
                    .take_while(|tree| {
                        !matches!(tree, TokenTree::Punct(punct)
                            if matches!(punct.as_char(), '=' | ';'))
                    })
                    .cloned()
                    .collect();
                found.push((name, TypeInfo::of_tokens(ty)));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// the module walk

/// How a file's child modules resolve (the Rust reference, "Module Source
/// Filenames"; a `#[path]` file owns its directory like a `mod.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum FileKind {
    ModRs,
    NonModRs(String),
}

/// A `mod x;` declaration the walk follows.
#[derive(Debug, Clone)]
struct ModDecl {
    name: String,
    path_attr: Option<String>,
    /// The inline modules around the declaration, in its file.
    inline: Vec<String>,
    /// The module path of the declared module, from its file's module.
    module: Vec<String>,
    /// The enclosing cfg predicates, which the declared file inherits.
    cfgs: Vec<String>,
}

/// A parsed file of the target.
struct Parsed {
    rel: String,
    module: Vec<String>,
    cfgs: Vec<String>,
    kind: FileKind,
    ast: syn::File,
}

/// Scans one target.
pub(super) fn scan_target(source: &dyn Source, target: &Target) -> anyhow::Result<Scanned> {
    let files = walk(source, target)?;
    let mut raws = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let mut collector = Collector::new(index, file, target.bin.as_deref());
        collector.visit_file(&file.ast);
        if let Some(error) = collector.errors.first() {
            bail!("{}: {error}", file.rel);
        }
        raws.extend(collector.raws);
    }
    let defs = raws
        .into_iter()
        .map(|raw| {
            let uses = (raw.shape == Shape::Static && raw.ty.atomic_int)
                .then(|| uses_in_scope(&files, &raw));
            Def {
                file: files[raw.file].rel.clone(),
                item: raw.item,
                shape: raw.shape,
                vis: raw.vis,
                ty: raw.ty,
                counter_init: raw.counter_init,
                cfg: raw.cfg,
                debug_only: raw.debug_only,
                uses,
            }
        })
        .collect();
    Ok(Scanned {
        files: files.len(),
        defs,
    })
}

/// Parses the target root and every module file it reaches.
fn walk(source: &dyn Source, target: &Target) -> anyhow::Result<Vec<Parsed>> {
    let mut files: Vec<Parsed> = Vec::new();
    let mut queue = vec![(target.src.clone(), Vec::new(), Vec::new(), FileKind::ModRs)];
    while let Some((rel, module, mut cfgs, kind)) = queue.pop() {
        let text = source
            .read(&rel)
            .with_context(|| format!("{rel}: the module walk reached it, but it cannot be read"))?;
        let ast = syn::parse_file(&text)
            .map_err(|error| anyhow!("{rel}: syn cannot parse it: {error}"))?;
        // the file's own `#![cfg(…)]` holds for its items and child modules
        let (truth, inner) = cfg_of(&ast.attrs);
        if truth == Truth::False {
            continue;
        }
        cfgs.extend(inner);
        let mut decls = Decls::default();
        decls.visit_file(&ast);
        let parsed = Parsed {
            rel,
            module,
            cfgs,
            kind,
            ast,
        };
        for decl in decls.found {
            let (child, child_kind) = resolve(source, &parsed, &decl)?;
            if files.iter().any(|file| file.rel == child)
                || queue.iter().any(|queued| queued.0 == child)
            {
                bail!(
                    "{child}: mounted twice in one target (last from {})",
                    parsed.rel
                );
            }
            let mut cfgs = parsed.cfgs.clone();
            cfgs.extend(decl.cfgs);
            let module = parsed.module.iter().cloned().chain(decl.module).collect();
            queue.push((child, module, cfgs, child_kind));
        }
        files.push(parsed);
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// The file a `mod` declaration names.
fn resolve(
    source: &dyn Source,
    file: &Parsed,
    decl: &ModDecl,
) -> anyhow::Result<(String, FileKind)> {
    let dir = parent(&file.rel);
    let mut base = match &file.kind {
        FileKind::ModRs => dir.to_owned(),
        FileKind::NonModRs(stem) => join(dir, stem),
    };
    for inline in &decl.inline {
        base = join(&base, inline);
    }
    if let Some(path) = &decl.path_attr {
        let target = if decl.inline.is_empty() {
            join(dir, path)
        } else {
            join(&base, path)
        };
        if !source.exists(&target) {
            bail!(
                "{}: `#[path = \"{path}\"] mod {};` names {target}, which does not exist",
                file.rel,
                decl.name
            );
        }
        return Ok((target, FileKind::ModRs));
    }
    let flat = join(&base, &format!("{}.rs", decl.name));
    let nested = join(&base, &format!("{}/mod.rs", decl.name));
    match (source.exists(&flat), source.exists(&nested)) {
        (true, false) => Ok((flat, FileKind::NonModRs(decl.name.clone()))),
        (false, true) => Ok((nested, FileKind::ModRs)),
        (true, true) => bail!(
            "{}: `mod {};` is ambiguous: both {flat} and {nested} exist",
            file.rel,
            decl.name
        ),
        (false, false) => bail!(
            "{}: `mod {};` resolves to no file (neither {flat} nor {nested})",
            file.rel,
            decl.name
        ),
    }
}

fn parent(rel: &str) -> &str {
    rel.rfind('/').map_or("", |at| &rel[..at])
}

/// `dir/rel`, with `.` and `..` resolved lexically.
fn join(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// The module declarations of one file, with cfg-false ones left out.
#[derive(Default)]
struct Decls {
    inline: Vec<String>,
    module: Vec<String>,
    cfgs: Vec<String>,
    found: Vec<ModDecl>,
}

impl Visit<'_> for Decls {
    fn visit_item(&mut self, item: &Item) {
        let (truth, texts) = cfg_of(item_attrs(item));
        if truth == Truth::False {
            return;
        }
        let depth = self.cfgs.len();
        self.cfgs.extend(texts);
        if let Item::Mod(module) = item {
            let name = module.ident.unraw().to_string();
            if let Some((_, items)) = &module.content {
                self.inline.push(name.clone());
                self.module.push(name);
                for item in items {
                    self.visit_item(item);
                }
                self.inline.pop();
                self.module.pop();
            } else {
                let mut module_path = self.module.clone();
                module_path.push(name.clone());
                self.found.push(ModDecl {
                    name,
                    path_attr: path_attr(&module.attrs),
                    inline: self.inline.clone(),
                    module: module_path,
                    cfgs: self.cfgs.clone(),
                });
            }
        }
        // Declarations inside fn bodies or impls do not occur in practice
        // and would need their own directory rules; only module items are
        // followed.
        self.cfgs.truncate(depth);
    }
}

// ---------------------------------------------------------------------------
// the collector

/// A definition before its counter uses are known.
struct Raw<'ast> {
    item: String,
    name: String,
    file: usize,
    shape: Shape,
    vis: Vis,
    ty: TypeInfo,
    counter_init: bool,
    cfg: String,
    debug_only: bool,
    /// The body of the innermost enclosing fn, for a static defined in one.
    fn_body: Option<&'ast Block>,
}

struct Collector<'ast> {
    file: usize,
    /// The key prefix: bin name, module path, fn/impl/trait segments.
    path: Vec<String>,
    cfgs: Vec<String>,
    fns: Vec<&'ast Block>,
    /// Whether the items being visited are module items (of the file or of
    /// an inline module in it), the only place the walk follows `mod x;`.
    module_level: bool,
    raws: Vec<Raw<'ast>>,
    errors: Vec<String>,
}

impl<'ast> Collector<'ast> {
    fn new(file: usize, parsed: &Parsed, bin: Option<&str>) -> Self {
        Self {
            file,
            path: bin
                .map(str::to_owned)
                .into_iter()
                .chain(parsed.module.iter().cloned())
                .collect(),
            cfgs: parsed.cfgs.clone(),
            fns: Vec::new(),
            module_level: true,
            raws: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Enters a node with `attrs`: `None` when its cfg is false (skip it),
    /// otherwise how many predicates to pop on leaving.
    fn enter(&mut self, attrs: &[Attribute]) -> Option<usize> {
        let (truth, texts) = cfg_of(attrs);
        if truth == Truth::False {
            return None;
        }
        let pushed = texts.len();
        self.cfgs.extend(texts);
        Some(pushed)
    }

    fn leave(&mut self, pushed: usize) {
        self.cfgs.truncate(self.cfgs.len() - pushed);
    }

    fn key(&self, name: &str) -> String {
        self.path
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(name))
            .collect::<Vec<_>>()
            .join("::")
    }

    fn record(&mut self, raw: Raw<'ast>) {
        self.raws.push(raw);
    }

    fn scan_macro(&mut self, name: &str, tokens: &TokenStream) {
        if name == "include" {
            if !includes_out_dir(tokens.clone()) {
                self.errors.push(format!(
                    "`include!` of `{tokens}` in `{}` names a file the walk does not read; \
                     mount it with `mod` instead",
                    self.path.join("::")
                ));
            }
            return;
        }
        if name == "thread_local" {
            match thread_local_entries(tokens.clone()) {
                Ok(entries) => {
                    for local in entries {
                        if local.truth == Truth::False {
                            continue;
                        }
                        let cfgs: Vec<String> =
                            self.cfgs.iter().cloned().chain(local.cfgs).collect();
                        let (cfg, debug_only) = (cfgs.join(" & "), debug_only(&cfgs));
                        self.record(Raw {
                            item: self.key(&local.name),
                            name: local.name,
                            file: self.file,
                            shape: Shape::ThreadLocal,
                            vis: Vis::Private,
                            ty: local.ty,
                            counter_init: false,
                            cfg,
                            debug_only,
                            fn_body: None,
                        });
                    }
                }
                Err(error) => self.errors.push(format!(
                    "a `thread_local!` body in `{}` does not parse: {error}",
                    self.path.join("::")
                )),
            }
            return;
        }
        let segment = format!("{name}!");
        for (entry, ty) in macro_statics(tokens.clone()) {
            self.path.push(segment.clone());
            let item = self.key(&entry);
            self.path.pop();
            self.record(Raw {
                item,
                name: entry,
                file: self.file,
                shape: Shape::Macro,
                vis: Vis::Private,
                ty,
                counter_init: false,
                cfg: self.cfgs.join(" & "),
                debug_only: debug_only(&self.cfgs),
                fn_body: None,
            });
        }
    }

    fn in_fn(&mut self, name: String, body: Option<&'ast Block>, walk: impl FnOnce(&mut Self)) {
        self.path.push(name);
        let pushed = body.is_some();
        if let Some(body) = body {
            self.fns.push(body);
        }
        walk(self);
        if pushed {
            self.fns.pop();
        }
        self.path.pop();
    }
}

impl<'ast> Visit<'ast> for Collector<'ast> {
    fn visit_item(&mut self, item: &'ast Item) {
        let Some(pushed) = self.enter(item_attrs(item)) else {
            return;
        };
        let module_level = self.module_level;
        if !matches!(item, Item::Mod(_)) {
            self.module_level = false;
        }
        visit::visit_item(self, item);
        self.module_level = module_level;
        self.leave(pushed);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        // A declaration's file is its own walk entry; an inline module nests.
        if let Some((_, items)) = &module.content {
            self.path.push(module.ident.unraw().to_string());
            for item in items {
                self.visit_item(item);
            }
            self.path.pop();
        } else if !self.module_level {
            self.errors.push(format!(
                "`mod {};` in `{}` is not at module level, and the walk does not follow it; \
                 declare it at module level",
                module.ident,
                self.path.join("::")
            ));
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let name = function.sig.ident.unraw().to_string();
        self.in_fn(name, Some(&function.block), |this| {
            visit::visit_item_fn(this, function);
        });
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let Some(pushed) = self.enter(impl_item_attrs(item)) else {
            return;
        };
        visit::visit_impl_item(self, item);
        self.leave(pushed);
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        let name = function.sig.ident.unraw().to_string();
        self.in_fn(name, Some(&function.block), |this| {
            visit::visit_impl_item_fn(this, function);
        });
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let Some(pushed) = self.enter(trait_item_attrs(item)) else {
            return;
        };
        visit::visit_trait_item(self, item);
        self.leave(pushed);
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        let name = function.sig.ident.unraw().to_string();
        self.in_fn(name, function.default.as_ref(), |this| {
            visit::visit_trait_item_fn(this, function);
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.path.push(type_segment(&item.self_ty));
        visit::visit_item_impl(self, item);
        self.path.pop();
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.path.push(item.ident.unraw().to_string());
        visit::visit_item_trait(self, item);
        self.path.pop();
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        let name = item.ident.unraw().to_string();
        self.record(Raw {
            item: self.key(&name),
            name,
            file: self.file,
            shape: match item.mutability {
                StaticMutability::Mut(_) => Shape::StaticMut,
                _ => Shape::Static,
            },
            vis: vis_of(&item.vis),
            ty: TypeInfo::of(&item.ty),
            counter_init: counter_init(&item.expr),
            cfg: self.cfgs.join(" & "),
            debug_only: debug_only(&self.cfgs),
            fn_body: self.fns.last().copied(),
        });
        visit::visit_item_static(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        match &item.ident {
            // `macro_rules! name { … }`: its body is keyed `name!`.
            Some(name) => self.scan_macro(&name.unraw().to_string(), &item.mac.tokens),
            None => self.visit_macro(&item.mac),
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.unraw().to_string());
        self.scan_macro(&name, &mac.tokens);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        let Some(pushed) = self.enter(stmt_attrs(stmt)) else {
            return;
        };
        visit::visit_stmt(self, stmt);
        self.leave(pushed);
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let Some(pushed) = self.enter(&arm.attrs) else {
            return;
        };
        visit::visit_arm(self, arm);
        self.leave(pushed);
    }
}

// ---------------------------------------------------------------------------
// counter uses

/// How `raw` is used where it is visible: the enclosing fn body for a static
/// defined in one; the declaring file and its descendant modules for a
/// private one; every file of the target otherwise.
fn uses_in_scope(files: &[Parsed], raw: &Raw<'_>) -> Uses {
    let mut uses = UseScan {
        name: &raw.name,
        total: 0,
        allowed: 0,
        in_macros: 0,
        methods: BTreeSet::new(),
    };
    if let Some(body) = raw.fn_body {
        uses.visit_block(body);
    } else {
        let declaring = &files[raw.file].module;
        for file in files {
            let visible = raw.vis != Vis::Private || file.module.starts_with(declaring);
            if visible {
                uses.visit_file(&file.ast);
            }
        }
    }
    Uses {
        disallowed: uses.total.saturating_sub(uses.allowed) + uses.in_macros,
        methods: uses.methods,
    }
}

struct UseScan<'a> {
    name: &'a str,
    /// Every identifier equal to the name outside macro tokens.
    total: usize,
    /// Those that are a declaration, a `fetch_add(<literal ≥ 1>)` receiver
    /// or a `use` path segment without `as`.
    allowed: usize,
    /// The name inside a macro's tokens.
    in_macros: usize,
    methods: BTreeSet<String>,
}

impl UseScan<'_> {
    fn names_it(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.last().is_some_and(|segment| segment.ident == self.name))
    }

    fn count_tokens(&mut self, tokens: TokenStream) {
        for tree in tokens {
            match tree {
                TokenTree::Ident(ident) if ident == self.name => self.in_macros += 1,
                TokenTree::Group(group) => self.count_tokens(group.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for UseScan<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if cfg_of(item_attrs(item)).0 != Truth::False {
            visit::visit_item(self, item);
        }
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if cfg_of(stmt_attrs(stmt)).0 != Truth::False {
            visit::visit_stmt(self, stmt);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if cfg_of(impl_item_attrs(item)).0 != Truth::False {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_ident(&mut self, ident: &'ast proc_macro2::Ident) {
        if ident == self.name {
            self.total += 1;
        }
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if item.ident == self.name {
            self.allowed += 1;
        }
        visit::visit_item_static(self, item);
    }

    fn visit_use_name(&mut self, name: &'ast syn::UseName) {
        if name.ident == self.name {
            self.allowed += 1;
        }
        visit::visit_use_name(self, name);
    }

    fn visit_use_path(&mut self, path: &'ast syn::UsePath) {
        if path.ident == self.name {
            self.allowed += 1;
        }
        visit::visit_use_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if self.names_it(&call.receiver) {
            let method = call.method.to_string();
            let positive_literal = matches!(call.args.first(), Some(Expr::Lit(lit))
                if matches!(&lit.lit, Lit::Int(int)
                    if int.base10_parse::<u128>().is_ok_and(|value| value >= 1)));
            if method == "fetch_add" && positive_literal {
                self.allowed += 1;
            }
            self.methods.insert(method);
        }
        visit::visit_expr_method_call(self, call);
    }

    /// Any macro naming it, a `macro_rules!` body included, disqualifies:
    /// what the tokens do with it is not visible here.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.count_tokens(mac.tokens.clone());
        visit::visit_macro(self, mac);
    }
}
