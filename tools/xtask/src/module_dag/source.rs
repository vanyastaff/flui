//! One crate's module tree read with syn: its modules at every depth, every
//! `crate`-relative path its non-test code names with the module that names
//! it, and the `use` items that relay a name from one module to another.
//! Conditional bindings retain every source, so every possible owning module
//! contributes an edge.
//!
//! A path resolves to the module that owns the item it names, and how far the
//! resolution reads into the tree is the caller's choice ([`Reach`]): through
//! the crate root and the transparent top-level modules only, so any other
//! top-level module owns what is named in it, or through every module, so the
//! answer is the deepest module that defines the item.
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

/// A path named by the non-test code of the module `from` (its segments from
/// the crate root; the first is the top-level module), made absolute: its
/// segments from the crate root.
#[derive(Debug, Clone)]
pub(super) struct Reference {
    pub(super) from: Vec<String>,
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
    /// A `::`-prefixed path that does not start at an `extern crate self`
    /// alias: another crate.
    External,
}

/// The non-test `use` items directly in one module.
#[derive(Debug, Default)]
struct UseTable {
    // Non-test cfg alternatives must not overwrite each other.
    names: BTreeMap<String, Vec<Target>>,
    globs: Vec<Target>,
}

/// What an absolute path resolves to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Resolved {
    /// An item of this module, as a path of module names from the crate root:
    /// one name when the resolution stopped at a top-level module.
    Module(Vec<String>),
    /// An item defined in the crate root itself (`lib.rs`), not in a module.
    Root(String),
    /// Something outside the crate.
    External,
    /// Nothing the scan can attribute; the text says why.
    Unattributed(String),
    /// The name is in no child module, `use` item, item or glob re-export of
    /// the module it was looked up in. A glob re-export that supplies the name
    /// outranks it; the owner of a name nothing supplies is unattributed.
    Missing(String),
}

/// One crate's module tree, as the direction rule reads it.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// The top-level modules compiled outside tests, with the file of each.
    pub(super) modules: BTreeMap<String, String>,
    /// Every module compiled outside tests, nested ones included, by its
    /// path from the root, with the file it lives in (an inline module's is
    /// the file around it).
    pub(super) tree: BTreeMap<Vec<String>, String>,
    /// Every `crate`-relative path the non-test code of a module names.
    pub(super) references: Vec<Reference>,
    /// The first item other than a `use` directly in each top-level module.
    pub(super) first_item: BTreeMap<String, Site>,
    /// The non-test `use` items of each module; the root is `[]`.
    uses: BTreeMap<Vec<String>, UseTable>,
    /// The names of the non-test items each module defines.
    defined: BTreeMap<Vec<String>, BTreeSet<String>>,
    /// Modules holding an item whose expansion or declaration the scan cannot
    /// read (a macro invocation, an `extern` block): any name may be defined
    /// there.
    opaque: BTreeSet<Vec<String>>,
    /// `#[macro_export]` macros (which live at the crate root) and the
    /// module that defines each; `[]` for the root itself.
    macro_exports: BTreeMap<String, Vec<String>>,
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
    loader.items(&[], &file.items, lib, &parent(lib), false, false)?;
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
    /// `file`; a `mod x;` among them lives under `dir`. `inline`: the items
    /// are those of an inline `mod x { .. }` block, not of the file itself.
    fn items(
        &mut self,
        module: &[String],
        items: &[Item],
        file: &str,
        dir: &str,
        inline: bool,
        test_only: bool,
    ) -> anyhow::Result<()> {
        for item in items {
            let test_only = test_only || implies_test(item_attrs(item));
            let test_only = if let Item::Mod(child) = item {
                !self.module(module, child, file, dir, inline, test_only)?
            } else {
                test_only
            };
            if let [top] = module
                && !test_only
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
                Item::Mod(_) => continue,
                Item::Use(item_use) if !test_only => {
                    let table = self.scan.uses.entry(module.to_vec()).or_default();
                    for leaf in use_leaves(&item_use.tree) {
                        let target = if other_crate(
                            item_use.leading_colon.is_some(),
                            leaf.path.first(),
                            &self.aliases,
                        ) {
                            Target::External
                        } else if let Some(path) = absolute(module, &leaf.path, &self.aliases)? {
                            Target::Absolute(path)
                        } else {
                            Target::Plain(leaf.path)
                        };
                        match leaf.binds {
                            Some(name) if name != "_" => {
                                table.names.entry(name).or_default().push(target);
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
                    let name = mac.ident.as_ref().map_or_default(ToString::to_string);
                    self.scan.macro_exports.insert(name, module.to_vec());
                }
                _ => {}
            }
            if test_only {
                continue;
            }
            if let Some(name) = defined_name(item) {
                self.scan
                    .defined
                    .entry(module.to_vec())
                    .or_default()
                    .insert(name);
            } else if hides_names(item) {
                self.scan.opaque.insert(module.to_vec());
            }
            if module.is_empty() {
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
    /// whose `mod x;` children live under `dir`; `inline` as for
    /// [`Self::items`]. Returns whether the module is compiled outside tests
    /// after both outer and file-level cfgs.
    fn module(
        &mut self,
        module: &[String],
        child: &syn::ItemMod,
        file: &str,
        dir: &str,
        inline: bool,
        mut test_only: bool,
    ) -> anyhow::Result<bool> {
        let name = child.ident.to_string();
        let path: Vec<String> = module.iter().cloned().chain([name.clone()]).collect();
        let line = child.mod_token.span.start().line;
        let child_file = if let Some((_, items)) = &child.content {
            self.items(
                &path,
                items,
                file,
                &format!("{dir}{name}/"),
                true,
                test_only,
            )?;
            file.to_owned()
        } else {
            let custom = path_attr(&child.attrs)?;
            let candidates = match &custom {
                // Relative to the directory of the file that declares the
                // module; inside an inline block, to the block's directory.
                Some(custom) if inline => vec![format!("{dir}{custom}")],
                Some(custom) => vec![format!("{}{custom}", parent(file))],
                None => vec![format!("{dir}{name}.rs"), format!("{dir}{name}/mod.rs")],
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
            // `#![cfg(test)]` at the top of the file gates the whole module.
            test_only = test_only || implies_test(&parsed.attrs);
            self.items(
                &path,
                &parsed.items,
                &child_file,
                &child_dir,
                false,
                test_only,
            )?;
            child_file
        };
        if !test_only {
            if module.is_empty() {
                self.scan.modules.insert(name, child_file.clone());
            }
            self.scan.tree.insert(path, child_file);
        }
        Ok(!test_only)
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

/// Whether the item may define names the scan cannot read: a macro
/// invocation at item position (not a `macro_rules!` definition), the items of
/// an `extern` block, an `extern crate` or tokens syn did not parse.
fn hides_names(item: &Item) -> bool {
    match item {
        Item::Macro(mac) => mac.ident.is_none(),
        Item::ForeignMod(_) | Item::ExternCrate(_) | Item::Verbatim(_) => true,
        _ => false,
    }
}

/// The name an item defines in its module, for items a path can name.
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

/// Whether a path, `::`-prefixed when `leading_colon`, names another crate
/// for certain: `::name` does, unless `name` is an `extern crate self` alias,
/// which the extern prelude resolves to this crate.
fn other_crate(leading_colon: bool, first: Option<&String>, aliases: &[String]) -> bool {
    leading_colon && !first.is_some_and(|first| aliases.contains(first))
}

/// Collects the references of one item of a module.
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
                from: self.module.to_vec(),
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
        for leaf in use_leaves(&item.tree) {
            if !other_crate(
                item.leading_colon.is_some(),
                leaf.path.first(),
                self.aliases,
            ) {
                self.record(leaf.path, leaf.line);
            }
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(first) = path.segments.first() {
            let segments: Vec<String> = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if !other_crate(path.leading_colon.is_some(), segments.first(), self.aliases) {
                self.record(segments, first.ident.span().start().line);
            }
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

/// How far [`Scan::resolve`] reads into the module tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Depth {
    /// The crate root and the transparent top-level modules: any other
    /// top-level module owns whatever is named in it.
    Top,
    /// Every module: a path ends in the deepest module that owns the item.
    Full,
}

/// What [`Scan::resolve`] follows a path through.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reach<'a> {
    pub(super) depth: Depth,
    /// The top-level modules of `use` items only: a path through one counts as
    /// a path to what it re-exports, and naming one itself is unattributed.
    pub(super) transparent: &'a BTreeSet<String>,
}

impl Reach<'_> {
    /// Whether a path that reaches `module` is read further inside it.
    fn opens(&self, module: &[String]) -> bool {
        match (self.depth, module) {
            (_, []) | (Depth::Full, _) => true,
            (Depth::Top, [top]) => self.transparent.contains(top),
            (Depth::Top, _) => false,
        }
    }

    /// Whether the child modules of `module` are looked up by name.
    fn descends(&self, module: &[String]) -> bool {
        module.is_empty() || self.depth == Depth::Full
    }

    /// `owner` as far as this reach attributes: a top-level module under
    /// [`Depth::Top`].
    fn clip(&self, owner: &[String]) -> Vec<String> {
        match self.depth {
            Depth::Top => owner.iter().take(1).cloned().collect(),
            Depth::Full => owner.to_vec(),
        }
    }
}

/// The lookups under way, as `module ++ segments`.
type Seen = BTreeSet<Vec<String>>;

/// What a lookup answers when it comes back to one already under way.
#[derive(Debug, Clone, Copy)]
enum Cycle {
    /// Named re-exports that lead to each other: nothing owns the name.
    Unattributed,
    /// Glob re-exports that read each other: that branch supplies nothing.
    Missing,
}

impl Scan {
    /// Every owner an absolute path can name. A path through a root
    /// re-export, a glob re-export, a `#[macro_export]` macro or, as far as
    /// `reach` reads, a module's `use` items counts as a path to what it
    /// relays.
    pub(super) fn resolve(&self, path: &[String], reach: Reach<'_>) -> BTreeSet<Resolved> {
        Resolver { scan: self, reach }.enter(
            &[],
            path.to_vec(),
            false,
            Cycle::Unattributed,
            &mut BTreeSet::new(),
        )
    }
}

/// [`Scan::resolve`] for one `reach`.
struct Resolver<'a> {
    scan: &'a Scan,
    reach: Reach<'a>,
}

/// A module path for a finding: `crate` for the root.
fn label(module: &[String]) -> String {
    if module.is_empty() {
        "crate".to_owned()
    } else {
        module.join("::")
    }
}

impl Resolver<'_> {
    /// Looks `segments` up from `module`. `plain`: `segments` came from a
    /// `use` whose first segment was a plain name, which may name another
    /// crate.
    fn enter(
        &self,
        module: &[String],
        segments: Vec<String>,
        plain: bool,
        cycle: Cycle,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        let key: Vec<String> = module.iter().chain(&segments).cloned().collect();
        if !seen.insert(key) {
            return BTreeSet::from([match cycle {
                Cycle::Unattributed => {
                    Resolved::Unattributed("its re-exports form a loop".to_owned())
                }
                Cycle::Missing => {
                    Resolved::Missing("its glob re-exports read each other".to_owned())
                }
            }]);
        }
        self.walk(module, &segments, plain, seen)
    }

    /// Descends from `start` through the child modules `segments` names, as
    /// far as the reach reads, then looks the next name up in the module it
    /// stopped in.
    fn walk(
        &self,
        start: &[String],
        segments: &[String],
        mut plain: bool,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        let mut module = start.to_vec();
        let mut at = 0;
        while self.reach.opens(&module) {
            let Some(name) = segments.get(at) else {
                return BTreeSet::from([self.names_module(module)]);
            };
            if self.reach.descends(&module) {
                let child: Vec<String> = module.iter().cloned().chain([name.clone()]).collect();
                if self.scan.tree.contains_key(&child) {
                    module = child;
                    at += 1;
                    plain = false;
                    continue;
                }
            }
            return self.through(&module, name, &segments[at + 1..], plain, seen);
        }
        BTreeSet::from([Resolved::Module(module)])
    }

    /// A path that ends at `module` itself.
    fn names_module(&self, module: Vec<String>) -> Resolved {
        match module.as_slice() {
            [] => Resolved::Unattributed("it names the crate root itself".to_owned()),
            [top] if self.reach.transparent.contains(top) => {
                Resolved::Unattributed(format!("it names the transparent module `{top}` itself"))
            }
            _ => Resolved::Module(module),
        }
    }

    /// `target`, a `use` of `module`, followed by `tail`. A plain path is
    /// relative to `module`: in the root a top-level item, a name another
    /// `use` binds or an external crate; in a relay module, which holds no
    /// items of its own, a name another `use` of the relay binds or another
    /// crate.
    fn follow(
        &self,
        module: &[String],
        target: &Target,
        tail: Vec<String>,
        cycle: Cycle,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        match target {
            Target::Absolute(path) => self.enter(
                &[],
                path.iter().cloned().chain(tail).collect(),
                false,
                cycle,
                seen,
            ),
            Target::Plain(path) => self.enter(
                module,
                path.iter().cloned().chain(tail).collect(),
                true,
                cycle,
                seen,
            ),
            Target::External => BTreeSet::from([Resolved::External]),
        }
    }

    /// `name` looked up in `module`, followed by `rest`.
    fn through(
        &self,
        module: &[String],
        name: &str,
        rest: &[String],
        plain: bool,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        let empty = UseTable::default();
        let table = self.scan.uses.get(module).unwrap_or(&empty);
        if let Some(all) = table.names.get(name) {
            // `use name;` beside a `macro_rules! name` re-exports the macro
            // the module defines. It is no source to follow: the name keeps
            // its owner.
            let defined = self.defines(module, name);
            let (rebinds, targets): (Vec<&Target>, Vec<&Target>) = all.iter().partition(|target| {
                defined.is_some() && matches!(target, Target::Plain(path) if path == &[name])
            });
            let Some((first, alternatives)) = targets.split_first() else {
                return BTreeSet::from([defined.unwrap_or_else(|| {
                    Resolved::Unattributed(format!("re-export `{name}` has no source"))
                })]);
            };
            if alternatives.is_empty() && rebinds.is_empty() {
                return self.follow(module, first, rest.to_vec(), Cycle::Unattributed, seen);
            }
            // A sibling alternative must not inherit paths visited by another
            // branch. Retain errors alongside every known owning module.
            let mut owners = self.follow(
                module,
                first,
                rest.to_vec(),
                Cycle::Unattributed,
                &mut seen.clone(),
            );
            for target in alternatives {
                owners.extend(self.follow(
                    module,
                    target,
                    rest.to_vec(),
                    Cycle::Unattributed,
                    &mut seen.clone(),
                ));
            }
            owners.extend(defined.filter(|_| !rebinds.is_empty()));
            return owners;
        }
        if let Some(defined) = self.defines(module, name) {
            return BTreeSet::from([defined]);
        }
        if plain {
            // A plain first segment nothing here binds is another crate's name.
            return BTreeSet::from([Resolved::External]);
        }
        let tail: Vec<String> = std::iter::once(name.to_owned())
            .chain(rest.iter().cloned())
            .collect();
        match self.reach.depth {
            Depth::Top => self.top_globs(module, name, table, tail, seen),
            Depth::Full => self.full_globs(module, name, table, tail, seen),
        }
    }

    /// The owner of `name` when `module` defines it: an exported macro or an
    /// item of the root, an item of any other module the reach reads into.
    fn defines(&self, module: &[String], name: &str) -> Option<Resolved> {
        let defined = self
            .scan
            .defined
            .get(module)
            .is_some_and(|names| names.contains(name));
        if module.is_empty() {
            if let Some(owner) = self.scan.macro_exports.get(name) {
                return Some(if owner.is_empty() {
                    Resolved::Root(name.to_owned())
                } else {
                    Resolved::Module(self.reach.clip(owner))
                });
            }
            return defined.then(|| Resolved::Root(name.to_owned()));
        }
        (self.reach.depth == Depth::Full && defined).then(|| Resolved::Module(module.to_vec()))
    }

    /// The glob re-exports of the root or of a transparent module, which
    /// name one module at most: the one glob supplies `name`, or nothing
    /// does.
    fn top_globs(
        &self,
        module: &[String],
        name: &str,
        table: &UseTable,
        tail: Vec<String>,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        let internal: Vec<&Target> = table
            .globs
            .iter()
            .filter(|glob| match glob {
                Target::Absolute(_) => true,
                Target::Plain(_) => module.is_empty(),
                Target::External => false,
            })
            .collect();
        match internal.as_slice() {
            [glob] => return self.follow(module, glob, tail, Cycle::Unattributed, seen),
            [_, _, ..] => {
                return BTreeSet::from([Resolved::Unattributed(format!(
                    "more than one glob re-export in `{}` could supply `{name}`",
                    label(module)
                ))]);
            }
            [] => {}
        }
        if table
            .globs
            .iter()
            .any(|glob| !matches!(glob, Target::Absolute(_)))
        {
            return BTreeSet::from([Resolved::External]);
        }
        BTreeSet::from([Resolved::Unattributed(format!(
            "`{}` has no module, re-export or item `{name}`",
            label(module)
        ))])
    }

    /// The glob re-exports of any module: each is tried, and the ones that
    /// supply `name` own it (a glob of another crate supplies it as external).
    /// A module nothing supplies `name` to owns it when it holds items the
    /// scan cannot read.
    fn full_globs(
        &self,
        module: &[String],
        name: &str,
        table: &UseTable,
        tail: Vec<String>,
        seen: &mut Seen,
    ) -> BTreeSet<Resolved> {
        let mut owners = BTreeSet::new();
        for glob in &table.globs {
            let supplied = self.follow(
                module,
                glob,
                tail.clone(),
                Cycle::Missing,
                &mut seen.clone(),
            );
            owners.extend(
                supplied
                    .into_iter()
                    .filter(|resolved| !matches!(resolved, Resolved::Missing(_))),
            );
        }
        if !owners.is_empty() {
            return owners;
        }
        if !module.is_empty() && self.scan.opaque.contains(module) {
            return BTreeSet::from([Resolved::Module(module.to_vec())]);
        }
        BTreeSet::from([Resolved::Missing(format!(
            "`{}` has no module, re-export or item `{name}`",
            label(module)
        ))])
    }
}
