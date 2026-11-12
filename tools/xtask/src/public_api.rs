//! `cargo xtask public-api`: the facade's public API, one line per thing a
//! user can name, against its committed snapshot `api/flui.txt` (ADR-0324).
//!
//! **What counts.** rustdoc's JSON output for `flui` with every feature on
//! the host, as `send-positions` loads it (the same builds, so one run of both
//! builds once), and for every workspace crate a public re-export reaches. The
//! walk starts at the crate root and visits, in order of path length, every
//! public item of every public module, through re-exports and glob
//! re-exports into the crates they name; then every public workspace type a
//! reached signature names that no path reaches (a type behind a type alias,
//! a returned engine type), at the path rustdoc records. An item gets the first path the walk
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

    /// Visits every public path from the root of `lib`, shortest first; then
    /// every public workspace type a reached signature names (through a type
    /// alias, a field, a return) that no path reaches, at the path rustdoc
    /// records for it, until no reached item names another.
    fn walk(&mut self, lib: &str, load: &mut Load<'_>) -> anyhow::Result<()> {
        if !self.ensure(lib, load)? {
            bail!("`{lib}` is not a workspace library");
        }
        let root = api_closure::id_text(&self.doc(lib)["root"]);
        let mut queue: BTreeSet<(usize, Vec<String>, Key)> = BTreeSet::new();
        queue.insert((1, vec![lib.to_owned()], (lib.to_owned(), root)));
        let mut scanned = BTreeSet::new();
        loop {
            self.drain(&mut queue, load)?;
            for (key, path) in self.named(&mut scanned, load)? {
                // After every facade path: a recorded path is a fallback.
                queue.insert((usize::MAX / 2 + path.len(), path, key));
            }
            if queue.is_empty() {
                return Ok(());
            }
        }
    }

    /// Visits the queued paths, shortest first, and the modules they reach.
    fn drain(
        &mut self,
        queue: &mut BTreeSet<(usize, Vec<String>, Key)>,
        load: &mut Load<'_>,
    ) -> anyhow::Result<()> {
        while let Some((depth, path, key)) = queue.pop_first() {
            if self.canonical.contains_key(&key) {
                if depth < usize::MAX / 2 {
                    self.reexports.push((path, Target::Item(key)));
                }
                continue;
            }
            self.canonical.insert(key.clone(), path.clone());
            if self.item(&key)["inner"].get("module").is_none() {
                continue;
            }
            let mut globs = BTreeSet::new();
            for ((name, _), target) in self.children(&key, load, &mut globs)? {
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

    /// The public workspace types, traits and aliases that the reached items
    /// not yet in `scanned` name and no path reached, with the path rustdoc
    /// records for each.
    fn named(
        &mut self,
        scanned: &mut BTreeSet<Key>,
        load: &mut Load<'_>,
    ) -> anyhow::Result<Vec<(Key, Vec<String>)>> {
        let fresh: Vec<Key> = self
            .canonical
            .keys()
            .filter(|key| !scanned.contains(*key))
            .cloned()
            .collect();
        let mut references: BTreeSet<(String, String)> = BTreeSet::new();
        for key in fresh {
            scanned.insert(key.clone());
            let (lib, id) = &key;
            let index = &self.doc(lib)["index"];
            let mut stack = vec![id.clone()];
            let mut seen = BTreeSet::new();
            while let Some(id) = stack.pop() {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Some(item) = index.get(&id) else {
                    continue;
                };
                if item["inner"].get("module").is_some() || api_closure::skipped(item) {
                    continue;
                }
                let mut ids = Vec::new();
                referenced_ids(&item["inner"], &mut ids);
                references.extend(ids.into_iter().map(|id| (lib.clone(), id)));
                stack.extend(members(index, &item["inner"]));
            }
        }
        let mut found = Vec::new();
        for (lib, id) in references {
            let Some((key, path)) = self.nameable(&lib, &id, load)? else {
                continue;
            };
            if !self.canonical.contains_key(&key) {
                found.push((key, path));
            }
        }
        found.sort();
        found.dedup();
        Ok(found)
    }

    /// The workspace item `id` of `lib`'s document refers to, with its
    /// recorded path, when it is a public type, trait or alias.
    fn nameable(
        &mut self,
        lib: &str,
        id: &str,
        load: &mut Load<'_>,
    ) -> anyhow::Result<Option<(Key, Vec<String>)>> {
        let doc = self.doc(lib);
        let (key, path) = if doc["index"].get(id).is_some() {
            let Some(path) = segments(&doc["paths"][id]["path"]) else {
                return Ok(None);
            };
            ((lib.to_owned(), id.to_owned()), path)
        } else {
            let Some((other, path)) = send_positions::external(doc, id) else {
                return Ok(None);
            };
            if !self.ensure(&other, load)? {
                return Ok(None);
            }
            let Some(other_id) = self.recorded[&other].get(&path).cloned() else {
                return Ok(None);
            };
            ((other, other_id), path)
        };
        let item = self.item(&key);
        let kind = kind_of(item);
        let nameable = matches!(
            kind.as_str(),
            "struct" | "enum" | "union" | "trait" | "type_alias" | "trait_alias"
        );
        Ok((nameable && send_positions::public(item) && !hidden(item)).then_some((key, path)))
    }
    /// The names a module gives, with where each leads: its own public items
    /// and re-exports first, then what its glob re-exports bring that those
    /// do not shadow. A name is unique within its namespace (a derive and a
    /// trait may share one); a function-like macro's name ends in `!`.
    fn children(
        &mut self,
        module: &Key,
        load: &mut Load<'_>,
        globs: &mut BTreeSet<Key>,
    ) -> anyhow::Result<BTreeMap<(String, Namespace), Target>> {
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
                let (target, kind) = self.target(lib, re_export, load)?;
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
                names
                    .entry((spelled(name, &kind), Namespace::of(&kind)))
                    .or_insert(target);
                continue;
            }
            let Some(name) = item["name"].as_str() else {
                continue;
            };
            if item["inner"].get("impl").is_some() {
                continue;
            }
            let kind = kind_of(&item);
            names.insert(
                (spelled(name, &kind), Namespace::of(&kind)),
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

    /// Where a re-export in `lib` leads, and the kind of item it names
    /// (`struct`, `proc_derive`, …; empty when rustdoc records none).
    fn target(
        &mut self,
        lib: &str,
        re_export: &Json,
        load: &mut Load<'_>,
    ) -> anyhow::Result<(Target, String)> {
        let source = re_export["source"].as_str().unwrap_or("?").to_owned();
        let id = &re_export["id"];
        if id.is_null() {
            return Ok((Target::Foreign(source), String::new()));
        }
        let id = api_closure::id_text(id);
        let doc = self.doc(lib);
        if let Some(item) = doc["index"].get(&id) {
            let kind = kind_of(item);
            return Ok((Target::Item((lib.to_owned(), id)), kind));
        }
        let recorded_kind = doc["paths"][&id]["kind"].as_str().unwrap_or("").to_owned();
        let Some((other, path)) = send_positions::external(doc, &id) else {
            return Ok((Target::Foreign(source), recorded_kind));
        };
        if !self.ensure(&other, load)? {
            return Ok((Target::Foreign(path.join("::")), recorded_kind));
        }
        Ok(match send_positions::resolve(self.doc(&other), &path) {
            Some(id) => {
                let kind = kind_of(&self.doc(&other)["index"][&id]);
                (Target::Item((other, id)), kind)
            }
            None => (Target::Foreign(path.join("::")), recorded_kind),
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
             # Written by `cargo xtask public-api --write` (ADR-0324); a pull request that\n\
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
                        if private {
                            " { .. }"
                        } else if members.is_empty() {
                            " {}"
                        } else {
                            ""
                        }
                    )
                } else {
                    format!("{head}{where_clause};")
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
                // The implemented type with its arguments: `impl Offset<f64>`
                // offers its items to that specialization only.
                let for_type = render::ty(&names, &data["for"]);
                let context = format!(
                    "{} {for_type}{}",
                    render::params(&names, generics),
                    render::where_clause(&names, generics)
                );
                let plain = render::params(&names, generics).is_empty()
                    && render::where_clause(&names, generics).is_empty()
                    && !for_type.contains('<');
                for member in send_positions::ids(&data["items"]) {
                    let member = &index[&member];
                    if !send_positions::public(member) || hidden(member) {
                        continue;
                    }
                    if let Some(mut line) = associated(&names, path, member, false) {
                        if !plain {
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

/// The kind of an item as rustdoc's `paths` table writes it: `struct`,
/// `function`, `macro` (a function-like macro of either form),
/// `proc_attribute`, `proc_derive`, …
fn kind_of(item: &Json) -> String {
    let inner = &item["inner"];
    match inner["proc_macro"]["kind"].as_str() {
        Some("bang") => "macro".to_owned(),
        Some("attr") => "proc_attribute".to_owned(),
        Some("derive") => "proc_derive".to_owned(),
        _ => inner
            .as_object()
            .and_then(|map| map.keys().next())
            .cloned()
            .unwrap_or_default(),
    }
}

/// Which of Rust's namespaces a name lives in: a type and a value, or a
/// trait and a derive, may share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Namespace {
    Type,
    Value,
    Macro,
}

impl Namespace {
    fn of(kind: &str) -> Self {
        match kind {
            "macro" | "proc_macro" | "proc_attribute" | "proc_derive" => Self::Macro,
            "function" | "constant" | "static" => Self::Value,
            _ => Self::Type,
        }
    }
}

/// A path segment: a function-like macro's name ends in `!`.
fn spelled(name: &str, kind: &str) -> String {
    if kind == "macro" {
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
            // A trait's default is written as `= ..`: whether it has one is
            // the API, its text is the implementation's.
            let value = match inner["value"].as_str() {
                Some(_) if in_trait => " = ..".to_owned(),
                Some(value) => format!(" = {value}"),
                None => String::new(),
            };
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
                "{attributes}type {path}{}{}{}{default}",
                render::params(names, &inner["generics"]),
                if bounds.is_empty() {
                    String::new()
                } else {
                    format!(": {bounds}")
                },
                render::where_clause(names, &inner["generics"])
            )
        }
        _ => return None,
    })
}

/// Every `"id"` an object inside `value` holds: the items a signature, a
/// bound or a field type names. A bare list of ids (a type's fields, an
/// implementation's items) is a list of members, not a reference.
fn referenced_ids(value: &Json, out: &mut Vec<String>) {
    match value {
        Json::Object(map) => {
            if let Some(id) = map.get("id").filter(|id| id.is_u64() || id.is_string()) {
                out.push(api_closure::id_text(id));
            }
            for child in map.values() {
                referenced_ids(child, out);
            }
        }
        Json::Array(list) => {
            for child in list {
                referenced_ids(child, out);
            }
        }
        _ => {}
    }
}

/// The members of an item a user reaches through it: public fields,
/// variants, implementations, a trait's items and an implementation's.
fn members(index: &Json, inner: &Json) -> Vec<String> {
    let public = |id: &String| {
        index
            .get(id)
            .is_some_and(|item| send_positions::public(item) && !hidden(item))
    };
    let mut out = Vec::new();
    if let Some(data) = inner.get("struct").or_else(|| inner.get("union")) {
        let kind = &data["kind"];
        out.extend(send_positions::ids(&kind["plain"]["fields"]).filter(public));
        out.extend(send_positions::ids(&kind["tuple"]).filter(public));
        out.extend(send_positions::ids(&data["fields"]).filter(public));
        out.extend(send_positions::ids(&data["impls"]));
    } else if let Some(data) = inner.get("enum") {
        out.extend(send_positions::ids(&data["variants"]));
        out.extend(send_positions::ids(&data["impls"]));
    } else if let Some(variant) = inner.get("variant") {
        out.extend(send_positions::ids(&variant["kind"]["tuple"]));
        out.extend(send_positions::ids(&variant["kind"]["struct"]["fields"]));
    } else if let Some(definition) = inner.get("trait") {
        out.extend(send_positions::ids(&definition["items"]));
    } else if let Some(implementation) = inner.get("impl") {
        out.extend(send_positions::ids(&implementation["items"]).filter(public));
    }
    out
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
# Written by `cargo xtask public-api --write` (ADR-0324); a pull request that
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
    use serde_json::json;

    fn entry(id: u64, name: Option<&str>, visibility: &str, inner: Json) -> (String, Json) {
        (
            id.to_string(),
            json!({ "id": id, "crate_id": 0, "name": name, "visibility": visibility,
                    "attrs": [], "deprecation": null, "inner": inner }),
        )
    }

    fn no_generics() -> Json {
        json!({ "params": [], "where_predicates": [] })
    }

    fn method(name: &str, id: u64) -> (String, Json) {
        entry(
            id,
            Some(name),
            "public",
            json!({ "function": {
            "sig": { "inputs": [["self", { "generic": "Self" }]], "output": null, "is_c_variadic": false },
            "generics": no_generics(), "header": {}, "has_body": true } }),
        )
    }

    /// A crate `lib` whose root exports a trait and a derive of one name, an
    /// alias of a type no path reaches, a specialized inherent
    /// implementation, a trait constant with a default and an associated
    /// type with a where clause.
    fn shapes() -> Json {
        let f64_point = json!({ "resolved_path": { "path": "Point", "id": 3,
            "args": { "angle_bracketed": { "args": [{ "type": { "primitive": "f64" } }], "constraints": [] } } } });
        let index: serde_json::Map<String, Json> = [
            entry(0, Some("lib"), "public", json!({ "module": { "is_crate": true, "items": [1, 2, 3, 10, 20], "is_stripped": false } })),
            entry(1, Some("Inspect"), "public", json!({ "trait": { "is_auto": false, "is_unsafe": false, "is_dyn_compatible": true,
                "items": [11, 12], "generics": no_generics(), "bounds": [], "implementations": [] } })),
            entry(2, Some("Inspect"), "public", json!({ "use": { "source": "lib_macros::Inspect", "name": "Inspect", "id": 90, "is_glob": false } })),
            entry(3, Some("Point"), "public", json!({ "struct": { "kind": "unit", "generics": no_generics(), "impls": [4] } })),
            entry(4, None, "default", json!({ "impl": { "is_unsafe": false, "generics": no_generics(), "provided_trait_methods": [],
                "trait": null, "for": f64_point, "items": [5], "is_negative": false, "is_synthetic": false, "blanket_impl": null } })),
            method("direction", 5),
            entry(10, Some("Context"), "public", json!({ "type_alias": { "type": { "resolved_path": { "path": "Inner", "id": 30, "args": null } }, "generics": no_generics() } })),
            entry(11, Some("EPSILON"), "default", json!({ "assoc_const": { "type": { "primitive": "f64" }, "value": "1e-6" } })),
            entry(12, Some("Ctx"), "default", json!({ "assoc_type": { "generics": {
                "params": [{ "name": "'ctx", "kind": { "lifetime": { "outlives": [] } } }],
                "where_predicates": [{ "bound_predicate": { "type": { "generic": "Self" }, "bounds": [{ "outlives": "'ctx" }], "generic_params": [] } }] },
                "bounds": [], "type": null } })),
            entry(20, Some("hidden"), "crate", json!({ "module": { "is_crate": false, "items": [30], "is_stripped": false } })),
            entry(30, Some("Inner"), "public", json!({ "struct": { "kind": "unit", "generics": no_generics(), "impls": [31] } })),
            entry(31, None, "default", json!({ "impl": { "is_unsafe": false, "generics": no_generics(), "provided_trait_methods": [],
                "trait": null, "for": { "resolved_path": { "path": "Inner", "id": 30, "args": null } }, "items": [32],
                "is_negative": false, "is_synthetic": false, "blanket_impl": null } })),
            method("layout_child", 32),
        ]
        .into_iter()
        .collect();
        json!({
            "root": 0,
            "index": index,
            "paths": {
                "1": { "crate_id": 0, "path": ["lib", "Inspect"], "kind": "trait" },
                "3": { "crate_id": 0, "path": ["lib", "Point"], "kind": "struct" },
                "10": { "crate_id": 0, "path": ["lib", "Context"], "kind": "type_alias" },
                "30": { "crate_id": 0, "path": ["lib", "hidden", "Inner"], "kind": "struct" },
                "90": { "crate_id": 1, "path": ["lib_macros", "Inspect"], "kind": "proc_derive" },
            },
            "external_crates": { "1": { "name": "lib_macros", "html_root_url": null } },
            "format_version": 61,
        })
    }

    #[test]
    fn public_api_writes_namespaces_specializations_defaults_and_aliased_types() {
        let mut load =
            |lib: &str| -> anyhow::Result<Option<Json>> { Ok((lib == "lib").then(shapes)) };
        let mut surface = Surface::default();
        surface.walk("lib", &mut load).expect("the shapes render");
        let text = surface.render();
        for line in [
            // A trait and a derive of one name are both re-exports a user names.
            "pub trait lib::Inspect",
            "reexport lib::Inspect => lib_macros::Inspect",
            // A trait constant's default, and an associated type's where clause.
            "const lib::Inspect::EPSILON: f64 = ..",
            "type lib::Inspect::Ctx<'ctx> where Self: 'ctx",
            // The specialization that owns an inherent method.
            "pub fn lib::Point::direction(self) // impl lib::Point<f64>",
            // A type only an alias reaches, with its methods.
            "pub type lib::Context = lib::hidden::Inner",
            "pub struct lib::hidden::Inner;",
            "pub fn lib::hidden::Inner::layout_child(self)",
        ] {
            assert!(
                text.lines().any(|candidate| candidate == line),
                "missing `{line}` in:\n{text}"
            );
        }
    }

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
