//! One crate's module tree read with syn: its top-level modules, every
//! `crate`-relative path its non-test code names, and the `use` items that
//! relay a name from one module to another.
//!
//! Everything here is pure over [`Sources`], so the tests and the self-test
//! run it on crates that exist only in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, bail};
use proc_macro2::{Spacing, Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItem, Item, Meta, TraitItem, UseTree};

/// Source files by repository-relative, `/`-separated path. A path the map
/// does not hold is read from the repository at `disk`, when one is set.
#[derive(Debug, Default)]
pub(super) struct Sources {
    files: BTreeMap<String, String>,
    disk: Option<PathBuf>,
}

impl Sources {
    /// The files of the repository at `root`.
    pub(super) fn on_disk(root: PathBuf) -> Self {
        Self {
            files: BTreeMap::new(),
            disk: Some(root),
        }
    }

    /// These sources with `rel` holding `text` instead.
    pub(super) fn with(mut self, rel: &str, text: &str) -> Self {
        self.files.insert(rel.to_owned(), text.to_owned());
        self
    }

    /// The text of `rel`, `None` when there is no such file.
    pub(super) fn read(&self, rel: &str) -> anyhow::Result<Option<String>> {
        if let Some(text) = self.files.get(rel) {
            return Ok(Some(text.clone()));
        }
        let Some(root) = &self.disk else {
            return Ok(None);
        };
        let path = root.join(rel);
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .map(Some)
            .with_context(|| format!("reading {}", path.display()))
    }
}

/// Where a path is named: file, line and the path as written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Site {
    pub(super) file: String,
    pub(super) line: usize,
    pub(super) text: String,
}

impl std::fmt::Display for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} `{}`", self.file, self.line, self.text)
    }
}

/// A path named by the non-test code of the top-level module `from`, made
/// absolute: its segments from the crate root.
#[derive(Debug, Clone)]
pub(super) struct Reference {
    pub(super) from: String,
    pub(super) path: Vec<String>,
    pub(super) site: Site,
}

/// What a `use` item binds a name to, or what a glob import reads.
#[derive(Debug, Clone)]
enum Target {
    /// A path from the crate root.
    Absolute(Vec<String>),
    /// A path whose first segment is a plain name: in the root module a
    /// top-level item, a name another root `use` binds, or an external crate;
    /// anywhere else a child item or an external crate.
    Plain(Vec<String>),
    /// A `::`-prefixed path: always another crate.
    External,
}

/// The non-test `use` items directly in one module.
#[derive(Debug, Default)]
struct UseTable {
    names: BTreeMap<String, Target>,
    globs: Vec<Target>,
}

/// What an absolute path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Resolved {
    /// An item of this top-level module.
    Module(String),
    /// An item defined in the crate root itself (`lib.rs`), not in a module.
    Root(String),
    /// Something outside the crate.
    External,
    /// Nothing the scan can attribute; the text says why.
    Unattributed(String),
}

/// One crate's module tree, as the direction rule reads it.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// The top-level modules compiled outside tests, with the file of each.
    pub(super) modules: BTreeMap<String, String>,
    /// Every `crate`-relative path the non-test code of a top-level module names.
    pub(super) references: Vec<Reference>,
    /// The first item other than a `use` directly in each top-level module.
    pub(super) first_item: BTreeMap<String, Site>,
    root_uses: UseTable,
    module_uses: BTreeMap<String, UseTable>,
    /// `#[macro_export]` macros (which live at the crate root) and the
    /// top-level module that defines each; `""` for the root itself.
    macro_exports: BTreeMap<String, String>,
    /// Items defined in the crate root itself.
    root_items: BTreeSet<String>,
}

/// Reads the crate whose root file is `lib` (repository-relative), following
/// `mod` declarations. A file that does not parse, or a `mod x;` with no file,
/// is an error.
pub(super) fn scan(sources: &Sources, lib: &str) -> anyhow::Result<Scan> {
    let file = parse(sources, lib)?.with_context(|| format!("{lib} does not exist"))?;
    let aliases = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ExternCrate(krate) if krate.ident == "self" && !implies_test(&krate.attrs) => {
                krate.rename.as_ref().map(|(_, name)| name.to_string())
            }
            _ => None,
        })
        .collect();
    let mut loader = Loader {
        sources,
        aliases,
        scan: Scan::default(),
    };
    loader.items(&[], &file.items, lib, &parent(lib), false)?;
    Ok(loader.scan)
}

fn parse(sources: &Sources, rel: &str) -> anyhow::Result<Option<syn::File>> {
    let Some(text) = sources.read(rel)? else {
        return Ok(None);
    };
    syn::parse_file(&text)
        .map(Some)
        .map_err(|error| {
            let at = error.span().start();
            anyhow::anyhow!("{rel}:{}:{}: {error}", at.line, at.column + 1)
        })
        .context("module-dag reads every module with syn; a file it cannot parse stops the gate")
}

/// The directory of `rel`, with a trailing `/` unless empty.
fn parent(rel: &str) -> String {
    rel.rfind('/')
        .map_or_else(String::new, |at| rel[..=at].to_owned())
}

struct Loader<'a> {
    sources: &'a Sources,
    /// `extern crate self as NAME;`: names that mean `crate`.
    aliases: Vec<String>,
    scan: Scan,
}

impl Loader<'_> {
    /// Walks the items of `module` (its path from the root), which live in
    /// `file`; a `mod x;` among them lives under `dir`.
    fn items(
        &mut self,
        module: &[String],
        items: &[Item],
        file: &str,
        dir: &str,
        test_only: bool,
    ) -> anyhow::Result<()> {
        for item in items {
            let test_only = test_only || implies_test(item_attrs(item));
            if let [top] = module
                && !matches!(item, Item::Use(_))
            {
                self.scan
                    .first_item
                    .entry(top.clone())
                    .or_insert_with(|| Site {
                        file: file.to_owned(),
                        line: item_line(item),
                        text: item_kind(item).to_owned(),
                    });
            }
            match item {
                Item::Mod(child) => {
                    self.module(module, child, file, dir, test_only)?;
                    continue;
                }
                Item::Use(item_use) if !test_only && module.len() <= 1 => {
                    let table = match module.first() {
                        None => &mut self.scan.root_uses,
                        Some(top) => self.scan.module_uses.entry(top.clone()).or_default(),
                    };
                    for leaf in use_leaves(&item_use.tree) {
                        let target = if item_use.leading_colon.is_some() {
                            Target::External
                        } else if let Some(path) = absolute(module, &leaf.path, &self.aliases)? {
                            Target::Absolute(path)
                        } else {
                            Target::Plain(leaf.path)
                        };
                        match leaf.binds {
                            Some(name) if name != "_" => {
                                table.names.insert(name, target);
                            }
                            Some(_) => {}
                            None => table.globs.push(target),
                        }
                    }
                }
                Item::Macro(mac)
                    if !test_only
                        && mac.ident.is_some()
                        && mac
                            .attrs
                            .iter()
                            .any(|attr| attr.path().is_ident("macro_export")) =>
                {
                    let name = mac
                        .ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let owner = module.first().cloned().unwrap_or_default();
                    self.scan.macro_exports.insert(name, owner);
                }
                _ => {}
            }
            if test_only {
                continue;
            }
            if module.is_empty() {
                if let Some(name) = defined_name(item) {
                    self.scan.root_items.insert(name);
                }
                continue;
            }
            let mut collector = Collector {
                module,
                file,
                aliases: &self.aliases,
                references: &mut self.scan.references,
                error: None,
            };
            collector.visit_item(item);
            if let Some(error) = collector.error {
                return Err(error);
            }
        }
        Ok(())
    }

    /// A `mod` item inside `module`, whose own items live in `file` and
    /// whose `mod x;` children live under `dir`.
    fn module(
        &mut self,
        module: &[String],
        child: &syn::ItemMod,
        file: &str,
        dir: &str,
        test_only: bool,
    ) -> anyhow::Result<()> {
        let name = child.ident.to_string();
        let path: Vec<String> = module.iter().cloned().chain([name.clone()]).collect();
        let line = child.mod_token.span.start().line;
        let child_file = if let Some((_, items)) = &child.content {
            self.items(&path, items, file, &format!("{dir}{name}/"), test_only)?;
            file.to_owned()
        } else {
            let custom = path_attr(&child.attrs)?;
            let candidates = match (&custom, module.is_empty()) {
                (Some(custom), true) => vec![format!("{dir}{custom}")],
                (Some(_), false) => bail!(
                    "{file}:{line}: `#[path]` on the nested module `{}` is not supported; \
                     module-dag honours it only on a top-level `mod`",
                    path.join("::")
                ),
                (None, _) => vec![format!("{dir}{name}.rs"), format!("{dir}{name}/mod.rs")],
            };
            let mut found = None;
            for candidate in &candidates {
                if let Some(parsed) = parse(self.sources, candidate)? {
                    found = Some((candidate.clone(), parsed));
                    break;
                }
            }
            let Some((child_file, parsed)) = found else {
                bail!(
                    "{file}:{line}: `mod {name};` has no file (looked for {})",
                    candidates.join(" and ")
                );
            };
            // A `mod.rs` or `#[path]` file keeps its children beside it;
            // `x.rs` keeps them in `x/`.
            let child_dir = if custom.is_some() || child_file.ends_with("/mod.rs") {
                parent(&child_file)
            } else {
                format!("{dir}{name}/")
            };
            let test_only = test_only || implies_test(&parsed.attrs);
            self.items(&path, &parsed.items, &child_file, &child_dir, test_only)?;
            child_file
        };
        if module.is_empty() && !test_only {
            self.scan.modules.insert(name, child_file);
        }
        Ok(())
    }
}

/// `#[path = "..."]`, when present.
fn path_attr(attrs: &[Attribute]) -> anyhow::Result<Option<String>> {
    for attr in attrs {
        if let Meta::NameValue(pair) = &attr.meta
            && pair.path.is_ident("path")
        {
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) = &pair.value
            else {
                bail!("`#[path]` takes a string literal");
            };
            return Ok(Some(text.value().replace('\\', "/")));
        }
    }
    Ok(None)
}

/// Whether the attributes compile the item only under `cfg(test)`: a `cfg`
/// whose predicate implies `test`.
pub(super) fn implies_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|predicate| predicate_implies_test(&predicate))
    })
}

/// `test`, or `all(...)` with a member that implies `test`. `any(test, ...)`
/// and `not(...)` do not.
fn predicate_implies_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
            .is_ok_and(|members| members.iter().any(predicate_implies_test)),
        _ => false,
    }
}

/// The attributes of an item.
fn item_attrs(item: &Item) -> &[Attribute] {
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

/// The line an item starts on (its keyword).
fn item_line(item: &Item) -> usize {
    let span = match item {
        Item::Const(item) => item.const_token.span,
        Item::Enum(item) => item.enum_token.span,
        Item::ExternCrate(item) => item.extern_token.span,
        Item::Fn(item) => item.sig.fn_token.span,
        Item::ForeignMod(item) => item.abi.extern_token.span,
        Item::Impl(item) => item.impl_token.span,
        Item::Macro(item) => item.mac.bang_token.span,
        Item::Mod(item) => item.mod_token.span,
        Item::Static(item) => item.static_token.span,
        Item::Struct(item) => item.struct_token.span,
        Item::Trait(item) => item.trait_token.span,
        Item::TraitAlias(item) => item.trait_token.span,
        Item::Type(item) => item.type_token.span,
        Item::Union(item) => item.union_token.span,
        Item::Use(item) => item.use_token.span,
        _ => return 0,
    };
    span.start().line
}

/// A word for the kind of an item, for a finding.
fn item_kind(item: &Item) -> &'static str {
    match item {
        Item::Const(_) => "const",
        Item::Enum(_) => "enum",
        Item::ExternCrate(_) => "extern crate",
        Item::Fn(_) => "fn",
        Item::ForeignMod(_) => "extern block",
        Item::Impl(_) => "impl",
        Item::Macro(_) => "macro",
        Item::Mod(_) => "mod",
        Item::Static(_) => "static",
        Item::Struct(_) => "struct",
        Item::Trait(_) | Item::TraitAlias(_) => "trait",
        Item::Type(_) => "type",
        Item::Union(_) => "union",
        Item::Use(_) => "use",
        _ => "item",
    }
}

/// The name an item defines in the root module, for items a path can name.
fn defined_name(item: &Item) -> Option<String> {
    let ident = match item {
        Item::Const(item) => &item.ident,
        Item::Enum(item) => &item.ident,
        Item::Fn(item) => &item.sig.ident,
        Item::Macro(item) => item.ident.as_ref()?,
        Item::Static(item) => &item.ident,
        Item::Struct(item) => &item.ident,
        Item::Trait(item) => &item.ident,
        Item::TraitAlias(item) => &item.ident,
        Item::Type(item) => &item.ident,
        Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.to_string())
}

/// One import of a `use` tree.
struct UseLeaf {
    /// The path as written, `self` leaves folded into their prefix; a glob's
    /// path is the module it reads.
    path: Vec<String>,
    /// The name it binds; `None` for a glob.
    binds: Option<String>,
    line: usize,
}

fn use_leaves(tree: &UseTree) -> Vec<UseLeaf> {
    fn walk(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<UseLeaf>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                walk(&path.tree, prefix, out);
                prefix.pop();
            }
            UseTree::Name(name) => {
                let (path, binds) = leaf(prefix, &name.ident, None);
                out.push(UseLeaf {
                    path,
                    binds: Some(binds),
                    line: name.ident.span().start().line,
                });
            }
            UseTree::Rename(rename) => {
                let (path, binds) = leaf(prefix, &rename.ident, Some(&rename.rename));
                out.push(UseLeaf {
                    path,
                    binds: Some(binds),
                    line: rename.ident.span().start().line,
                });
            }
            UseTree::Glob(glob) => out.push(UseLeaf {
                path: prefix.clone(),
                binds: None,
                line: glob.star_token.spans[0].start().line,
            }),
            UseTree::Group(group) => {
                for tree in &group.items {
                    walk(tree, prefix, out);
                }
            }
        }
    }
    fn leaf(
        prefix: &[String],
        ident: &syn::Ident,
        rename: Option<&syn::Ident>,
    ) -> (Vec<String>, String) {
        let mut path = prefix.to_vec();
        if ident != "self" {
            path.push(ident.to_string());
        }
        let binds = rename.map_or_else(
            || path.last().cloned().unwrap_or_default(),
            ToString::to_string,
        );
        (path, binds)
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), &mut out);
    out
}

/// `path`, named inside `module`, as segments from the crate root: `crate::`,
/// `$crate::` and an `extern crate self` alias start at the root, `self::` at
/// `module`, each `super::` one module up. `None` for a path that starts with
/// any other name: under uniform paths that is an item in scope, which stays
/// in the module or leaves the crate.
fn absolute(
    module: &[String],
    path: &[String],
    aliases: &[String],
) -> anyhow::Result<Option<Vec<String>>> {
    let Some(first) = path.first() else {
        return Ok(None);
    };
    Ok(Some(match first.as_str() {
        "crate" | "$crate" => path[1..].to_vec(),
        "self" => module.iter().chain(&path[1..]).cloned().collect(),
        "super" => {
            let ups = path
                .iter()
                .take_while(|segment| *segment == "super")
                .count();
            let Some(kept) = module.len().checked_sub(ups) else {
                bail!(
                    "`{}` in `{}` climbs above the crate root",
                    path.join("::"),
                    if module.is_empty() {
                        "crate".to_owned()
                    } else {
                        module.join("::")
                    }
                );
            };
            module[..kept].iter().chain(&path[ups..]).cloned().collect()
        }
        name if aliases.iter().any(|alias| alias == name) => path[1..].to_vec(),
        _ => return Ok(None),
    }))
}

/// Collects the references of one item of a top-level module.
struct Collector<'a> {
    module: &'a [String],
    file: &'a str,
    aliases: &'a [String],
    references: &'a mut Vec<Reference>,
    error: Option<anyhow::Error>,
}

impl Collector<'_> {
    fn record(&mut self, path: Vec<String>, line: usize) {
        match absolute(self.module, &path, self.aliases) {
            Ok(Some(absolute)) => self.references.push(Reference {
                from: self.module[0].clone(),
                path: absolute,
                site: Site {
                    file: self.file.to_owned(),
                    line,
                    text: path.join("::"),
                },
            }),
            Ok(None) => {}
            Err(error) => {
                if self.error.is_none() {
                    self.error = Some(error.context(format!("{}:{line}", self.file)));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !implies_test(item_attrs(item)) {
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
        if !implies_test(attrs) {
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
        if !implies_test(attrs) {
            visit::visit_trait_item(self, item);
        }
    }

    /// Each import of the tree once; a glob names the module it reads.
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if item.leading_colon.is_some() {
            return;
        }
        for leaf in use_leaves(&item.tree) {
            self.record(leaf.path, leaf.line);
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.leading_colon.is_none()
            && let Some(first) = path.segments.first()
        {
            let segments = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            self.record(segments, first.ident.span().start().line);
        }
        visit::visit_path(self, path);
    }

    /// `pub(in crate::x)` says who may name the item, not what it depends on.
    fn visit_vis_restricted(&mut self, _: &'ast syn::VisRestricted) {}

    /// Doc comments and other attributes name no dependency.
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    /// The macro's own path, and each path in its tokens (a `macro_rules!`
    /// body or an invocation's arguments).
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.visit_path(&mac.path);
        let mut found = Vec::new();
        token_paths(mac.tokens.clone(), self.aliases, &mut found);
        for (path, span) in found {
            self.record(path, span.start().line);
        }
    }
}

/// The paths in `tokens` that start at `crate`, `$crate`, `self`, `super` or
/// an alias and continue with `::name`, groups included.
fn token_paths(tokens: TokenStream, aliases: &[String], out: &mut Vec<(Vec<String>, Span)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut at = 0;
    while at < trees.len() {
        let start = match &trees[at] {
            TokenTree::Group(group) => {
                token_paths(group.stream(), aliases, out);
                None
            }
            TokenTree::Punct(dollar) if dollar.as_char() == '$' => match trees.get(at + 1) {
                Some(TokenTree::Ident(ident)) if ident == "crate" => {
                    at += 1;
                    Some(("$crate".to_owned(), ident.span()))
                }
                _ => None,
            },
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                let starts =
                    matches!(name.as_str(), "crate" | "self" | "super") || aliases.contains(&name);
                starts.then(|| (name, ident.span()))
            }
            _ => None,
        };
        at += 1;
        let Some((first, span)) = start else {
            continue;
        };
        let mut path = vec![first];
        while let (
            Some(TokenTree::Punct(a)),
            Some(TokenTree::Punct(b)),
            Some(TokenTree::Ident(next)),
        ) = (trees.get(at), trees.get(at + 1), trees.get(at + 2))
        {
            if a.as_char() != ':' || a.spacing() != Spacing::Joint || b.as_char() != ':' {
                break;
            }
            path.push(next.to_string());
            at += 3;
        }
        if path.len() > 1 {
            out.push((path, span));
        }
    }
}

impl Scan {
    /// The node an absolute path names. A path through a `transparent`
    /// module, a root re-export, a glob re-export or a `#[macro_export]`
    /// macro counts as a path to what it relays.
    pub(super) fn resolve(&self, path: &[String], transparent: &BTreeSet<String>) -> Resolved {
        self.resolve_in(path.to_vec(), false, transparent, &mut BTreeSet::new())
    }

    /// `plain`: `path` came from a root `use` whose first segment was a plain
    /// name, which may name another crate.
    fn resolve_in(
        &self,
        path: Vec<String>,
        plain: bool,
        transparent: &BTreeSet<String>,
        seen: &mut BTreeSet<Vec<String>>,
    ) -> Resolved {
        if !seen.insert(path.clone()) {
            return Resolved::Unattributed("its re-exports form a loop".to_owned());
        }
        let Some(first) = path.first() else {
            return Resolved::Unattributed("it names the crate root itself".to_owned());
        };
        if !self.modules.contains_key(first) {
            return self.through(None, first, &path[1..], plain, transparent, seen);
        }
        if !transparent.contains(first) {
            return Resolved::Module(first.clone());
        }
        let Some(name) = path.get(1) else {
            return Resolved::Unattributed(format!(
                "it names the transparent module `{first}` itself"
            ));
        };
        self.through(Some(first), name, &path[2..], false, transparent, seen)
    }

    /// `name` looked up in the `use` table of `module` (the root when
    /// `None`), followed by `rest`.
    fn through(
        &self,
        module: Option<&String>,
        name: &str,
        rest: &[String],
        plain: bool,
        transparent: &BTreeSet<String>,
        seen: &mut BTreeSet<Vec<String>>,
    ) -> Resolved {
        let empty = UseTable::default();
        let table = match module {
            None => &self.root_uses,
            Some(module) => self.module_uses.get(module).unwrap_or(&empty),
        };
        // A plain path relayed by the root starts at the root; elsewhere it
        // names another crate (a relay module holds no items of its own).
        let follow =
            |target: &Target, tail: Vec<String>, seen: &mut BTreeSet<Vec<String>>| match target {
                Target::Absolute(path) => self.resolve_in(
                    path.iter().cloned().chain(tail).collect(),
                    false,
                    transparent,
                    seen,
                ),
                Target::Plain(path) if module.is_none() => self.resolve_in(
                    path.iter().cloned().chain(tail).collect(),
                    true,
                    transparent,
                    seen,
                ),
                Target::Plain(_) | Target::External => Resolved::External,
            };
        if let Some(target) = table.names.get(name) {
            return follow(target, rest.to_vec(), seen);
        }
        if module.is_none() {
            if let Some(owner) = self.macro_exports.get(name) {
                return if owner.is_empty() {
                    Resolved::Root(name.to_owned())
                } else {
                    Resolved::Module(owner.clone())
                };
            }
            if self.root_items.contains(name) {
                return Resolved::Root(name.to_owned());
            }
        }
        if plain {
            // A plain first segment nothing here binds is another crate's name.
            return Resolved::External;
        }
        let internal: Vec<&Target> = table
            .globs
            .iter()
            .filter(|glob| match glob {
                Target::Absolute(_) => true,
                Target::Plain(_) => module.is_none(),
                Target::External => false,
            })
            .collect();
        let tail = || {
            std::iter::once(name.to_owned())
                .chain(rest.iter().cloned())
                .collect()
        };
        match internal.as_slice() {
            [glob] => return follow(glob, tail(), seen),
            [_, _, ..] => {
                return Resolved::Unattributed(format!(
                    "more than one glob re-export in `{}` could supply `{name}`",
                    module.map_or("crate", String::as_str)
                ));
            }
            [] => {}
        }
        if table
            .globs
            .iter()
            .any(|glob| !matches!(glob, Target::Absolute(_)))
        {
            return Resolved::External;
        }
        Resolved::Unattributed(format!(
            "`{}` has no module, re-export or item `{name}`",
            module.map_or("crate", String::as_str)
        ))
    }
}
