//! `cargo xtask public-api`: the facade's public API, one line per thing a
//! user can name, against its committed snapshot `api/flui.txt` (ADR-0323).
//!
//! **What counts.** rustdoc's JSON output for `flui` with every feature on
//! the host, as `send-positions` loads it (the same builds, so the two share
//! their cost), and for every workspace crate a public re-export reaches. The
//! walk starts at the crate root and visits, in order of path length, every
//! public item of every public module, through re-exports and glob
//! re-exports into the crates they name. An item gets the first path the walk
//! finds (the shortest, then the first in order); every other path to it is a
//! `reexport` line naming that one. A `#[doc(hidden)]` item, and everything
//! under one, is not part of the API.
//!
//! **What a line says.** A module, a type with its public fields or variants,
//! a function with its signature, a constant, a static, a macro; under a type,
//! its public inherent methods and constants and every implementation an
//! author wrote (`impl Clone for …`), with the associated types it sets; under
//! a trait, its items and its implementations for types the trait's crate
//! does not reach otherwise (a blanket `impl<T: Send> Trait for T`). A path in
//! a signature is written as the facade path of the item it names; an item
//! the facade does not reach keeps the path rustdoc records for it, so a
//! signature that names an engine type shows as `flui_engine::…`. A
//! synthetic implementation (`Send`, `Unpin`), and a blanket implementation's
//! copy on each type, are not written.
//!
//! **The check.** The rendered text equals the snapshot, or the command lists
//! the lines that differ and fails. `--write` restates the snapshot, and the
//! pull request that changes the surface carries its diff.
//!
//! **Limits.** The host's build only: an item another target builds is not
//! seen. The rendering reads the JSON untyped; a shape it does not know is
//! written as `?`.
//!
//! `--self-test` renders the built-in document `send-positions` checks, and
//! fails unless the text is the expected one and a planted stale snapshot
//! reports exactly its two planted lines (ADR-0078 §4).

mod render;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::Value as Json;

use crate::api_closure::{self, Build, Items};
use crate::send_positions;
use crate::util;

use render::Names;

/// The snapshot, repository-relative.
const SNAPSHOT: &str = "api/flui.txt";

/// The package whose public API the snapshot records.
const ROOT: &str = "flui";

/// Arguments for `cargo xtask public-api`.
#[derive(Debug, clap::Args)]
pub(crate) struct PublicApiArgs {
    /// The toolchain that writes rustdoc's JSON output (`rustup run <toolchain>`).
    #[arg(long, default_value = "nightly")]
    toolchain: String,
    /// Write the snapshot instead of checking it.
    #[arg(long, conflicts_with = "self_test")]
    write: bool,
    /// Render a built-in rustdoc JSON document instead of the workspace; exit
    /// 1 unless the text and the planted differences are the expected ones.
    #[arg(long)]
    self_test: bool,
}

/// `cargo xtask public-api`. Exit code 1 lists every line that differs.
pub(crate) fn public_api(args: &PublicApiArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let libraries = send_positions::libraries(&metadata);
    let lib = libraries
        .iter()
        .find(|(_, name)| name.as_str() == ROOT)
        .map(|(lib, _)| lib.clone())
        .with_context(|| format!("`{ROOT}` is not a workspace library"))?;
    let mut load = |lib: &str| -> anyhow::Result<Option<Json>> {
        let Some(package) = libraries.get(lib) else {
            return Ok(None);
        };
        let build = Build {
            all_features: true,
            ..Build::HOST
        };
        api_closure::rustdoc_json(&root, &args.toolchain, package, lib, &build, Items::Private)
            .map(Some)
    };
    let mut surface = Surface::default();
    surface.walk(&lib, &mut load)?;
    let text = surface.render();
    let path = root.join(SNAPSHOT);
    if args.write {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&path, &text).with_context(|| format!("writing {}", path.display()))?;
        println!(
            "public-api: wrote {SNAPSHOT} ({} lines)",
            text.lines().count()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let committed = match std::fs::read_to_string(&path) {
        Ok(committed) => committed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let (removed, added) = differences(&committed, &text);
    if removed.is_empty() && added.is_empty() {
        println!(
            "public-api: {SNAPSHOT} is the API of `{ROOT}` ({} lines)",
            text.lines().count()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "public-api: the API of `{ROOT}` is not {SNAPSHOT}: {} line(s) gone, {} new",
        removed.len(),
        added.len()
    );
    for line in &removed {
        eprintln!("- {line}");
    }
    for line in &added {
        eprintln!("+ {line}");
    }
    eprintln!(
        "public-api: if the change is meant, run `cargo xtask public-api --write` and commit \
         {SNAPSHOT}"
    );
    Ok(ExitCode::FAILURE)
}

/// The lines of `old` that `new` lacks, and the lines of `new` that `old`
/// lacks, each in its own order.
fn differences(old: &str, new: &str) -> (Vec<String>, Vec<String>) {
    let old_lines: BTreeSet<&str> = old.lines().collect();
    let new_lines: BTreeSet<&str> = new.lines().collect();
    let removed = old
        .lines()
        .filter(|line| !new_lines.contains(line))
        .map(str::to_owned)
        .collect();
    let added = new
        .lines()
        .filter(|line| !old_lines.contains(line))
        .map(str::to_owned)
        .collect();
    (removed, added)
}

/// An item: its crate's name and its rustdoc id in that crate's document.
type Key = (String, String);

/// Where a name in a module leads.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    /// An item of a workspace crate.
    Item(Key),
    /// An item outside the workspace (or one rustdoc could not resolve), by
    /// the path it records.
    Foreign(String),
}

/// One crate's document loader: `None` for a crate outside the workspace.
type Load<'a> = dyn FnMut(&str) -> anyhow::Result<Option<Json>> + 'a;

/// Everything the walk reached.
#[derive(Debug, Default)]
struct Surface {
    /// Each loaded crate's document; `None` for a crate outside the workspace.
    docs: BTreeMap<String, Option<Json>>,
    /// Each loaded crate's own items by the path rustdoc records for them.
    recorded: BTreeMap<String, BTreeMap<Vec<String>, String>>,
    /// Each reached item's facade path.
    canonical: BTreeMap<Key, Vec<String>>,
    /// Every other path to a reached item, or to a foreign one.
    reexports: Vec<(Vec<String>, Target)>,
}

impl Surface {
    /// Loads `lib`'s document once; `false` when it is not a workspace crate.
    fn ensure(&mut self, lib: &str, load: &mut Load<'_>) -> anyhow::Result<bool> {
        if !self.docs.contains_key(lib) {
            let doc = load(lib)?;
            if let Some(doc) = &doc {
                let mut recorded = BTreeMap::new();
                for (id, summary) in doc["paths"].as_object().into_iter().flatten() {
                    if summary["crate_id"].as_u64() != Some(0) {
                        continue;
                    }
                    if let Some(path) = segments(&summary["path"]) {
                        recorded.insert(path, id.clone());
                    }
                }
                self.recorded.insert(lib.to_owned(), recorded);
            }
            self.docs.insert(lib.to_owned(), doc);
        }
        Ok(self.docs[lib].is_some())
    }

    fn doc(&self, lib: &str) -> &Json {
        self.docs[lib]
            .as_ref()
            .expect("BUG: only a loaded workspace crate's items are reached")
    }

    fn item(&self, key: &Key) -> &Json {
        &self.doc(&key.0)["index"][&key.1]
    }

    /// Visits every public path from the root of `lib`, shortest first.
    fn walk(&mut self, lib: &str, load: &mut Load<'_>) -> anyhow::Result<()> {
        if !self.ensure(lib, load)? {
            bail!("`{lib}` is not a workspace library");
        }
        let root = api_closure::id_text(&self.doc(lib)["root"]);
        let mut queue: BTreeSet<(usize, Vec<String>, Key)> = BTreeSet::new();
        queue.insert((1, vec![lib.to_owned()], (lib.to_owned(), root)));
        while let Some((depth, path, key)) = queue.pop_first() {
            if self.canonical.contains_key(&key) {
                self.reexports.push((path, Target::Item(key)));
                continue;
            }
            self.canonical.insert(key.clone(), path.clone());
            if self.item(&key)["inner"].get("module").is_none() {
                continue;
            }
            let mut globs = BTreeSet::new();
            for (name, target) in self.children(&key, load, &mut globs)? {
                let mut child = path.clone();
                child.push(name);
                match target {
                    Target::Item(key) => {
                        queue.insert((depth + 1, child, key));
                    }
                    foreign @ Target::Foreign(_) => self.reexports.push((child, foreign)),
                }
            }
        }
        Ok(())
    }

    /// The names a module gives, with where each leads: its own public items
    /// and re-exports first, then what its glob re-exports bring that those
    /// do not shadow. A macro's name ends in `!`, its own namespace.
    fn children(
        &mut self,
        module: &Key,
        load: &mut Load<'_>,
        globs: &mut BTreeSet<Key>,
    ) -> anyhow::Result<BTreeMap<String, Target>> {
        globs.insert(module.clone());
        let (lib, _) = module;
        let items: Vec<String> =
            send_positions::ids(&self.item(module)["inner"]["module"]["items"]).collect();
        let mut names = BTreeMap::new();
        let mut glob_targets = Vec::new();
        for id in items {
            let item = self.doc(lib)["index"][&id].clone();
            if !send_positions::public(&item) || hidden(&item) {
                continue;
            }
            if let Some(re_export) = item["inner"].get("use") {
                let target = self.target(lib, re_export, load)?;
                if re_export["is_glob"].as_bool() == Some(true) {
                    if let Target::Item(key) = target
                        && self.item(&key)["inner"].get("module").is_some()
                    {
                        glob_targets.push(key);
                    }
                    continue;
                }
                let Some(name) = re_export["name"].as_str() else {
                    continue;
                };
                let macro_name = matches!(&target, Target::Item(key) if is_macro(self.item(key)));
                names.entry(spelled(name, macro_name)).or_insert(target);
                continue;
            }
            let Some(name) = item["name"].as_str() else {
                continue;
            };
            if item["inner"].get("impl").is_some() {
                continue;
            }
            names.insert(
                spelled(name, is_macro(&item)),
                Target::Item((lib.clone(), id)),
            );
        }
        for key in glob_targets {
            if globs.contains(&key) {
                continue;
            }
            for (name, target) in self.children(&key, load, globs)? {
                names.entry(name).or_insert(target);
            }
        }
        Ok(names)
    }

    /// Where a re-export in `lib` leads.
    fn target(
        &mut self,
        lib: &str,
        re_export: &Json,
        load: &mut Load<'_>,
    ) -> anyhow::Result<Target> {
        let source = re_export["source"].as_str().unwrap_or("?").to_owned();
        let id = &re_export["id"];
        if id.is_null() {
            return Ok(Target::Foreign(source));
        }
        let id = api_closure::id_text(id);
        let doc = self.doc(lib);
        if doc["index"].get(&id).is_some() {
            return Ok(Target::Item((lib.to_owned(), id)));
        }
        let Some((other, path)) = send_positions::external(doc, &id) else {
            return Ok(Target::Foreign(source));
        };
        if !self.ensure(&other, load)? {
            return Ok(Target::Foreign(path.join("::")));
        }
        Ok(match send_positions::resolve(self.doc(&other), &path) {
            Some(id) => Target::Item((other, id)),
            None => Target::Foreign(path.join("::")),
        })
    }

    /// The snapshot's text.
    fn render(&self) -> String {
        let mut blocks: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut written: BTreeSet<Key> = BTreeSet::new();
        for (key, path) in &self.canonical {
            let lines = self.render_item(key, &path.join("::"), &mut written);
            blocks.entry(path.join("::")).or_default().extend(lines);
        }
        // A trait's implementations for a generic or foreign type: no reached
        // type lists them.
        for (key, path) in &self.canonical {
            let item = self.item(key);
            let Some(definition) = item["inner"].get("trait") else {
                continue;
            };
            let names = Namer {
                surface: self,
                lib: &key.0,
            };
            let index = &self.doc(&key.0)["index"];
            let mut lines = Vec::new();
            for id in send_positions::ids(&definition["implementations"]) {
                let implementation_key = (key.0.clone(), id.clone());
                let Some(implementation) = index.get(&id) else {
                    continue;
                };
                if api_closure::skipped(implementation) || written.contains(&implementation_key) {
                    continue;
                }
                let for_type = &implementation["inner"]["impl"]["for"];
                let local = for_type["resolved_path"]
                    .get("id")
                    .is_some_and(|id| index.get(api_closure::id_text(id)).is_some());
                if local {
                    continue;
                }
                lines.push(self.render_impl(&names, implementation));
            }
            lines.sort();
            blocks.entry(path.join("::")).or_default().extend(lines);
        }
        for (path, target) in &self.reexports {
            let target = match target {
                Target::Item(key) => self.canonical[key].join("::"),
                Target::Foreign(path) => path.clone(),
            };
            blocks
                .entry(path.join("::"))
                .or_default()
                .push(format!("reexport {} => {target}", path.join("::")));
        }
        let mut text = format!(
            "# The public API of `{ROOT}`, every feature on, as the Linux host builds it.\n\
             # Written by `cargo xtask public-api --write` (ADR-0323); a pull request that\n\
             # changes the API carries the diff of this file.\n"
        );
        for line in blocks.into_values().flatten() {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    /// The lines of one reached item: its declaration first, then its
    /// members in order.
    fn render_item(&self, key: &Key, path: &str, written: &mut BTreeSet<Key>) -> Vec<String> {
        let item = self.item(key);
        let names = Namer {
            surface: self,
            lib: &key.0,
        };
        let index = &self.doc(&key.0)["index"];
        let Some((kind, inner)) = item["inner"].as_object().and_then(|map| map.iter().next())
        else {
            return Vec::new();
        };
        let attributes = attributes(item);
        let mut members = Vec::new();
        let declaration = match kind.as_str() {
            "module" => format!("pub mod {path}"),
            "struct" | "union" => {
                let generics = &inner["generics"];
                let head = format!(
                    "{attributes}pub {kind} {path}{}",
                    render::params(&names, generics)
                );
                let where_clause = render::where_clause(&names, generics);
                let shape = &inner["kind"];
                let declaration = if let Some(fields) = shape.get("tuple") {
                    let fields: Vec<String> = fields
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|id| match index.get(api_closure::id_text(id)) {
                            Some(field) if send_positions::public(field) && !hidden(field) => {
                                render::ty(&names, &field["inner"]["struct_field"])
                            }
                            _ => "_".to_owned(),
                        })
                        .collect();
                    format!("{head}({}){where_clause}", fields.join(", "))
                } else if let Some(plain) = shape
                    .get("plain")
                    .or_else(|| inner.get("fields").map(|_| inner))
                {
                    let mut private = plain["has_stripped_fields"].as_bool() == Some(true);
                    for id in send_positions::ids(&plain["fields"]) {
                        let field = &index[&id];
                        if !send_positions::public(field) || hidden(field) {
                            private = true;
                            continue;
                        }
                        members.push(format!(
                            "field {path}::{}: {}",
                            field["name"].as_str().unwrap_or("?"),
                            render::ty(&names, &field["inner"]["struct_field"])
                        ));
                    }
                    format!(
                        "{head}{where_clause}{}",
                        if private { " { .. }" } else { "" }
                    )
                } else {
                    format!("{head}{where_clause}")
                };
                members.extend(self.render_impls(key, path, &inner["impls"], written));
                declaration
            }
            "enum" => {
                let generics = &inner["generics"];
                for id in send_positions::ids(&inner["variants"]) {
                    let variant = &index[&id];
                    if hidden(variant) {
                        continue;
                    }
                    members.push(variant_line(&names, index, path, variant));
                }
                members.extend(self.render_impls(key, path, &inner["impls"], written));
                format!(
                    "{attributes}pub enum {path}{}{}",
                    render::params(&names, generics),
                    render::where_clause(&names, generics)
                )
            }
            "trait" => {
                for id in send_positions::ids(&inner["items"]) {
                    let member = &index[&id];
                    if hidden(member) {
                        continue;
                    }
                    if let Some(line) = associated(&names, path, member, true) {
                        members.push(line);
                    }
                }
                let generics = &inner["generics"];
                let supertraits = render::bounds(&names, &inner["bounds"]);
                format!(
                    "{attributes}pub {}{}trait {path}{}{}{}",
                    if inner["is_unsafe"].as_bool() == Some(true) {
                        "unsafe "
                    } else {
                        ""
                    },
                    if inner["is_auto"].as_bool() == Some(true) {
                        "auto "
                    } else {
                        ""
                    },
                    render::params(&names, generics),
                    if supertraits.is_empty() {
                        String::new()
                    } else {
                        format!(": {supertraits}")
                    },
                    render::where_clause(&names, generics)
                )
            }
            "function" => format!("{attributes}pub {}", render::function(&names, path, inner)),
            "type_alias" => {
                let generics = &inner["generics"];
                format!(
                    "{attributes}pub type {path}{}{} = {}",
                    render::params(&names, generics),
                    render::where_clause(&names, generics),
                    render::ty(&names, &inner["type"])
                )
            }
            "constant" => format!(
                "{attributes}pub const {path}: {}{}",
                render::ty(&names, &inner["type"]),
                literal(&inner["const"])
            ),
            "static" => format!(
                "{attributes}pub {}static {path}: {}",
                if inner["is_mutable"].as_bool() == Some(true) {
                    "mut "
                } else {
                    ""
                },
                render::ty(&names, &inner["type"])
            ),
            "macro" => format!("{attributes}macro {path}"),
            "proc_macro" => format!(
                "{attributes}proc-macro {} {path}",
                inner["kind"].as_str().unwrap_or("?")
            ),
            "trait_alias" => format!(
                "{attributes}pub trait {path}{} = {}",
                render::params(&names, &inner["generics"]),
                render::bounds(&names, &inner["params"])
            ),
            "extern_type" => format!("{attributes}pub extern type {path}"),
            "extern_crate" => format!("pub extern crate {path}"),
            _ => return Vec::new(),
        };
        members.sort();
        let mut lines = vec![declaration];
        lines.extend(members);
        lines
    }

    /// A type's implementations: its public inherent items, and each trait
    /// implementation an author wrote, as one line.
    fn render_impls(
        &self,
        key: &Key,
        path: &str,
        impls: &Json,
        written: &mut BTreeSet<Key>,
    ) -> Vec<String> {
        let names = Namer {
            surface: self,
            lib: &key.0,
        };
        let index = &self.doc(&key.0)["index"];
        let mut lines = Vec::new();
        for id in send_positions::ids(impls) {
            let Some(implementation) = index.get(&id) else {
                continue;
            };
            if api_closure::skipped(implementation) {
                continue;
            }
            let data = &implementation["inner"]["impl"];
            if data["trait"].is_null() {
                let generics = &data["generics"];
                let context = format!(
                    "{}{}",
                    render::params(&names, generics),
                    render::where_clause(&names, generics)
                );
                for member in send_positions::ids(&data["items"]) {
                    let member = &index[&member];
                    if !send_positions::public(member) || hidden(member) {
                        continue;
                    }
                    if let Some(mut line) = associated(&names, path, member, false) {
                        if !context.is_empty() {
                            write!(line, " // impl{context}").expect("BUG: writing to a String");
                        }
                        lines.push(line);
                    }
                }
                continue;
            }
            if private_trait(index, data) {
                continue;
            }
            written.insert((key.0.clone(), id));
            lines.push(self.render_impl(&names, implementation));
        }
        lines
    }

    /// `impl<G> Trait for Type where … { type A = B; }`.
    fn render_impl(&self, names: &Namer<'_>, implementation: &Json) -> String {
        let data = &implementation["inner"]["impl"];
        let index = &self.doc(names.lib)["index"];
        let generics = &data["generics"];
        let mut associated_types: Vec<String> = send_positions::ids(&data["items"])
            .filter_map(|id| {
                let member = &index[&id];
                let value = member["inner"].get("assoc_type")?;
                Some(format!(
                    "type {} = {};",
                    member["name"].as_str().unwrap_or("?"),
                    render::ty(names, &value["type"])
                ))
            })
            .collect();
        associated_types.sort();
        format!(
            "{}impl{} {}{} for {}{}{}",
            if data["is_unsafe"].as_bool() == Some(true) {
                "unsafe "
            } else {
                ""
            },
            render::params(names, generics),
            if data["is_negative"].as_bool() == Some(true) {
                "!"
            } else {
                ""
            },
            render::path(names, &data["trait"]),
            render::ty(names, &data["for"]),
            render::where_clause(names, generics),
            if associated_types.is_empty() {
                String::new()
            } else {
                format!(" {{ {} }}", associated_types.join(" "))
            }
        )
    }
}

/// Writes paths for the signatures of one crate's items.
struct Namer<'a> {
    surface: &'a Surface,
    lib: &'a str,
}

impl Names for Namer<'_> {
    fn name(&self, id: &str) -> Option<String> {
        let surface = self.surface;
        if let Some(path) = surface.canonical.get(&(self.lib.to_owned(), id.to_owned())) {
            return Some(path.join("::"));
        }
        let doc = surface.doc(self.lib);
        if let Some((other, path)) = send_positions::external(doc, id) {
            let facade = surface
                .recorded
                .get(&other)
                .and_then(|recorded| recorded.get(&path))
                .and_then(|id| surface.canonical.get(&(other.clone(), id.clone())));
            return Some(match facade {
                Some(facade) => facade.join("::"),
                None => render::recorded(&other, &path),
            });
        }
        let summary = doc["paths"].get(id)?;
        let path = segments(&summary["path"])?;
        Some(render::recorded(self.lib, &path))
    }
}

/// A list of path segments.
fn segments(value: &Json) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|segment| segment.as_str().map(str::to_owned))
        .collect()
}

fn is_macro(item: &Json) -> bool {
    item["inner"].get("macro").is_some()
        || item["inner"]["proc_macro"]["kind"].as_str() == Some("bang")
}

fn spelled(name: &str, macro_name: bool) -> String {
    if macro_name {
        format!("{name}!")
    } else {
        name.to_owned()
    }
}

/// `#[doc(hidden)]`, in any of the forms rustdoc's attribute list writes it.
fn hidden(item: &Json) -> bool {
    item["attrs"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|attribute| {
            let text = match attribute {
                Json::String(text) => text.clone(),
                other => other.to_string(),
            };
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            compact.contains("doc(hidden)") || compact.contains("\"hidden\"")
        })
}

/// The attributes a line states: `#[deprecated]` and `#[non_exhaustive]`,
/// the ones that change what a user may write.
fn attributes(item: &Json) -> String {
    let mut text = String::new();
    if !item["deprecation"].is_null() {
        text.push_str("#[deprecated] ");
    }
    let non_exhaustive = item["attrs"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|attribute| {
            attribute.as_str() == Some("non_exhaustive")
                || attribute.get("non_exhaustive").is_some()
                || attribute.to_string().contains("#[non_exhaustive]")
        });
    if non_exhaustive {
        text.push_str("#[non_exhaustive] ");
    }
    text
}

/// ` = 3` for a literal constant; nothing for a computed one, whose text is
/// an implementation detail.
fn literal(constant: &Json) -> String {
    if constant["is_literal"].as_bool() == Some(true)
        && let Some(expr) = constant["expr"].as_str()
    {
        format!(" = {expr}")
    } else {
        String::new()
    }
}

/// One variant of the enum at `path`.
fn variant_line(names: &Namer<'_>, index: &Json, path: &str, variant: &Json) -> String {
    let name = variant["name"].as_str().unwrap_or("?");
    let data = &variant["inner"]["variant"];
    let kind = &data["kind"];
    let field_type = |id: &str| -> Option<String> {
        let field = index.get(id)?;
        if hidden(field) {
            return None;
        }
        Some(render::ty(names, &field["inner"]["struct_field"]))
    };
    let shape = if let Some(fields) = kind.get("tuple") {
        let fields: Vec<String> = fields
            .as_array()
            .into_iter()
            .flatten()
            .map(|id| {
                if id.is_null() {
                    "_".to_owned()
                } else {
                    field_type(&api_closure::id_text(id)).unwrap_or_else(|| "_".to_owned())
                }
            })
            .collect();
        format!("({})", fields.join(", "))
    } else if let Some(record) = kind.get("struct") {
        let mut fields: Vec<String> = send_positions::ids(&record["fields"])
            .filter_map(|id| {
                let field_name = index[&id]["name"].as_str()?.to_owned();
                Some(format!("{field_name}: {}", field_type(&id)?))
            })
            .collect();
        if record["has_stripped_fields"].as_bool() == Some(true) {
            fields.push("..".to_owned());
        }
        format!(" {{ {} }}", fields.join(", "))
    } else {
        String::new()
    };
    let discriminant = data["discriminant"]["expr"]
        .as_str()
        .map_or_else(String::new, |expr| format!(" = {expr}"));
    format!(
        "{}variant {path}::{name}{shape}{discriminant}",
        attributes(variant)
    )
}

/// A trait's or an inherent implementation's item, by `owner::name`.
/// `in_trait` writes a provided method's default as `{ .. }`.
fn associated(names: &Namer<'_>, owner: &str, member: &Json, in_trait: bool) -> Option<String> {
    let name = member["name"].as_str()?;
    let path = format!("{owner}::{name}");
    let attributes = attributes(member);
    let (kind, inner) = member["inner"].as_object()?.iter().next()?;
    let visibility = if in_trait { "" } else { "pub " };
    Some(match kind.as_str() {
        "function" => {
            let provided = in_trait && inner["has_body"].as_bool() == Some(true);
            format!(
                "{attributes}{visibility}{}{}",
                render::function(names, &path, inner),
                if provided { " { .. }" } else { "" }
            )
        }
        "assoc_const" => {
            let value = inner["value"]
                .as_str()
                .filter(|_| !in_trait)
                .map_or_else(String::new, |value| format!(" = {value}"));
            format!(
                "{attributes}{visibility}const {path}: {}{value}",
                render::ty(names, &inner["type"])
            )
        }
        "assoc_type" => {
            let bounds = render::bounds(names, &inner["bounds"]);
            let default = if inner["type"].is_null() {
                String::new()
            } else {
                format!(" = {}", render::ty(names, &inner["type"]))
            };
            format!(
                "{attributes}type {path}{}{}{default}",
                render::params(names, &inner["generics"]),
                if bounds.is_empty() {
                    String::new()
                } else {
                    format!(": {bounds}")
                }
            )
        }
        _ => return None,
    })
}

/// An implementation of a trait its crate keeps private (a sealing trait):
/// no user can name it.
fn private_trait(index: &Json, implementation: &Json) -> bool {
    implementation["trait"]
        .get("id")
        .and_then(|id| index.get(api_closure::id_text(id)))
        .is_some_and(|definition| !send_positions::public(definition))
}

/// What the self-test must render for `send-positions`' fixture.
const EXPECTED: &str = "\
# The public API of `flui`, every feature on, as the Linux host builds it.
# Written by `cargo xtask public-api --write` (ADR-0323); a pull request that
# changes the API carries the diff of this file.
pub mod fixture
pub trait fixture::Bounded: Send
pub type fixture::Callback = dyn Send
pub struct fixture::S { .. }
field fixture::S::payload: dyn Send
impl Sync for fixture::S
pub trait fixture::Sealed: fixture::sealed::Base
impl<T: Send> fixture::Sealed for T
pub fn fixture::hand_over() where T: Send
pub fn fixture::run(job: dyn Send)
pub fn fixture::spawn<T: Sync>()
pub fn fixture::worker() -> impl Send
";

/// Renders the fixture: the text, and what a planted stale snapshot reports.
fn self_test_render() -> anyhow::Result<(String, Vec<String>, Vec<String>)> {
    let mut load = |lib: &str| -> anyhow::Result<Option<Json>> {
        Ok((lib == "fixture").then(send_positions::fixture))
    };
    let mut surface = Surface::default();
    surface.walk("fixture", &mut load)?;
    let text = surface.render();
    let stale = EXPECTED
        .replace("pub fn fixture::run(job: dyn Send)\n", "")
        .replace(
            "pub fn fixture::spawn<T: Sync>()",
            "pub fn fixture::spawn<T: Send>()",
        );
    let (removed, added) = differences(&stale, &text);
    Ok((text, removed, added))
}

/// `cargo xtask public-api --self-test`.
fn self_test() -> ExitCode {
    let (text, removed, added) = match self_test_render() {
        Ok(result) => result,
        Err(error) => {
            println!("self-test: the fixture does not render: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    let mut ok = true;
    if text != EXPECTED {
        ok = false;
        let (missing, extra) = differences(EXPECTED, &text);
        for line in missing {
            println!("self-test: MISSED {line}");
        }
        for line in extra {
            println!("self-test: UNEXPECTED {line}");
        }
    }
    let planted_removed = vec!["pub fn fixture::spawn<T: Send>()".to_owned()];
    let planted_added = vec![
        "pub fn fixture::run(job: dyn Send)".to_owned(),
        "pub fn fixture::spawn<T: Sync>()".to_owned(),
    ];
    if removed != planted_removed || added != planted_added {
        ok = false;
        println!("self-test: the stale snapshot reports -{removed:?} +{added:?}");
    }
    if !ok {
        return ExitCode::FAILURE;
    }
    println!("public-api: self-test ok (the fixture renders as expected, 3 planted lines differ)");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_api_fixture_renders_as_expected_and_reports_the_planted_lines() {
        let (text, removed, added) = self_test_render().expect("the fixture renders");
        assert_eq!(text, EXPECTED);
        assert_eq!(removed, vec!["pub fn fixture::spawn<T: Send>()".to_owned()]);
        assert_eq!(
            added,
            vec![
                "pub fn fixture::run(job: dyn Send)".to_owned(),
                "pub fn fixture::spawn<T: Sync>()".to_owned(),
            ]
        );
    }
}
