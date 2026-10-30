//! `cargo xtask send-positions`: every place the public API of `flui` and
//! `flui-sdk` names `Send` or `Sync` is listed, and the list only shrinks
//! (ADR-0089 §6, ADR-0136).
//!
//! UI state is owner-local (ADR-0136): a bound that asks for `Send` where a
//! value never leaves its owner thread shuts out every `Rc` an application
//! holds. The positions that stay are the ones a value really crosses a
//! thread through (a panic payload, a waker, a worker's result); a new one is
//! a decision, and the gate makes it visible.
//!
//! **What counts.** rustdoc's JSON output (the loader `api-closure` uses, with
//! private items) for `flui` and `flui-sdk` with every feature, and for each
//! workspace crate their public re-exports reach, followed through the
//! re-exports of those crates in turn. From each crate's re-exported items the
//! gate walks what a user can name: public items of public modules, public
//! fields, variants, implementations of reached types and the items inside
//! them, the implementations of a reached trait for a generic or foreign type
//! (a blanket `impl<T: Send> Trait for T`), and a reached trait's
//! supertraits, private ones included (a sealed `Base: Send`). A position is a
//! path to `core::marker::Send` or `Sync` that the author wrote in one of
//! those items:
//!
//! - `supertrait`: a trait's bound on `Self`;
//! - `bound`: a generic parameter's or where-clause's bound, on an item or an
//!   implementation, or an associated type's;
//! - `dyn`: `dyn Trait + Send` in a type alias, a field or a signature;
//! - `impl-trait`: `-> impl Trait + Send`;
//! - `impl`: an explicit `impl Send for T`.
//!
//! A synthetic auto-trait implementation (rustdoc's `impl Send for Size`,
//! which follows from the fields) is not a position: no author wrote it, a
//! new data type would add one against a list that only shrinks, and which
//! types are or are not `Send` is pinned elsewhere (trybuild for the owner
//! types, a consumer test for the handles meant to cross). Neither is a
//! blanket implementation's copy on each type; its original is counted once.
//!
//! **The list.** [`ALLOWLIST`] names each position by item path, kind and
//! marker, with a count when the item writes it more than once. A position
//! the list does not cover is a finding; so is a listed one the code no longer
//! has, so the list only shrinks. `--seed` prints the list the surface needs.
//!
//! **Limits.** The surface is the host's build: an item only another target
//! builds is not seen. An item path is the one rustdoc records where the item
//! is defined, so moving an item changes its entry.
//!
//! `--self-test` runs the rule over a built-in rustdoc JSON document with
//! one planted position of each kind and fails unless it reports exactly
//! those (ADR-0078 §4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, bail};
use cargo_metadata::TargetKind;
use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::api_closure::{self, Build, Items};
use crate::util;

/// The list, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/send-positions.toml";

/// The packages whose public API the gate walks from.
const ROOTS: &[&str] = &["flui", "flui-sdk"];

/// Arguments for `cargo xtask send-positions`.
#[derive(Debug, clap::Args)]
pub(crate) struct SendPositionsArgs {
    /// The toolchain that writes rustdoc's JSON output (`rustup run <toolchain>`).
    #[arg(long, default_value = "nightly")]
    toolchain: String,
    /// Run the rule over a built-in rustdoc JSON document with planted
    /// positions instead of the workspace; exit 1 unless it reports exactly
    /// those.
    #[arg(long)]
    self_test: bool,
    /// Print the list the surface needs instead of checking it.
    #[arg(long, conflicts_with = "self_test")]
    seed: bool,
}

/// `cargo xtask send-positions`. Exit code 1 lists every finding.
pub(crate) fn send_positions(args: &SendPositionsArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let libraries = libraries(&metadata);
    let mut entries: BTreeMap<String, BTreeSet<Vec<String>>> = BTreeMap::new();
    for package in ROOTS {
        let lib = libraries
            .iter()
            .find(|(_, name)| name.as_str() == *package)
            .map(|(lib, _)| lib.clone())
            .with_context(|| format!("`{package}` is not a workspace library"))?;
        entries.entry(lib.clone()).or_default().insert(vec![lib]);
    }
    // Each crate's walk can reach re-exports of further crates; walk again
    // until no crate gains an entry.
    let mut docs: BTreeMap<String, Json> = BTreeMap::new();
    let mut reaches: BTreeMap<String, (usize, Reach)> = BTreeMap::new();
    let unwalked = |entries: &BTreeMap<String, BTreeSet<Vec<String>>>,
                    reaches: &BTreeMap<String, (usize, Reach)>| {
        entries
            .iter()
            .find(|(lib, paths)| reaches.get(*lib).map(|(walked, _)| *walked) != Some(paths.len()))
            .map(|(lib, paths)| (lib.clone(), paths.clone()))
    };
    while let Some((lib, paths)) = unwalked(&entries, &reaches) {
        if !docs.contains_key(&lib) {
            let package = &libraries[&lib];
            let build = Build {
                all_features: true,
                ..Build::HOST
            };
            let doc = api_closure::rustdoc_json(
                &root,
                &args.toolchain,
                package,
                &lib,
                &build,
                Items::Private,
            )?;
            docs.insert(lib.clone(), doc);
        }
        let reach =
            reach(&docs[&lib], &paths, &libraries).with_context(|| format!("walking `{lib}`"))?;
        for (other, path) in &reach.exits {
            entries
                .entry(other.clone())
                .or_default()
                .insert(path.clone());
        }
        reaches.insert(lib, (paths.len(), reach));
    }
    let mut surface = Surface::new();
    for (lib, (_, reach)) in &reaches {
        positions(&docs[lib], reach, &root, &mut surface);
    }
    if args.seed {
        print!("{}", seed(&surface));
        return Ok(ExitCode::SUCCESS);
    }
    let path = root.join(ALLOWLIST);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let list = parse_list(&text).with_context(|| format!("parsing {ALLOWLIST}"))?;
    let findings = check(&surface, &list);
    if findings.is_empty() {
        let crates: Vec<&str> = reaches.keys().map(String::as_str).collect();
        println!(
            "send-positions: {} positions in {} crates ({}), each listed in {ALLOWLIST}",
            surface.values().map(|seen| seen.count).sum::<usize>(),
            crates.len(),
            crates.join(", ")
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("send-positions: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// Every workspace library that is not a proc-macro, by crate name, with its
/// package name.
fn libraries(metadata: &cargo_metadata::Metadata) -> BTreeMap<String, String> {
    metadata
        .workspace_packages()
        .into_iter()
        .flat_map(|package| {
            package
                .targets
                .iter()
                .filter(|target| {
                    target.kind.iter().any(|kind| {
                        matches!(
                            kind,
                            TargetKind::Lib
                                | TargetKind::RLib
                                | TargetKind::DyLib
                                | TargetKind::CDyLib
                                | TargetKind::StaticLib
                        )
                    })
                })
                .map(|target| (target.name.replace('-', "_"), package.name.to_string()))
        })
        .collect()
}

/// The auto trait a position names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
enum Marker {
    Send,
    Sync,
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Send => "Send",
            Self::Sync => "Sync",
        })
    }
}

/// How an item names the marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum At {
    Supertrait,
    Bound,
    Dyn,
    ImplTrait,
    Impl,
}

impl fmt::Display for At {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Supertrait => "supertrait",
            Self::Bound => "bound",
            Self::Dyn => "dyn",
            Self::ImplTrait => "impl-trait",
            Self::Impl => "impl",
        })
    }
}

/// One position: the item that writes it, how, and the marker.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    item: String,
    at: At,
    marker: Marker,
}

/// How often an item writes a position, and where the item is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    count: usize,
    location: String,
}

/// Every position the surface writes.
type Surface = BTreeMap<Position, Seen>;

/// What one crate's walk reached.
#[derive(Debug, Default)]
struct Reach {
    /// Every reached item of the crate, by rustdoc id, with the item it was
    /// reached from (`None` for an entry).
    parents: BTreeMap<String, Option<String>>,
    /// Re-exports of another workspace crate's items: the crate and the
    /// item's path.
    exits: BTreeSet<(String, Vec<String>)>,
}

/// Walks a crate's rustdoc JSON from the items at `entries`, each a path that
/// starts with the crate's name.
fn reach(
    doc: &Json,
    entries: &BTreeSet<Vec<String>>,
    libraries: &BTreeMap<String, String>,
) -> anyhow::Result<Reach> {
    let mut reach = Reach::default();
    let mut stack: Vec<(String, Option<String>)> = Vec::new();
    for path in entries {
        let id = resolve(doc, path)
            .with_context(|| format!("`{}` is not in the crate's API", path.join("::")))?;
        stack.push((id, None));
    }
    let index = &doc["index"];
    while let Some((id, parent)) = stack.pop() {
        if reach.parents.contains_key(&id) {
            continue;
        }
        let Some(item) = index.get(&id) else {
            continue;
        };
        reach.parents.insert(id.clone(), parent);
        let mut children = Vec::new();
        let inner = &item["inner"];
        if let Some(module) = inner.get("module") {
            children.extend(ids(&module["items"]).filter(|child| public(&index[child])));
        } else if let Some(re_export) = inner.get("use") {
            let target = &re_export["id"];
            if !target.is_null() {
                let target = api_closure::id_text(target);
                if index.get(&target).is_some() {
                    children.push(target);
                } else if let Some((lib, path)) = external(doc, &target)
                    && libraries.contains_key(&lib)
                {
                    reach.exits.insert((lib, path));
                }
            }
        } else if let Some(data) = inner.get("struct").or_else(|| inner.get("union")) {
            children.extend(fields(data).filter(|child| public(&index[child])));
            children.extend(implementations(index, &data["impls"]));
        } else if let Some(data) = inner.get("enum") {
            children.extend(ids(&data["variants"]));
            children.extend(implementations(index, &data["impls"]));
        } else if let Some(variant) = inner.get("variant") {
            children.extend(ids(&variant["kind"]["struct"]["fields"]));
            children.extend(ids(&variant["kind"]["tuple"]));
        } else if let Some(definition) = inner.get("trait") {
            children.extend(ids(&definition["items"]));
            // Supertraits of this crate, private ones included: a sealed
            // `Base: Send` binds every implementor of the public trait.
            let mut named = Vec::new();
            for bound in definition["bounds"].as_array().into_iter().flatten() {
                named.extend(
                    bound["trait_bound"]["trait"]
                        .get("id")
                        .map(api_closure::id_text),
                );
            }
            children.extend(named.into_iter().filter(|id| index.get(id).is_some()));
            // An implementation for a type of this crate is reached through
            // that type; one for a generic or foreign type only through here.
            children.extend(
                implementations(index, &definition["implementations"]).filter(|implementation| {
                    let for_type = &index[implementation]["inner"]["impl"]["for"];
                    let local = for_type["resolved_path"]
                        .get("id")
                        .is_some_and(|id| index.get(api_closure::id_text(id)).is_some());
                    !local
                }),
            );
        } else if let Some(implementation) = inner.get("impl") {
            let inherent = implementation["trait"].is_null();
            children.extend(
                ids(&implementation["items"]).filter(|child| !inherent || public(&index[child])),
            );
        }
        stack.extend(children.into_iter().map(|child| (child, Some(id.clone()))));
    }
    Ok(reach)
}

/// The ids in a list of ids, skipping a stripped (`null`) one.
fn ids(list: &Json) -> impl Iterator<Item = String> + '_ {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|id| !id.is_null())
        .map(api_closure::id_text)
}

/// A struct's or union's fields, named or tuple.
fn fields(data: &Json) -> impl Iterator<Item = String> + '_ {
    let kind = &data["kind"];
    ids(&kind["plain"]["fields"]).chain(ids(&kind["tuple"]))
}

/// The implementations in `list` that an author wrote: not a synthetic one,
/// not a blanket implementation's copy.
fn implementations<'a>(index: &'a Json, list: &'a Json) -> impl Iterator<Item = String> + 'a {
    ids(list).filter(|id| {
        index
            .get(id)
            .is_some_and(|item| !api_closure::skipped(item))
    })
}

/// Public to whoever can name the parent: `pub`, or the default of a trait's
/// items, a variant or an implementation's items.
fn public(item: &Json) -> bool {
    matches!(item["visibility"].as_str(), Some("public" | "default"))
}

/// The crate and path of an item another crate defines, as `doc` records it.
fn external(doc: &Json, id: &str) -> Option<(String, Vec<String>)> {
    let summary = doc["paths"].get(id)?;
    let krate = api_closure::id_text(&summary["crate_id"]);
    let lib = doc["external_crates"].get(&krate)?["name"]
        .as_str()?
        .to_owned();
    let path = summary["path"]
        .as_array()?
        .iter()
        .map(|segment| segment.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some((lib, path))
}

/// The id of the item at `path` (crate name first): the path rustdoc records
/// for a local item, or the walk from the crate root through public modules
/// and re-exports.
fn resolve(doc: &Json, path: &[String]) -> Option<String> {
    let recorded = doc["paths"].as_object()?.iter().find(|(_, summary)| {
        summary["crate_id"].as_u64() == Some(0)
            && summary["path"].as_array().is_some_and(|segments| {
                segments
                    .iter()
                    .map(Json::as_str)
                    .eq(path.iter().map(|s| Some(s.as_str())))
            })
    });
    if let Some((id, _)) = recorded {
        return Some(id.clone());
    }
    let mut current = api_closure::id_text(&doc["root"]);
    for segment in path.iter().skip(1) {
        current = child(doc, &current, segment)?;
    }
    Some(current)
}

/// The item a module names `name`, through a re-export or a glob re-export of
/// this crate.
fn child(doc: &Json, module: &str, name: &str) -> Option<String> {
    let index = &doc["index"];
    for id in ids(&index[module]["inner"]["module"]["items"]) {
        let item = &index[&id];
        let Some(re_export) = item["inner"].get("use") else {
            if item["name"].as_str() == Some(name) {
                return Some(id);
            }
            continue;
        };
        let target = api_closure::id_text(&re_export["id"]);
        if index.get(&target).is_none() {
            continue;
        }
        if re_export["is_glob"].as_bool() == Some(true) {
            if let Some(found) = child(doc, &target, name) {
                return Some(found);
            }
        } else if re_export["name"].as_str() == Some(name) {
            return Some(target);
        }
    }
    None
}

/// The rustdoc ids of `core::marker::Send` and `Sync` in `doc`.
fn markers(doc: &Json) -> BTreeMap<String, Marker> {
    let mut markers = BTreeMap::new();
    for (id, summary) in doc["paths"].as_object().into_iter().flatten() {
        let Some(path) = summary["path"].as_array() else {
            continue;
        };
        let path: Vec<&str> = path.iter().filter_map(Json::as_str).collect();
        let marker = match path.as_slice() {
            ["core", "marker", "Send"] => Marker::Send,
            ["core", "marker", "Sync"] => Marker::Sync,
            _ => continue,
        };
        markers.insert(id.clone(), marker);
    }
    markers
}

/// Adds every position the reached items of one crate write to `surface`.
fn positions(doc: &Json, reach: &Reach, root: &Path, surface: &mut Surface) {
    let markers = markers(doc);
    if markers.is_empty() {
        return;
    }
    let index = &doc["index"];
    for id in reach.parents.keys() {
        let item = &index[id];
        let Some((kind, inner)) = item["inner"]
            .as_object()
            .and_then(|inner| inner.iter().next())
        else {
            continue;
        };
        // A re-export names an item, it does not bound one; `impl !Send`
        // takes the marker away.
        if kind == "use" || inner["is_negative"].as_bool() == Some(true) {
            continue;
        }
        let mut found = Vec::new();
        named(inner, &mut Vec::new(), &markers, &mut found);
        for (keys, marker) in found {
            let position = Position {
                item: label(doc, reach, id),
                at: classify(kind, &keys),
                marker,
            };
            surface
                .entry(position)
                .or_insert_with(|| Seen {
                    count: 0,
                    location: api_closure::location(item, root),
                })
                .count += 1;
        }
    }
}

/// Every path to a marker inside `value`, with the object keys that lead to
/// it.
fn named<'a>(
    value: &'a Json,
    keys: &mut Vec<&'a str>,
    markers: &BTreeMap<String, Marker>,
    found: &mut Vec<(Vec<&'a str>, Marker)>,
) {
    match value {
        Json::Object(map) => {
            if map.contains_key("path")
                && let Some(marker) = map
                    .get("id")
                    .and_then(|id| markers.get(&api_closure::id_text(id)))
            {
                found.push((keys.clone(), *marker));
            }
            for (key, child) in map {
                keys.push(key);
                named(child, keys, markers, found);
                keys.pop();
            }
        }
        Json::Array(list) => {
            for child in list {
                named(child, keys, markers, found);
            }
        }
        _ => {}
    }
}

/// How an item of `kind` names a marker at `keys` under its `inner.<kind>`.
fn classify(kind: &str, keys: &[&str]) -> At {
    if keys.contains(&"dyn_trait") {
        At::Dyn
    } else if keys.contains(&"impl_trait") {
        At::ImplTrait
    } else if kind == "impl" && keys == ["trait"] {
        At::Impl
    } else if kind == "trait" && keys.first() == Some(&"bounds") {
        At::Supertrait
    } else {
        At::Bound
    }
}

/// An item's path: the one rustdoc records, `impl Trait for Type` for an
/// implementation, or the parent's label and the item's name.
fn label(doc: &Json, reach: &Reach, id: &str) -> String {
    let item = &doc["index"][id];
    if let Some(implementation) = item["inner"].get("impl") {
        let for_type = type_label(doc, &implementation["for"]);
        return match implementation["trait"].as_object() {
            Some(name) => format!("impl {} for {for_type}", path_label(doc, name)),
            None => format!("impl {for_type}"),
        };
    }
    if let Some(path) = doc["paths"]
        .get(id)
        .filter(|summary| summary["crate_id"].as_u64() == Some(0))
        .and_then(|summary| summary["path"].as_array())
    {
        return path
            .iter()
            .filter_map(Json::as_str)
            .collect::<Vec<_>>()
            .join("::");
    }
    let name = item["name"].as_str().unwrap_or("_");
    match reach.parents.get(id).and_then(Option::as_ref) {
        Some(parent) => format!("{}::{name}", label(doc, reach, parent)),
        None => name.to_owned(),
    }
}

/// A path object's label: the recorded path of its item, or the path as
/// written.
fn path_label(doc: &Json, path: &serde_json::Map<String, Json>) -> String {
    path.get("id")
        .and_then(|id| doc["paths"].get(api_closure::id_text(id)))
        .and_then(|summary| summary["path"].as_array())
        .map(|segments| {
            segments
                .iter()
                .filter_map(Json::as_str)
                .collect::<Vec<_>>()
                .join("::")
        })
        .or_else(|| path.get("path").and_then(Json::as_str).map(str::to_owned))
        .unwrap_or_else(|| "_".to_owned())
}

/// A self type's label: a path, a generic's or a primitive's name, through
/// references; `_` for any other shape.
fn type_label(doc: &Json, ty: &Json) -> String {
    if let Some(path) = ty["resolved_path"].as_object() {
        return path_label(doc, path);
    }
    if let Some(name) = ty["generic"].as_str().or_else(|| ty["primitive"].as_str()) {
        return name.to_owned();
    }
    if let Some(reference) = ty.get("borrowed_ref") {
        return format!("&{}", type_label(doc, &reference["type"]));
    }
    "_".to_owned()
}

/// The list as [`ALLOWLIST`] writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFile {
    reason: String,
    #[serde(default)]
    positions: Vec<Entry>,
}

/// One listed position.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    item: String,
    at: At,
    marker: Marker,
    #[serde(default = "one")]
    count: usize,
}

fn one() -> usize {
    1
}

/// The listed positions and their counts. A position listed twice, or with a
/// count of zero, is an error.
fn parse_list(text: &str) -> anyhow::Result<BTreeMap<Position, usize>> {
    let file: ListFile = toml::from_str(text)?;
    anyhow::ensure!(
        !file.reason.trim().is_empty(),
        "the list's `reason` is empty"
    );
    let mut list = BTreeMap::new();
    for entry in file.positions {
        let position = Position {
            item: entry.item,
            at: entry.at,
            marker: entry.marker,
        };
        if entry.count == 0 {
            bail!(
                "`{}` ({} {}): a count of 0; remove the entry",
                position.item,
                position.at,
                position.marker
            );
        }
        if list.contains_key(&position) {
            bail!(
                "`{}` ({} {}) is listed twice",
                position.item,
                position.at,
                position.marker
            );
        }
        list.insert(position, entry.count);
    }
    Ok(list)
}

/// What is wrong with the surface against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// The surface writes a position more often than the list allows.
    Unlisted {
        position: Position,
        found: usize,
        listed: usize,
        location: String,
    },
    /// The list allows a position more often than the surface writes it.
    Stale {
        position: Position,
        found: usize,
        listed: usize,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted {
                position,
                found,
                listed,
                location,
            } => write!(
                f,
                "{location}: `{}` names {} at `{}` {found} time(s), {ALLOWLIST} allows {listed}: \
                 an owner-local value does not need it (ADR-0136); if a value really crosses a \
                 thread here, list the position",
                position.item, position.marker, position.at
            ),
            Self::Stale {
                position,
                found,
                listed,
            } => write!(
                f,
                "`{}` names {} at `{}` {found} time(s), {ALLOWLIST} allows {listed}: lower the \
                 entry to what the code has",
                position.item, position.marker, position.at
            ),
        }
    }
}

/// Every position the surface writes more often than listed, and every entry
/// that allows more than the surface writes.
fn check(surface: &Surface, list: &BTreeMap<Position, usize>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (position, seen) in surface {
        let listed = list.get(position).copied().unwrap_or(0);
        if seen.count > listed {
            findings.push(Finding::Unlisted {
                position: position.clone(),
                found: seen.count,
                listed,
                location: seen.location.clone(),
            });
        }
    }
    for (position, &listed) in list {
        let found = surface.get(position).map_or(0, |seen| seen.count);
        if found < listed {
            findings.push(Finding::Stale {
                position: position.clone(),
                found,
                listed,
            });
        }
    }
    findings.sort();
    findings
}

/// The list the surface needs, in [`ALLOWLIST`]'s format.
fn seed(surface: &Surface) -> String {
    let mut out = String::from(
        "# Every place the public API of `flui` and `flui-sdk` names `Send` or `Sync`\n\
         # (ADR-0089 §6, ADR-0136), written by `cargo xtask send-positions --seed`. The list\n\
         # only shrinks: the gate fails on a position it does not cover and on an entry that\n\
         # allows more than the code has.\n\
         reason = \"a value crosses a thread here: a panic payload, a waker, a worker's result \
         or a handle meant for another thread\"\n\npositions = [\n",
    );
    for (position, seen) in surface {
        let _ = write!(
            out,
            "    {{ item = \"{}\", at = \"{}\", marker = \"{}\"",
            position.item, position.at, position.marker
        );
        if seen.count > 1 {
            let _ = write!(out, ", count = {}", seen.count);
        }
        out.push_str(" },\n");
    }
    out.push_str("]\n");
    out
}

/// A path to `core::marker::Send` (id 90) or `Sync` (id 91), as a bound,
/// a `dyn` trait or an implemented trait writes it.
fn marker_path(id: u64) -> Json {
    let name = if id == SEND { "Send" } else { "Sync" };
    json!({ "path": name, "id": id, "args": null })
}

const SEND: u64 = 90;
const SYNC: u64 = 91;

fn bound(id: u64) -> Json {
    json!({ "trait_bound": { "trait": marker_path(id), "generic_params": [], "modifier": "none" } })
}

fn dyn_type(id: u64) -> Json {
    json!({ "dyn_trait": { "traits": [{ "trait": marker_path(id), "generic_params": [] }], "lifetime": null } })
}

fn generics_with(bounds: Json) -> Json {
    json!({
        "params": [{ "name": "T", "kind": { "type": { "bounds": bounds, "default": null, "is_synthetic": false } } }],
        "where_predicates": []
    })
}

fn no_generics() -> Json {
    json!({ "params": [], "where_predicates": [] })
}

fn item(id: u64, name: Option<&str>, visibility: &str, inner: Json) -> (String, Json) {
    (
        id.to_string(),
        json!({
            "id": id,
            "crate_id": 0,
            "name": name,
            "visibility": visibility,
            "span": { "filename": "fixture/src/lib.rs", "begin": [id, 0], "end": [id, 0] },
            "inner": inner,
        }),
    )
}

fn function(inputs: Json, output: Json, generics: Json) -> Json {
    json!({ "function": { "sig": { "inputs": inputs, "output": output, "is_c_variadic": false }, "generics": generics, "has_body": true } })
}

fn implementation(trait_: Json, for_type: Json, generics: Json, synthetic: bool) -> Json {
    json!({ "impl": {
        "is_unsafe": false, "generics": generics, "provided_trait_methods": [], "trait": trait_,
        "for": for_type, "items": [], "is_negative": false, "is_synthetic": synthetic, "blanket_impl": null
    } })
}

/// The self-test's crate `fixture`, as rustdoc's JSON writes it with private
/// items: one planted position of each kind, one hidden in each place the
/// walk must not reach, and the synthetic and blanket implementations it
/// must skip.
fn fixture() -> Json {
    let s = json!({ "resolved_path": { "path": "S", "id": 5, "args": null } });
    let index: serde_json::Map<String, Json> = [
        item(0, Some("fixture"), "public", json!({ "module": { "is_crate": true, "items": [1, 2, 3, 4, 5, 9, 10, 12, 14, 15], "is_stripped": false } })),
        // supertrait
        item(1, Some("Bounded"), "public", json!({ "trait": { "is_auto": false, "is_unsafe": false, "is_dyn_compatible": true, "items": [], "generics": no_generics(), "bounds": [bound(SEND)], "implementations": [] } })),
        // bound of a generic parameter
        item(2, Some("spawn"), "public", function(json!([]), json!(null), generics_with(json!([bound(SYNC)])))),
        // bound of a where-clause
        item(3, Some("hand_over"), "public", function(json!([]), json!(null), json!({ "params": [], "where_predicates": [{ "bound_predicate": { "type": { "generic": "T" }, "bounds": [bound(SEND)], "generic_params": [] } }] }))),
        // `dyn` in a type alias
        item(4, Some("Callback"), "public", json!({ "type_alias": { "type": dyn_type(SEND), "generics": no_generics() } })),
        // `dyn` in a public field, an explicit impl and a synthetic one
        item(5, Some("S"), "public", json!({ "struct": { "kind": { "plain": { "fields": [6, 17], "has_stripped_fields": false } }, "generics": no_generics(), "impls": [7, 8, 18] } })),
        item(6, Some("payload"), "public", json!({ "struct_field": dyn_type(SEND) })),
        item(7, None, "default", implementation(marker_path(SYNC), s.clone(), no_generics(), false)),
        item(8, None, "default", implementation(marker_path(SEND), s.clone(), no_generics(), true)),
        // `dyn` in a signature
        item(9, Some("run"), "public", function(json!([["job", dyn_type(SEND)]]), json!(null), no_generics())),
        // a private sealed supertrait, and the blanket impl that grants it
        item(10, Some("Sealed"), "public", json!({ "trait": { "is_auto": false, "is_unsafe": false, "is_dyn_compatible": true, "items": [], "generics": no_generics(), "bounds": [{ "trait_bound": { "trait": { "path": "Base", "id": 11, "args": null }, "generic_params": [], "modifier": "none" } }], "implementations": [13] } })),
        item(11, Some("Base"), "crate", json!({ "trait": { "is_auto": false, "is_unsafe": false, "is_dyn_compatible": true, "items": [], "generics": no_generics(), "bounds": [bound(SYNC)], "implementations": [] } })),
        // `impl Trait + Send` in a return
        item(12, Some("worker"), "public", function(json!([]), json!({ "impl_trait": [bound(SEND)] }), no_generics())),
        item(13, None, "default", implementation(json!({ "path": "Sealed", "id": 10, "args": null }), json!({ "generic": "T" }), generics_with(json!([bound(SEND)])), false)),
        // Not reached: a private function, and a public one in a private module.
        item(14, Some("hidden"), "crate", function(json!([["job", dyn_type(SEND)]]), json!(null), no_generics())),
        item(15, Some("inner"), "crate", json!({ "module": { "is_crate": false, "items": [16], "is_stripped": false } })),
        item(16, Some("unreached"), "public", function(json!([]), json!(null), generics_with(json!([bound(SEND)])))),
        // A private field.
        item(17, Some("secret"), "crate", json!({ "struct_field": dyn_type(SYNC) })),
        // A blanket implementation's copy on `S`.
        item(18, None, "default", json!({ "impl": {
            "is_unsafe": false, "generics": generics_with(json!([bound(SEND)])), "provided_trait_methods": [],
            "trait": { "path": "Sealed", "id": 10, "args": null }, "for": s, "items": [],
            "is_negative": false, "is_synthetic": false, "blanket_impl": { "generic": "T" }
        } })),
    ]
    .into_iter()
    .collect();
    json!({
        "root": 0,
        "crate_version": null,
        "includes_private": true,
        "index": index,
        "paths": {
            "1": { "crate_id": 0, "path": ["fixture", "Bounded"], "kind": "trait" },
            "2": { "crate_id": 0, "path": ["fixture", "spawn"], "kind": "function" },
            "3": { "crate_id": 0, "path": ["fixture", "hand_over"], "kind": "function" },
            "4": { "crate_id": 0, "path": ["fixture", "Callback"], "kind": "type_alias" },
            "5": { "crate_id": 0, "path": ["fixture", "S"], "kind": "struct" },
            "9": { "crate_id": 0, "path": ["fixture", "run"], "kind": "function" },
            "10": { "crate_id": 0, "path": ["fixture", "Sealed"], "kind": "trait" },
            "11": { "crate_id": 0, "path": ["fixture", "sealed", "Base"], "kind": "trait" },
            "12": { "crate_id": 0, "path": ["fixture", "worker"], "kind": "function" },
            "90": { "crate_id": 1, "path": ["core", "marker", "Send"], "kind": "trait" },
            "91": { "crate_id": 1, "path": ["core", "marker", "Sync"], "kind": "trait" },
        },
        "external_crates": { "1": { "name": "core", "html_root_url": null } },
        "format_version": 61,
    })
}

/// The list the self-test checks against: the field's position is listed,
/// one entry is stale, and the alias's count is lower than one but equal.
const FIXTURE_LIST: &str = r#"
    reason = "fixture"
    positions = [
        { item = "fixture::S::payload", at = "dyn", marker = "Send" },
        { item = "fixture::Callback", at = "dyn", marker = "Send" },
        { item = "fixture::removed", at = "bound", marker = "Send" },
        { item = "fixture::Bounded", at = "supertrait", marker = "Send", count = 2 },
    ]
"#;

/// What the self-test must report, as `Display` text heads.
const EXPECTED: [&str; 9] = [
    "`fixture::spawn` names Sync at `bound` 1 time(s)",
    "`fixture::hand_over` names Send at `bound` 1 time(s)",
    "`impl core::marker::Sync for fixture::S` names Sync at `impl` 1 time(s)",
    "`fixture::run` names Send at `dyn` 1 time(s)",
    "`fixture::sealed::Base` names Sync at `supertrait` 1 time(s)",
    "`fixture::worker` names Send at `impl-trait` 1 time(s)",
    "`impl fixture::Sealed for T` names Send at `bound` 1 time(s)",
    "`fixture::removed` names Send at `bound` 0 time(s)",
    "`fixture::Bounded` names Send at `supertrait` 1 time(s), tools/xtask/allowlists/send-positions.toml allows 2",
];

/// Runs the rule over the fixture: what it missed and what it reported
/// beyond [`EXPECTED`].
fn self_test_diff() -> (Vec<String>, Vec<String>) {
    let doc = fixture();
    let entries = BTreeSet::from([vec!["fixture".to_owned()]]);
    let reach = reach(&doc, &entries, &BTreeMap::new()).expect("BUG: the fixture's root resolves");
    let mut surface = Surface::new();
    positions(&doc, &reach, Path::new("."), &mut surface);
    let list = parse_list(FIXTURE_LIST).expect("BUG: the fixture list parses");
    let reported: Vec<String> = check(&surface, &list)
        .iter()
        .map(ToString::to_string)
        .collect();
    let missed = EXPECTED
        .iter()
        .filter(|head| !reported.iter().any(|line| line.contains(*head)))
        .map(|head| (*head).to_owned())
        .collect();
    let extra = reported
        .into_iter()
        .filter(|line| !EXPECTED.iter().any(|head| line.contains(head)))
        .collect();
    (missed, extra)
}

/// `cargo xtask send-positions --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff();
    for head in &missed {
        println!("self-test: MISSED {head}");
    }
    for line in &extra {
        println!("self-test: FALSE POSITIVE {line}");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "send-positions: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_positions_fixture_reports_exactly_the_planted_findings() {
        let (missed, extra) = self_test_diff();
        assert_eq!(missed, Vec::<String>::new(), "missed");
        assert_eq!(extra, Vec::<String>::new(), "false positives");
    }

    #[test]
    fn send_positions_list_rejects_a_position_listed_twice() {
        let error = parse_list(
            r#"
            reason = "twice"
            positions = [
                { item = "a::b", at = "dyn", marker = "Send" },
                { item = "a::b", at = "dyn", marker = "Send", count = 2 },
            ]
            "#,
        )
        .expect_err("a position listed twice is an error");
        assert!(error.to_string().contains("listed twice"), "{error}");
    }

    #[test]
    fn send_positions_seed_reads_back_as_the_surface() {
        let doc = fixture();
        let entries = BTreeSet::from([vec!["fixture".to_owned()]]);
        let reach =
            reach(&doc, &entries, &BTreeMap::new()).expect("BUG: the fixture's root resolves");
        let mut surface = Surface::new();
        positions(&doc, &reach, Path::new("."), &mut surface);
        let list = parse_list(&seed(&surface)).expect("the seed parses");
        assert_eq!(check(&surface, &list), Vec::new());
    }
}
