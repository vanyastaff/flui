//! A crate target's module tree as the AST gates read it: where the source
//! comes from (the repository, or a self-test's files in memory), the walk
//! that follows `mod x;` the way rustc does, the three-valued cfg evaluator
//! and the attributes of the nodes that carry them. `globals` and
//! `panic-boundary` read the same tree.
//!
//! The walk starts at the target's root file and follows `mod x;` the way
//! rustc does (`x.rs` or `x/mod.rs`, the non-mod-rs directory rule, inline
//! modules, `#[path]`). A `mod` whose cfg is false is not followed, so test
//! files mounted under `#[cfg(test)]` are never read. A declaration that
//! resolves to no file and a file syn cannot parse are errors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::visit::Visit;
use syn::{Attribute, Expr, ImplItem, Item, Lit, Meta, Stmt, TraitItem};

/// Where the scan reads source: the repository on disk, or the self-test's
/// in-memory crates. Paths are repository-relative and `/`-separated.
pub(crate) trait Source {
    /// The file's text, or `None` when there is no such file.
    fn read(&self, rel: &str) -> Option<String>;

    /// Whether the file exists.
    fn exists(&self, rel: &str) -> bool {
        self.read(rel).is_some()
    }
}

/// The repository on disk, at its root.
pub(crate) struct Disk(pub(crate) PathBuf);

impl Source for Disk {
    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(rel)).ok()
    }

    fn exists(&self, rel: &str) -> bool {
        self.0.join(rel).is_file()
    }
}

/// Files in memory, repository-relative: a self-test's crates.
pub(crate) struct Memory(pub(crate) BTreeMap<String, String>);

impl Source for Memory {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }
}

/// One crate target to scan.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    /// The root file, repository-relative.
    pub(crate) src: String,
    /// `Some(name)` for a bin target: its items are keyed `name::…`.
    pub(crate) bin: Option<String>,
}

// ---------------------------------------------------------------------------
// cfg

/// A cfg predicate's value when only `test` is known (false): everything else
/// may be either, so the scan result does not depend on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    pub(crate) fn and(self, other: Self) -> Self {
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
pub(crate) fn eval_cfg(tokens: TokenStream) -> Truth {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    eval_predicate(&trees, Truth::Unknown)
}

/// `debug` is the value `debug_assertions` takes.
pub(crate) fn eval_predicate(trees: &[TokenTree], debug: Truth) -> Truth {
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
pub(crate) fn cfg_of(attrs: &[Attribute]) -> (Truth, Vec<String>) {
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
// attributes of the nodes that carry them

pub(crate) fn item_attrs(item: &Item) -> &[Attribute] {
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

pub(crate) fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

pub(crate) fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
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

pub(crate) fn stmt_attrs(stmt: &Stmt) -> &[Attribute] {
    match stmt {
        Stmt::Local(local) => &local.attrs,
        Stmt::Macro(mac) => &mac.attrs,
        Stmt::Expr(expr, _) => expr_attrs(expr),
        Stmt::Item(_) => &[],
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
pub(crate) struct Parsed {
    /// Repository-relative, `/`-separated.
    pub(crate) rel: String,
    /// The file's module path from the target root.
    pub(crate) module: Vec<String>,
    pub(crate) cfgs: Vec<String>,
    kind: FileKind,
    pub(crate) ast: syn::File,
}

/// Parses the target root and every module file it reaches, leaving out the
/// ones mounted under a cfg that is false in every build (`test`). The
/// `panic-boundary` gate reads the same tree.
pub(crate) fn walk(source: &dyn Source, target: &Target) -> anyhow::Result<Vec<Parsed>> {
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
