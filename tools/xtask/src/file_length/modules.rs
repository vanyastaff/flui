//! The production module tree of a set of crate roots, read with `syn`.
//!
//! The walk starts at each target root and follows `mod name;` the way rustc
//! resolves it. A module whose `cfg` implies `test` is not followed, and a
//! test-only item anywhere in a production file is recorded as a line range,
//! so a caller can ask "outside `#[cfg(test)]`" of any file it reaches.
//!
//! What the walk cannot see: modules declared inside a macro invocation
//! (`syn` does not expand macros), and `include!`d files. A test-only
//! statement, field or expression inside production code is not an item and
//! counts as production.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Attribute, ForeignItem, ImplItem, Item, ItemMod, Meta, TraitItem};

/// One production source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Module {
    /// The file, as reached from its root (lexically normalised).
    pub(crate) path: PathBuf,
    /// Physical lines.
    pub(crate) lines: usize,
    /// The 1-based, inclusive line ranges of test-only items, merged and sorted.
    pub(crate) test_only: Vec<(usize, usize)>,
}

impl Module {
    /// Physical lines minus the lines of test-only items.
    pub(crate) fn production_lines(&self) -> usize {
        let excluded: usize = self.test_only.iter().map(|(a, b)| b - a + 1).sum();
        self.lines.saturating_sub(excluded)
    }
}

/// Something the walk could not follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Problem {
    /// `mod name;` with no file where rustc would look.
    Unresolved {
        file: PathBuf,
        line: usize,
        name: String,
        tried: Vec<PathBuf>,
    },
    /// A file that could not be read or parsed.
    Unreadable { file: PathBuf, error: String },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved {
                file,
                line,
                name,
                tried,
            } => {
                let tried: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "{}:{line}: `mod {name};` resolves to no file (tried {})",
                    file.display(),
                    tried.join(", ")
                )
            }
            Self::Unreadable { file, error } => write!(f, "{}: {error}", file.display()),
        }
    }
}

/// The result of a walk.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    /// Every production file reached, by path.
    pub(crate) modules: BTreeMap<PathBuf, Module>,
    /// Files reached only as test modules: read for their module declarations, not counted.
    pub(crate) test_files: BTreeSet<PathBuf>,
    pub(crate) problems: Vec<Problem>,
}

/// Walks the module trees under `roots` (crate root files).
pub(crate) fn walk(roots: impl IntoIterator<Item = PathBuf>) -> Tree {
    let mut tree = Tree::default();
    // (file, whether it is a mod-rs file, whether it is only compiled for tests)
    let mut queue: Vec<(PathBuf, bool, bool)> = roots
        .into_iter()
        .map(|root| (normalise(&root), true, false))
        .collect();
    let mut seen: BTreeSet<(PathBuf, bool)> = BTreeSet::new();
    while let Some((path, mod_rs, test)) = queue.pop() {
        if !seen.insert((path.clone(), test)) {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if !test => {
                tree.problems.push(Problem::Unreadable {
                    file: path,
                    error: error.to_string(),
                });
                continue;
            }
            Err(_) => continue,
        };
        let outcome = read_file(&text, test);
        // spans are per-thread and grow with every parse; nothing holds one now
        proc_macro2::extra::invalidate_current_thread_spans();
        let parsed = match outcome {
            Ok(parsed) => parsed,
            // a test module that does not parse is the test build's problem
            Err(_) if test => continue,
            Err(error) => {
                tree.problems
                    .push(Problem::Unreadable { file: path, error });
                continue;
            }
        };
        for declaration in &parsed.declarations {
            let tried = candidates(&path, mod_rs, declaration);
            let child_test = parsed.test || declaration.test_only;
            match tried.iter().find(|candidate| candidate.is_file()) {
                Some(found) => {
                    let child_mod_rs = declaration.path_attr.is_some()
                        || found.file_name().is_some_and(|name| name == "mod.rs");
                    queue.push((found.clone(), child_mod_rs, child_test));
                }
                None if child_test => {}
                None => tree.problems.push(Problem::Unresolved {
                    file: path.clone(),
                    line: declaration.line,
                    name: declaration.name.clone(),
                    tried,
                }),
            }
        }
        if parsed.test {
            tree.test_files.insert(path);
        } else {
            tree.modules.insert(
                path.clone(),
                Module {
                    path,
                    lines: physical_lines(&text),
                    test_only: parsed.test_only,
                },
            );
        }
    }
    // a file reached both ways is production
    let production: BTreeSet<PathBuf> = tree.modules.keys().cloned().collect();
    tree.test_files.retain(|path| !production.contains(path));
    tree
}

/// Lines as an editor numbers them: a final line without a newline counts.
pub(crate) fn physical_lines(text: &str) -> usize {
    let newlines = text.bytes().filter(|&b| b == b'\n').count();
    newlines + usize::from(!text.is_empty() && !text.ends_with('\n'))
}

/// A `mod name;` found in a file.
#[derive(Debug)]
struct Declaration {
    name: String,
    /// `#[path = "..."]`, if any.
    path_attr: Option<String>,
    /// The inline modules around it, each as its directory (its name or its `#[path]`).
    inline: Vec<String>,
    line: usize,
    test_only: bool,
}

/// What one file contributes to the walk.
struct Parsed {
    /// Only compiled for tests: reached as a test module, or carrying an
    /// inner `#![cfg(test)]`. Its module declarations are test modules too.
    test: bool,
    test_only: Vec<(usize, usize)>,
    declarations: Vec<Declaration>,
}

fn read_file(text: &str, test: bool) -> Result<Parsed, String> {
    let file = syn::parse_file(text).map_err(|error| {
        let start = error.span().start();
        format!("{}:{}: {error}", start.line, start.column + 1)
    })?;
    let test = test || is_test_only(&file.attrs);
    let mut collector = Collector {
        in_test: test,
        ..Collector::default()
    };
    for item in &file.items {
        collector.visit_item(item);
    }
    Ok(Parsed {
        test,
        test_only: merged(collector.ranges),
        declarations: collector.declarations,
    })
}

/// Where rustc looks for `declaration`, declared in `file`.
fn candidates(file: &Path, mod_rs: bool, declaration: &Declaration) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let children = if mod_rs {
        dir.to_path_buf()
    } else {
        let stem = file.file_stem().unwrap_or_default();
        dir.join(stem)
    };
    let inline: PathBuf = declaration.inline.iter().collect();
    let found = match &declaration.path_attr {
        // outside inline blocks, relative to the declaring file's directory
        Some(path) if declaration.inline.is_empty() => vec![dir.join(path)],
        Some(path) => vec![children.join(inline).join(path)],
        None => {
            let base = children.join(inline);
            vec![
                base.join(format!("{}.rs", declaration.name)),
                base.join(&declaration.name).join("mod.rs"),
            ]
        }
    };
    found.iter().map(|path| normalise(path)).collect()
}

/// `path` with `.` and `name/..` pairs removed, without touching the file system.
fn normalise(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// Test-only item ranges and module declarations of one file.
#[derive(Default)]
struct Collector {
    ranges: Vec<(usize, usize)>,
    declarations: Vec<Declaration>,
    inline: Vec<String>,
    /// Inside a test-only item: already excluded whole; module declarations
    /// found here are test modules.
    in_test: bool,
}

impl Collector {
    fn exclude(&mut self, node: &impl syn::spanned::Spanned) {
        if self.in_test {
            return;
        }
        let span = node.span();
        self.ranges.push((span.start().line, span.end().line));
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, item: &'ast Item) {
        if !self.in_test && is_test_only(item_attrs(item)) {
            self.exclude(item);
            // only to learn which files are test modules
            self.in_test = true;
            visit::visit_item(self, item);
            self.in_test = false;
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        match &module.content {
            Some((_, items)) => {
                self.inline
                    .push(path_attr(&module.attrs).unwrap_or_else(|| unraw(&module.ident)));
                for item in items {
                    self.visit_item(item);
                }
                self.inline.pop();
            }
            None => self
                .declarations
                .push(declaration(module, &self.inline, self.in_test)),
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs: &[Attribute] = match item {
            ImplItem::Const(i) => &i.attrs,
            ImplItem::Fn(i) => &i.attrs,
            ImplItem::Type(i) => &i.attrs,
            ImplItem::Macro(i) => &i.attrs,
            _ => &[],
        };
        if is_test_only(attrs) {
            self.exclude(item);
        } else {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let attrs: &[Attribute] = match item {
            TraitItem::Const(i) => &i.attrs,
            TraitItem::Fn(i) => &i.attrs,
            TraitItem::Type(i) => &i.attrs,
            TraitItem::Macro(i) => &i.attrs,
            _ => &[],
        };
        if is_test_only(attrs) {
            self.exclude(item);
        } else {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        let attrs: &[Attribute] = match item {
            ForeignItem::Fn(i) => &i.attrs,
            ForeignItem::Static(i) => &i.attrs,
            ForeignItem::Type(i) => &i.attrs,
            ForeignItem::Macro(i) => &i.attrs,
            _ => &[],
        };
        if is_test_only(attrs) {
            self.exclude(item);
        } else {
            visit::visit_foreign_item(self, item);
        }
    }
}

fn declaration(module: &ItemMod, inline: &[String], test_only: bool) -> Declaration {
    Declaration {
        name: unraw(&module.ident),
        path_attr: path_attr(&module.attrs),
        inline: inline.to_vec(),
        line: module.mod_token.span.start().line,
        test_only,
    }
}

fn unraw(ident: &syn::Ident) -> String {
    let name = ident.to_string();
    name.strip_prefix("r#")
        .map_or_else(|| name.clone(), str::to_owned)
}

/// The value of a `#[path = "..."]` attribute.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// Whether one of `attrs` is a `cfg` whose predicate implies `test`.
pub(crate) fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|predicate| implies_test(&predicate))
    })
}

/// Whether the `cfg` predicate can hold only under `cfg(test)`: `test`; `all`
/// with an operand that implies it; `any` whose operands all do. `not(_)` and
/// every other predicate do not.
pub(crate) fn implies_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(operands) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                operands.iter().any(implies_test)
            } else if list.path.is_ident("any") {
                !operands.is_empty() && operands.iter().all(implies_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// Sorted, overlapping or adjacent ranges joined.
fn merged(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match out.last_mut() {
            Some(last) if start <= last.1 + 1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}
