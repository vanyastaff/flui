//! `cargo xtask workspace`: the shape of the workspace.
//!
//! - **Tiers** (ADR-0081). Each crate under `crates/` or `packages/` (the
//!   official packages, ADR-0088) and the facade declare
//!   `[package.metadata.flui] tier`, `tier-kind` and `order`; the tier names
//!   live in the root manifest's `[workspace.metadata.flui] tiers`, bottom to
//!   top. A normal or build dependency on another workspace package points to a
//!   lower tier, or to the same tier and a smaller `order`, unless the dependent
//!   lists the edge in `edge-exceptions` with the ADR that removes it; an
//!   exception for an edge the rule admits, or one that does not exist, is a
//!   finding, so the list only shrinks. Nothing with a tier depends on a
//!   `tier-kind = "tool"` package. Examples and tools declare only
//!   `tier-kind = "tool"`. Dev-dependencies may point anywhere.
//! - **Reach declarations** (ADR-0081 §2). `reach-forbid` and
//!   `reach-exceptions` are read here, and each exception's `exit` or `grant`
//!   must cite an ADR with a file under `docs/adr`; what they mean over the
//!   resolved graph is `cargo xtask reach` ([`mod@reach`]).
//! - **Layers** (ADR-0041), checked beside the tiers until the `layer` key is
//!   removed. Each crate under `crates/` or `packages/` and the facade declare
//!   `[package.metadata.flui] layer`; the names live in the root manifest's
//!   `[workspace.metadata.flui] layers`. A normal or build dependency on another
//!   workspace package points to the same layer or lower, never higher, and never
//!   at an example or tool. Cargo rejects cycles itself; this adds direction.
//!   Dev-dependencies may point up (tests use `flui-testing`).
//! - **Allowed dependents.** A crate may list `allowed-dependents`, the complete
//!   set of crates allowed a normal or build dependency on it, and
//!   `allowed-dev-dependents`, the same for dev-dependencies: `flui-log`, which
//!   only composition roots link, the design systems, which nothing else
//!   depends on in any form (ADR-0028), and the crates ADR-0081 deletes, whose
//!   dependents are frozen until then. Examples and tools are applications and
//!   may depend on anything.
//! - **wasm32.** `wasm = false` marks a package that cannot build for wasm32;
//!   wasm-check and the fast lane leave it out. `globals` is ADR-0097's
//!   allowlist, which `cargo xtask globals` reads and checks. Any other key in
//!   `[package.metadata.flui]`, or a mistyped value, is an error rather than a
//!   silently ignored setting.
//! - **Modules.** `modules` must be a table; `cargo xtask module-dag` reads
//!   and checks what is in it (the import direction between a crate's
//!   top-level modules).
//! - **Manifests.** Crates inherit the shared `[workspace.package]` keys and the
//!   workspace lints, except that a `tier-kind = "evolving"` crate sets its own
//!   `0.N` version (ADR-0081 §3, ADR-0088 §4); examples and tools are `publish = false`.
//! - **Train guard** (ADR-0088 §5). `flui-foundation` declares
//!   `links = "flui_train"`, and no other member does.
//! - **Unreachable tests.** Under `autotests = false` a new `tests/*.rs` file is
//!   silently never compiled unless a `[[test]]` target declares or mounts it;
//!   that went unnoticed for eleven days across five crates once.
//! - **ADR numbers** are unique, because code and docs cite them.
//!
//! `--self-test` runs the tier rule and the train guard over a built-in graph
//! with planted violations and fails unless it reports exactly those
//! (ADR-0078 §4).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::{Context, bail};
use cargo_metadata::{DependencyKind, Metadata};
use regex::Regex;
use serde_json::Value as Json;
use toml::{Table, Value as Toml};

use crate::util;

mod reach;
mod tiers;

pub(crate) use reach::{ReachArgs, reach};

/// Arguments for `cargo xtask workspace`.
#[derive(Debug, clap::Args)]
pub(crate) struct WorkspaceArgs {
    /// Run the tier rule over a built-in graph with planted violations instead
    /// of the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
}

/// `cargo xtask workspace`: check tiers, layers, manifests, test reachability
/// and ADR numbers. Exit code 1 lists every finding.
pub(crate) fn workspace(args: &WorkspaceArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(tiers::self_test());
    }
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let (findings, summary) = check(&root, &metadata)?;
    if findings.is_empty() {
        println!("workspace: {summary}");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("workspace: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// Runs every check; returns the findings and, when there are none, a summary.
fn check(root: &Path, metadata: &Metadata) -> anyhow::Result<(Vec<String>, String)> {
    let members = Members::load(root, metadata)?;
    let mut findings = Vec::new();
    let tier_names = tiers::names(&metadata.workspace_metadata)?;
    findings.extend(
        tiers::check_tiers(&members, &tier_names)
            .iter()
            .chain(&tiers::check_train_guard(&members))
            .map(ToString::to_string),
    );
    tiers::check_adr_citations(root, &members, &mut findings);
    let (layers, edges) = check_layers(&members, &metadata.workspace_metadata, &mut findings)?;
    check_manifests(root, &members, &mut findings)?;
    check_unique_adr_numbers(root, &mut findings)?;
    Ok((
        findings,
        format!(
            "{} crates in {} tiers and {layers} layers, {edges} dependency edges checked",
            members
                .iter()
                .filter(|member| member.tier.is_some())
                .count(),
            tier_names.len(),
        ),
    ))
}

/// The keys a member's `[package.metadata.flui]` may set.
const FLUI_KEYS: [&str; 12] = [
    "tier",
    "tier-kind",
    "order",
    "edge-exceptions",
    "reach-forbid",
    "reach-exceptions",
    "layer",
    "allowed-dependents",
    "allowed-dev-dependents",
    "wasm",
    "globals",
    "modules",
];

/// A workspace package as the checks see it, before its
/// `[package.metadata.flui]` is read: what `cargo metadata` reports, or what
/// the self-test builds without it.
struct Node {
    name: String,
    /// Manifest path relative to the repository root, `/`-separated.
    rel: String,
    /// `[package.metadata.flui]`, `null` when absent.
    flui: Json,
    /// `[package] links`, when declared.
    links: Option<String>,
    deps: Vec<Dep>,
}

/// One dependency of a [`Node`].
struct Dep {
    name: String,
    kind: DependencyKind,
    /// A `path` dependency: only in-repository edges are architecture; a
    /// registry crate that happens to share a member's name is not a member.
    in_repo: bool,
}

/// A dependent's permission for one normal or build edge the tier rule
/// refuses, until the ADR in `exit` removes the edge.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EdgeException {
    to: String,
    exit: String,
    reason: String,
}

/// What admits a `reach-exceptions` entry: the ADR whose change removes the
/// path, or the ADR that permits it for good.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Warrant {
    /// Debt: the named ADR removes the path.
    Exit(String),
    /// A standing permission the named ADR gives.
    Grant(String),
}

impl Warrant {
    /// The ADR number, whichever kind.
    pub(super) fn adr(&self) -> &str {
        match self {
            Self::Exit(adr) | Self::Grant(adr) => adr,
        }
    }

    /// The manifest key: `exit` or `grant`.
    pub(super) fn key(&self) -> &'static str {
        match self {
            Self::Exit(_) => "exit",
            Self::Grant(_) => "grant",
        }
    }
}

/// A dependent's permission for every path that runs through it and then
/// enters a package named `to`, which `cargo xtask reach` would otherwise
/// report (ADR-0081 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReachException {
    pub(super) to: String,
    pub(super) warrant: Warrant,
    pub(super) reason: String,
}

/// A workspace member, with what the checks read from it.
struct Member {
    name: String,
    /// Manifest path relative to the repository root, `/`-separated.
    rel: String,
    /// `[package] links`, when declared: only the train guard sets it.
    links: Option<String>,
    deps: Vec<Dep>,
    /// `[package.metadata.flui] tier`, when declared.
    tier: Option<String>,
    /// `[package.metadata.flui] tier-kind`, when declared.
    tier_kind: Option<String>,
    /// `[package.metadata.flui] order`, when declared.
    order: Option<u64>,
    /// `[package.metadata.flui] edge-exceptions`.
    edge_exceptions: Vec<EdgeException>,
    /// `[package.metadata.flui] reach-forbid`: names or `*` globs added to
    /// the tier's forbidden set.
    reach_forbid: Vec<String>,
    /// `[package.metadata.flui] reach-exceptions`.
    reach_exceptions: Vec<ReachException>,
    /// `[package.metadata.flui] layer`, when declared.
    layer: Option<usize>,
    /// `[package.metadata.flui] allowed-dependents`: the crates allowed a
    /// normal or build dependency on this one, when declared.
    allowed_dependents: Option<BTreeSet<String>>,
    /// `[package.metadata.flui] allowed-dev-dependents`: the crates allowed a
    /// dev-dependency on this one, when declared.
    allowed_dev_dependents: Option<BTreeSet<String>>,
}

impl Member {
    fn name(&self) -> &str {
        &self.name
    }

    /// Crates under `crates/` or `packages/` and the root facade carry a
    /// layer, a tier and an order.
    fn must_have_layer(&self) -> bool {
        self.rel == "Cargo.toml"
            || self.rel.starts_with("crates/")
            || self.rel.starts_with("packages/")
    }

    fn is_example_or_tool(&self) -> bool {
        self.rel.starts_with("examples/") || self.rel.starts_with("tools/")
    }

    /// The in-repository dependencies.
    fn repo_deps(&self) -> impl Iterator<Item = &Dep> {
        self.deps.iter().filter(|dep| dep.in_repo)
    }
}

struct Members(Vec<Member>);

impl Members {
    fn load(root: &Path, metadata: &Metadata) -> anyhow::Result<Self> {
        let nodes = metadata
            .workspace_packages()
            .into_iter()
            .map(|package| {
                Ok(Node {
                    name: package.name.to_string(),
                    rel: relative(root, package.manifest_path.as_std_path())?,
                    flui: package.metadata["flui"].clone(),
                    links: package.links.clone(),
                    deps: package
                        .dependencies
                        .iter()
                        .map(|dependency| Dep {
                            name: dependency.name.clone(),
                            kind: dependency.kind,
                            in_repo: dependency.path.is_some(),
                        })
                        .collect(),
                })
            })
            .collect::<anyhow::Result<_>>()?;
        Self::from_nodes(nodes)
    }

    /// Reads each node's `[package.metadata.flui]`. A key the checks do not
    /// know, or a value of the wrong type, is an error.
    fn from_nodes(nodes: Vec<Node>) -> anyhow::Result<Self> {
        let mut members = Vec::new();
        for Node {
            name,
            rel,
            flui,
            links,
            deps,
        } in nodes
        {
            if let Some(key) = flui
                .as_object()
                .and_then(|table| table.keys().find(|key| !FLUI_KEYS.contains(&key.as_str())))
            {
                bail!(
                    "{rel}: unknown `[package.metadata.flui]` key `{key}` (known: {})",
                    FLUI_KEYS.join(", ")
                );
            }
            if !(flui["wasm"].is_null() || flui["wasm"].is_boolean()) {
                bail!("{rel}: `wasm` must be `true` or `false`");
            }
            // Its keys are `cargo xtask module-dag`'s to read and check.
            if !(flui["modules"].is_null() || flui["modules"].is_object()) {
                bail!("{rel}: `modules` must be a table");
            }
            let layer = match &flui["layer"] {
                Json::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .and_then(|layer| usize::try_from(layer).ok())
                        .with_context(|| {
                            format!("{rel}: `layer` must be a non-negative integer")
                        })?,
                ),
            };
            let order =
                match &flui["order"] {
                    Json::Null => None,
                    value => Some(value.as_u64().with_context(|| {
                        format!("{rel}: `order` must be a non-negative integer")
                    })?),
                };
            let allowed_dependents = package_names(&flui, "allowed-dependents", &rel)?;
            let allowed_dev_dependents = package_names(&flui, "allowed-dev-dependents", &rel)?;
            members.push(Member {
                tier: string(&flui, "tier", &rel)?,
                tier_kind: string(&flui, "tier-kind", &rel)?,
                order,
                edge_exceptions: edge_exceptions(&flui, &rel)?,
                reach_forbid: package_names(&flui, "reach-forbid", &rel)?
                    .map(|names| names.into_iter().collect())
                    .unwrap_or_default(),
                reach_exceptions: reach_exceptions(&flui, &rel)?,
                layer,
                allowed_dependents,
                allowed_dev_dependents,
                name,
                rel,
                links,
                deps,
            });
        }
        members.sort_by(|a, b| a.rel.cmp(&b.rel));
        Ok(Self(members))
    }

    fn iter(&self) -> impl Iterator<Item = &Member> {
        self.0.iter()
    }

    fn by_name(&self) -> HashMap<&str, &Member> {
        self.0
            .iter()
            .map(|member| (member.name(), member))
            .collect()
    }
}

/// The string under `key`, when the manifest declares one.
fn string(flui: &Json, key: &str, rel: &str) -> anyhow::Result<Option<String>> {
    match &flui[key] {
        Json::Null => Ok(None),
        value => value
            .as_str()
            .map(|text| Some(text.to_owned()))
            .with_context(|| format!("{rel}: `{key}` must be a string")),
    }
}

/// The list of package names under `key`, when the manifest declares one.
fn package_names(flui: &Json, key: &str, rel: &str) -> anyhow::Result<Option<BTreeSet<String>>> {
    match &flui[key] {
        Json::Null => Ok(None),
        value => value
            .as_array()
            .and_then(|names| {
                names
                    .iter()
                    .map(|name| name.as_str().map(str::to_owned))
                    .collect::<Option<BTreeSet<_>>>()
            })
            .map(Some)
            .with_context(|| format!("{rel}: `{key}` must be a list of package names")),
    }
}

/// `edge-exceptions = [{ to = "…", exit = "ADR-NNNN", reason = "…" }, …]`.
fn edge_exceptions(flui: &Json, rel: &str) -> anyhow::Result<Vec<EdgeException>> {
    let malformed = || {
        format!(
            "{rel}: `edge-exceptions` must be a list of \
             `{{ to = \"<package>\", exit = \"ADR-NNNN\", reason = \"<text>\" }}`"
        )
    };
    match &flui["edge-exceptions"] {
        Json::Null => Ok(Vec::new()),
        value => value
            .as_array()
            .with_context(malformed)?
            .iter()
            .map(|entry| {
                let table = entry.as_object().with_context(malformed)?;
                if table.len() != 3 {
                    bail!(malformed());
                }
                let field = |key: &str| {
                    table
                        .get(key)
                        .and_then(Json::as_str)
                        .map(str::to_owned)
                        .with_context(malformed)
                };
                Ok(EdgeException {
                    to: field("to")?,
                    exit: field("exit")?,
                    reason: field("reason")?,
                })
            })
            .collect(),
    }
}

/// `reach-exceptions = [{ to = "…", exit | grant = "ADR-NNNN", reason = "…" }, …]`,
/// with exactly one of `exit` and `grant`, and no `to` named twice.
fn reach_exceptions(flui: &Json, rel: &str) -> anyhow::Result<Vec<ReachException>> {
    let malformed = || {
        format!(
            "{rel}: `reach-exceptions` must be a list of `{{ to = \"<package>\", exit = \
             \"ADR-NNNN\", reason = \"<text>\" }}`, with `grant` in place of `exit` for a \
             standing permission"
        )
    };
    let entries = match &flui["reach-exceptions"] {
        Json::Null => return Ok(Vec::new()),
        value => value.as_array().with_context(malformed)?,
    };
    let mut out: Vec<ReachException> = Vec::new();
    for entry in entries {
        let table = entry.as_object().with_context(malformed)?;
        if table.len() != 3 {
            bail!(malformed());
        }
        let field = |key: &str| table.get(key).and_then(Json::as_str).map(str::to_owned);
        let warrant = match (field("exit"), field("grant")) {
            (Some(adr), None) => Warrant::Exit(adr),
            (None, Some(adr)) => Warrant::Grant(adr),
            _ => bail!(malformed()),
        };
        let to = field("to").with_context(malformed)?;
        if out.iter().any(|seen| seen.to == to) {
            bail!("{rel}: `reach-exceptions` names {to} twice");
        }
        out.push(ReachException {
            to,
            warrant,
            reason: field("reason").with_context(malformed)?,
        });
    }
    Ok(out)
}

/// Layer direction and allowed dependents. Returns the number of named layers
/// and of in-workspace edges examined.
fn check_layers(
    members: &Members,
    workspace_metadata: &Json,
    findings: &mut Vec<String>,
) -> anyhow::Result<(usize, usize)> {
    let names: Vec<&str> = workspace_metadata["flui"]["layers"]
        .as_array()
        .context("Cargo.toml needs `[workspace.metadata.flui] layers = [...]`")?
        .iter()
        .map(|name| name.as_str().context("layer names must be strings"))
        .collect::<anyhow::Result<_>>()?;
    let describe = |layer: usize| {
        names.get(layer).map_or_else(
            || format!("layer {layer}"),
            |name| format!("layer {layer} ({name})"),
        )
    };

    for member in members.iter() {
        match member.layer {
            None if member.must_have_layer() => findings.push(format!(
                "{} has no `[package.metadata.flui] layer`; every crate under crates/ or \
                 packages/ declares one",
                member.rel
            )),
            Some(layer) if layer >= names.len() => findings.push(format!(
                "{} declares layer {layer}, but the root manifest names only {} layers",
                member.rel,
                names.len()
            )),
            _ => {}
        }
    }

    let by_name = members.by_name();
    let mut edges = 0;
    for member in members.iter() {
        for dependency in member.repo_deps() {
            let Some(target) = by_name.get(dependency.name.as_str()) else {
                continue;
            };
            edges += 1;

            let dev = dependency.kind == DependencyKind::Development;
            let (allowed, key, edge) = if dev {
                (
                    &target.allowed_dev_dependents,
                    "allowed-dev-dependents",
                    "has a dev-dependency on",
                )
            } else {
                (
                    &target.allowed_dependents,
                    "allowed-dependents",
                    "depends on",
                )
            };
            if let Some(allowed) = allowed
                && !member.is_example_or_tool()
                && !allowed.contains(member.name())
            {
                findings.push(format!(
                    "{} {edge} {}, which allows only {} (its `{key}`)",
                    member.name(),
                    target.name(),
                    allowed.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if dev {
                continue;
            }

            let Some(from) = member.layer else { continue };
            match target.layer {
                Some(to) if to > from => findings.push(format!(
                    "{} ({}) depends on {} ({}): dependencies point to the same layer or lower",
                    member.name(),
                    describe(from),
                    target.name(),
                    describe(to)
                )),
                None if target.is_example_or_tool() => findings.push(format!(
                    "{} depends on {}, an example or tool; nothing layered depends on those",
                    member.name(),
                    target.name()
                )),
                _ => {}
            }
        }
    }
    Ok((names.len(), edges))
}

/// The `[workspace.package]` keys every crate inherits; examples and tools
/// inherit the first four.
const INHERITED: [&str; 6] = [
    "version",
    "edition",
    "rust-version",
    "license",
    "authors",
    "repository",
];

/// Inherited package metadata, workspace lints, private examples and tools, and
/// test reachability under `autotests = false`.
fn check_manifests(
    root: &Path,
    members: &Members,
    findings: &mut Vec<String>,
) -> anyhow::Result<()> {
    for member in members.iter() {
        // The root manifest is the workspace itself; its package inherits
        // nothing from a table it defines.
        if member.rel == "Cargo.toml" {
            continue;
        }
        let manifest_path = root.join(&member.rel);
        let text = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", member.rel))?;
        let manifest: Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", member.rel))?;
        let package = manifest
            .get("package")
            .and_then(Toml::as_table)
            .with_context(|| format!("{} has no [package] table", member.rel))?;

        let inherited = if member.is_example_or_tool() {
            if package.get("publish") != Some(&Toml::Boolean(false)) {
                findings.push(format!("{} must set `publish = false`", member.rel));
            }
            &INHERITED[..4]
        } else {
            &INHERITED[..]
        };
        let evolving = member.tier_kind.as_deref() == Some(EVOLVING);
        if evolving {
            check_evolving_version(&member.rel, package.get("version"), findings);
        }
        for key in inherited {
            if evolving && *key == "version" {
                continue;
            }
            if !inherits_workspace(package.get(*key)) {
                findings.push(format!(
                    "{} must inherit `{key}.workspace = true`",
                    member.rel
                ));
            }
        }
        // A missing `[lints]` table opts the crate out of the workspace lints
        // silently, and Cargo forbids mixing `workspace = true` with local keys.
        if !inherits_workspace(manifest.get("lints")) {
            findings.push(format!(
                "{} must set `[lints] workspace = true`",
                member.rel
            ));
        }

        if package.get("autotests") == Some(&Toml::Boolean(false)) {
            let crate_dir = manifest_path
                .parent()
                .expect("BUG: a manifest path has a parent directory");
            check_test_reachability(root, member.name(), crate_dir, &manifest, findings)?;
        }
    }
    Ok(())
}

/// The `tier-kind` of a crate that versions apart from the train.
const EVOLVING: &str = "evolving";

/// An evolving crate carries its own `0.N` version, bumped on every train
/// (ADR-0081 §3, ADR-0088 §4): inheriting the workspace version would tie its semver to
/// the Stable facade's, and a major above 0 would promise what an evolving
/// surface does not.
fn check_evolving_version(rel: &str, version: Option<&Toml>, findings: &mut Vec<String>) {
    match version {
        Some(Toml::String(version)) => {
            if version.split('.').next() != Some("0") {
                findings.push(format!(
                    "{rel} is evolving: its version is `0.N` (ADR-0081 §3, ADR-0088 §4), not `{version}`"
                ));
            }
        }
        _ => findings.push(format!(
            "{rel} is evolving: it sets its own `version = \"0.N…\"` instead of inheriting the \
             workspace version (ADR-0081 §3, ADR-0088 §4)"
        )),
    }
}

fn inherits_workspace(value: Option<&Toml>) -> bool {
    value.and_then(Toml::as_table).is_some_and(|table| {
        table.len() == 1 && table.get("workspace") == Some(&Toml::Boolean(true))
    })
}

static PATH_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"#\[path\s*=\s*"(?P<path>[^"]+)"\]\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;"#)
        .expect("BUG: static regex is valid")
});

static BARE_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;").expect("BUG: static regex is valid")
});

/// Every top-level `tests/*.rs` file is its own `[[test]]` target or is mounted
/// (`mod` / `#[path] mod`) from one. A `tests/main.rs` that no `[[test]]`
/// declares is itself a finding: Cargo never compiles it, so trusting its `mod`
/// lines would call its siblings reachable while none of them run.
fn check_test_reachability(
    root: &Path,
    name: &str,
    crate_dir: &Path,
    manifest: &Table,
    findings: &mut Vec<String>,
) -> anyhow::Result<()> {
    let tests_dir = crate_dir.join("tests");
    if !tests_dir.is_dir() {
        return Ok(());
    }
    let declared: BTreeSet<PathBuf> = manifest
        .get("test")
        .and_then(Toml::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("path").and_then(Toml::as_str))
        .map(|path| normalize(&crate_dir.join(path)))
        .collect();

    let main_rs = normalize(&tests_dir.join("main.rs"));
    if main_rs.is_file() && !declared.contains(&main_rs) {
        findings.push(format!(
            "{} is not a declared `[[test]]` target: `{name}` sets `autotests = false`, so it \
             never compiles and nothing it mounts runs",
            relative(root, &main_rs)?
        ));
    }

    let mut reachable = BTreeSet::new();
    for source in declared.iter().filter(|path| path.is_file()) {
        let text = std::fs::read_to_string(source)
            .with_context(|| format!("reading {}", source.display()))?;
        let dir = source
            .parent()
            .expect("BUG: a file path has a parent directory");
        let mut pathed = BTreeSet::new();
        for found in PATH_MOD.captures_iter(&text) {
            pathed.insert(found["name"].to_owned());
            reachable.insert(normalize(&dir.join(&found["path"])));
        }
        // A bare `mod foo;` resolves next to the mounting file, which is
        // `tests/` only when that file lives there.
        if normalize(dir) == normalize(&tests_dir) {
            for found in BARE_MOD.captures_iter(&text) {
                if !pathed.contains(&found["name"]) {
                    reachable.insert(normalize(&tests_dir.join(format!("{}.rs", &found["name"]))));
                }
            }
        }
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&tests_dir)
        .with_context(|| format!("listing {}", tests_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
                && path.file_name().is_some_and(|file| file != "main.rs")
        })
        .map(|path| normalize(&path))
        .collect();
    entries.sort();
    for file in entries {
        if !declared.contains(&file) && !reachable.contains(&file) {
            findings.push(format!(
                "{} never runs: `{name}` sets `autotests = false`; give it a `[[test]]` target \
                 or mount it from one",
                relative(root, &file)?
            ));
        }
    }
    Ok(())
}

/// Two ADR files may not share a number.
fn check_unique_adr_numbers(root: &Path, findings: &mut Vec<String>) -> anyhow::Result<()> {
    let dir = root.join("docs").join("adr");
    let mut by_number: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
        let file = entry?.file_name().to_string_lossy().into_owned();
        if let Some(number) = file
            .strip_prefix("ADR-")
            .and_then(|rest| rest.get(..4))
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
        {
            by_number.entry(number.to_owned()).or_default().push(file);
        }
    }
    for (number, mut files) in by_number {
        if files.len() > 1 {
            files.sort();
            findings.push(format!("ADR-{number} is used by {}", files.join(", ")));
        }
    }
    Ok(())
}

/// `path` relative to `root`, `/`-separated.
///
/// Cargo reports canonical manifest paths, so a `root` reached through a
/// symlink (macOS's `/var` -> `/private/var`, a checkout under a linked
/// directory) spells the same directory differently; when the lexical prefix
/// does not match, both sides are compared canonicalized.
pub(crate) fn relative(root: &Path, path: &Path) -> anyhow::Result<String> {
    let path = normalize(path);
    let rel = match path.strip_prefix(normalize(root)) {
        Ok(rel) => rel.to_path_buf(),
        Err(lexical) => match (path.canonicalize(), root.canonicalize()) {
            (Ok(canonical), Ok(canonical_root)) => canonical
                .strip_prefix(&canonical_root)
                .map(Path::to_path_buf)
                .with_context(|| format!("{} is outside the repository", path.display()))?,
            _ => {
                return Err(anyhow::Error::new(lexical)
                    .context(format!("{} is outside the repository", path.display())));
            }
        },
    };
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Lexically resolves `.` and `..`, so paths from manifests and `#[path]`
/// attributes compare equal to directory listings on every host.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests;
