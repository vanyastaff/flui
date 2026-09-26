//! The tier rule (ADR-0081 §1), the `tier-kind` declarations and the kind
//! rule (§3, with ADR-0088 §2), and the train guard (ADR-0088 §5).
//!
//! The kind rule: only applications name an official package, in any
//! dependency kind, and an official package names another only through a
//! declared `edge-exceptions` entry; an official package's normal and build
//! dependencies are [`SDK_SURFACE`]; a member under `packages/` is an
//! official package with no `edge-exceptions`. A refused edge is admitted by
//! the dependent's `edge-exceptions`, as for the tier rule, and an entry that
//! admits nothing either rule refuses is stale.
//!
//! The rule itself is pure over [`Members`], so `--self-test` runs it on a
//! built-in graph without cargo or a disk; only the exit citations of
//! `edge-exceptions` read `docs/adr`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use cargo_metadata::DependencyKind;
use serde_json::{Value as Json, json};

use super::{Dep, Member, Members, Node};
use crate::util;

/// The root manifest's `[workspace.metadata.flui] tiers`, bottom to top.
pub(super) fn names(workspace_metadata: &Json) -> anyhow::Result<Vec<String>> {
    workspace_metadata["flui"]["tiers"]
        .as_array()
        .context("Cargo.toml needs `[workspace.metadata.flui] tiers = [...]`")?
        .iter()
        .map(|name| {
            name.as_str()
                .map(str::to_owned)
                .context("tier names must be strings")
        })
        .collect()
}

/// What a crate promises (ADR-0081 §3); the set is closed.
const KINDS: [&str; 5] = ["stable", "evolving", "internal", "official", "tool"];

/// The kind of an application: an example, a tool or `flui-cli`.
const TOOL: &str = "tool";

/// The kind of an official package (ADR-0088).
const OFFICIAL: &str = "official";

/// What an official package may name in a normal or build dependency: the
/// package-author surface and the contract crates (ADR-0081 §3, ADR-0088 §2).
const SDK_SURFACE: [&str; 3] = ["flui-sdk", "flui-platform-api", "flui-protocol"];

/// The keys a crate with a tier declares.
const TIER_KEYS: [&str; 3] = ["tier", "tier-kind", "order"];

/// One violation of the tier rule or its declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Finding {
    /// A required key is absent.
    Missing {
        name: String,
        rel: String,
        key: &'static str,
    },
    /// An example or tool declares `tier` or `order`.
    NotForTools {
        name: String,
        rel: String,
        key: &'static str,
    },
    /// An example or tool declares a kind other than `tool`.
    NotToolKind {
        name: String,
        rel: String,
        kind: String,
    },
    /// `tier` is not one of the root `tiers`.
    UnknownTier {
        name: String,
        rel: String,
        tier: String,
        known: Vec<String>,
    },
    /// `tier-kind` is not one of [`KINDS`].
    UnknownKind {
        name: String,
        rel: String,
        kind: String,
    },
    /// Two or more crates share an order in one tier.
    DuplicateOrder {
        tier: String,
        order: u64,
        names: Vec<String>,
    },
    /// A normal or build edge to a higher tier, or to a larger or equal order
    /// in the same tier, that no `edge-exceptions` entry admits.
    Upward { from: Position, to: Position },
    /// A crate with a tier depends on an application.
    OnTool { from: String, to: String },
    /// A crate that is neither an official package nor an application names
    /// an official package, in a dependency of any kind, and no
    /// `edge-exceptions` entry admits it (ADR-0081 §3).
    NamesOfficial { from: String, to: String },
    /// An official package has a normal or build dependency outside
    /// [`SDK_SURFACE`] that no `edge-exceptions` entry admits (ADR-0088 §2).
    OffSdk { from: String, to: String },
    /// An official package depends on another official package, in a
    /// dependency of any kind, and no `edge-exceptions` entry declares it
    /// (ADR-0081 §3, ADR-0028).
    OfficialOnOfficial { from: String, to: String },
    /// A member under `packages/` whose kind is not `official`.
    PackageNotOfficial {
        name: String,
        rel: String,
        kind: String,
    },
    /// A member under `packages/` lists `edge-exceptions`: a package moves
    /// there only once it builds on the SDK alone.
    PackageExcepted { name: String, rel: String },
    /// An `edge-exceptions` entry for an edge the rule admits or that does
    /// not exist.
    Stale { from: String, to: String },
    /// An `edge-exceptions` exit that is not an ADR number.
    BadExit {
        from: String,
        to: String,
        exit: String,
    },
    /// [`TRAIN_GUARD`] does not declare `links = "flui_train"`.
    NoTrainGuard { name: String, rel: String },
    /// A member other than [`TRAIN_GUARD`] declares `links = "flui_train"`.
    SecondTrainGuard { name: String, rel: String },
}

/// The crate that carries the train guard (ADR-0088 §5): every train depends
/// on it, the facade and `flui-sdk` included.
pub(super) const TRAIN_GUARD: &str = "flui-foundation";

/// The `links` value of the train guard.
pub(super) const TRAIN_LINKS: &str = "flui_train";

/// A crate and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Position {
    name: String,
    tier: String,
    order: u64,
}

impl Finding {
    /// `(subject, object, kind)`: what the self-test compares.
    fn identity(&self) -> (String, String, &'static str) {
        match self {
            Self::Missing { name, key, .. } => (name.clone(), (*key).to_owned(), "missing"),
            Self::NotForTools { name, key, .. } => {
                (name.clone(), (*key).to_owned(), "not for tools")
            }
            Self::NotToolKind { name, kind, .. } => (name.clone(), kind.clone(), "not tool kind"),
            Self::UnknownTier { name, tier, .. } => (name.clone(), tier.clone(), "unknown tier"),
            Self::UnknownKind { name, kind, .. } => (name.clone(), kind.clone(), "unknown kind"),
            Self::DuplicateOrder { names, .. } => (
                names.first().cloned().unwrap_or_default(),
                names[1..].join(","),
                "duplicate order",
            ),
            Self::Upward { from, to } => (from.name.clone(), to.name.clone(), "upward"),
            Self::OnTool { from, to } => (from.clone(), to.clone(), "on tool"),
            Self::NamesOfficial { from, to } => (from.clone(), to.clone(), "names official"),
            Self::OffSdk { from, to } => (from.clone(), to.clone(), "off sdk"),
            Self::OfficialOnOfficial { from, to } => {
                (from.clone(), to.clone(), "official on official")
            }
            Self::PackageNotOfficial { name, kind, .. } => {
                (name.clone(), kind.clone(), "package not official")
            }
            Self::PackageExcepted { name, .. } => (
                name.clone(),
                "edge-exceptions".to_owned(),
                "package excepted",
            ),
            Self::Stale { from, to } => (from.clone(), to.clone(), "stale exception"),
            Self::BadExit { from, to, .. } => (from.clone(), to.clone(), "bad exit"),
            Self::NoTrainGuard { name, .. } => (name.clone(), TRAIN_LINKS.to_owned(), "no guard"),
            Self::SecondTrainGuard { name, .. } => {
                (name.clone(), TRAIN_LINKS.to_owned(), "second guard")
            }
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { rel, key, .. } => write!(
                f,
                "{rel} has no `[package.metadata.flui] {key}`; crates declare `tier`, \
                 `tier-kind` and `order`, examples and tools `tier-kind = \"tool\"` (ADR-0081)"
            ),
            Self::NotForTools { rel, key, .. } => write!(
                f,
                "{rel} is an example or tool: it declares only `tier-kind = \"tool\"`, not \
                 `{key}`"
            ),
            Self::NotToolKind { rel, kind, .. } => write!(
                f,
                "{rel} is an example or tool: its `tier-kind` is \"{kind}\", not \"tool\""
            ),
            Self::UnknownTier {
                rel, tier, known, ..
            } => write!(
                f,
                "{rel} declares tier \"{tier}\", but the root manifest's `tiers` are {}",
                known.join(", ")
            ),
            Self::UnknownKind { rel, kind, .. } => write!(
                f,
                "{rel} declares tier-kind \"{kind}\"; the kinds are {}",
                KINDS.join(", ")
            ),
            Self::DuplicateOrder { tier, order, names } => write!(
                f,
                "{} share order {order} in tier {tier}; an order is unique within its tier",
                names.join(" and ")
            ),
            Self::Upward { from, to } => write!(
                f,
                "{} (tier {}, order {}) depends on {} (tier {}, order {}): a dependency points \
                 to a lower tier, or to a smaller order in the same tier",
                from.name, from.tier, from.order, to.name, to.tier, to.order
            ),
            Self::OnTool { from, to } => write!(
                f,
                "{from} depends on {to}, whose `tier-kind` is \"tool\"; nothing with a tier \
                 depends on an application"
            ),
            Self::NamesOfficial { from, to } => write!(
                f,
                "{from} depends on {to}, an official package: only applications and other \
                 official packages name one, in any dependency kind (ADR-0081 §3); list it in \
                 `edge-exceptions` with the ADR that removes the edge"
            ),
            Self::OffSdk { from, to } => write!(
                f,
                "{from} is an official package and depends on {to}: its normal and build \
                 dependencies are {} only (ADR-0088 §2); list it in `edge-exceptions` with the \
                 ADR that removes the edge",
                SDK_SURFACE.join(", ")
            ),
            Self::OfficialOnOfficial { from, to } => write!(
                f,
                "{from} and {to} are both official packages and {from} depends on {to}: an \
                 edge between official packages, in any dependency kind, is declared in the \
                 dependent's `edge-exceptions` with its ADR (ADR-0081 §3, ADR-0028)"
            ),
            Self::PackageNotOfficial { rel, kind, .. } => write!(
                f,
                "{rel} is under packages/, which holds official packages only, but its \
                 `tier-kind` is \"{kind}\" (ADR-0088)"
            ),
            Self::PackageExcepted { rel, .. } => write!(
                f,
                "{rel} is under packages/ and lists `edge-exceptions`: a package moves there \
                 once it builds on the SDK alone (ADR-0088)"
            ),
            Self::Stale { from, to } => write!(
                f,
                "{from} lists `edge-exceptions` for {to}, but has no normal or build \
                 dependency on it that the tier rule refuses, and no dependency on it that the \
                 kind rule refuses; remove the entry"
            ),
            Self::BadExit { from, to, exit } => write!(
                f,
                "{from}'s `edge-exceptions` entry for {to} names exit \"{exit}\", which is not \
                 an `ADR-NNNN` number"
            ),
            Self::NoTrainGuard { rel, .. } => write!(
                f,
                "{rel} must declare `links = \"{TRAIN_LINKS}\"` (with a build script): it is the \
                 train guard every FLUI train depends on (ADR-0088 §5)"
            ),
            Self::SecondTrainGuard { rel, .. } => write!(
                f,
                "{rel} declares `links = \"{TRAIN_LINKS}\"`, which only {TRAIN_GUARD} carries: \
                 the guard is the one crate every train shares (ADR-0088 §5)"
            ),
        }
    }
}

/// Every tier finding over `members`, with `tiers` the root's tier names.
pub(super) fn check_tiers(members: &Members, tiers: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let tier_index = |tier: &str| tiers.iter().position(|name| name == tier);

    for member in members.iter() {
        let (name, rel) = (member.name.clone(), member.rel.clone());
        if member.is_example_or_tool() {
            for (key, set) in [
                ("tier", member.tier.is_some()),
                ("order", member.order.is_some()),
            ] {
                if set {
                    findings.push(Finding::NotForTools {
                        name: name.clone(),
                        rel: rel.clone(),
                        key,
                    });
                }
            }
            match &member.tier_kind {
                None => findings.push(Finding::Missing {
                    name,
                    rel,
                    key: "tier-kind",
                }),
                Some(kind) if kind != TOOL => findings.push(Finding::NotToolKind {
                    name,
                    rel,
                    kind: kind.clone(),
                }),
                Some(_) => {}
            }
            continue;
        }
        let present = [
            member.tier.is_some(),
            member.tier_kind.is_some(),
            member.order.is_some(),
        ];
        for (key, present) in TIER_KEYS.into_iter().zip(present) {
            if !present && (key == "tier-kind" || member.must_have_layer()) {
                findings.push(Finding::Missing {
                    name: name.clone(),
                    rel: rel.clone(),
                    key,
                });
            }
        }
        if let Some(tier) = &member.tier
            && tier_index(tier).is_none()
        {
            findings.push(Finding::UnknownTier {
                name: name.clone(),
                rel: rel.clone(),
                tier: tier.clone(),
                known: tiers.to_vec(),
            });
        }
        if member.rel.starts_with("packages/") {
            if member.tier_kind.as_deref() != Some(OFFICIAL) {
                findings.push(Finding::PackageNotOfficial {
                    name: name.clone(),
                    rel: rel.clone(),
                    kind: member.tier_kind.clone().unwrap_or_default(),
                });
            }
            if !member.edge_exceptions.is_empty() {
                findings.push(Finding::PackageExcepted {
                    name: name.clone(),
                    rel: rel.clone(),
                });
            }
        }
        if let Some(kind) = &member.tier_kind
            && !KINDS.contains(&kind.as_str())
        {
            findings.push(Finding::UnknownKind {
                name,
                rel,
                kind: kind.clone(),
            });
        }
    }

    // Where each crate with a valid tier and an order sits.
    let position = |member: &Member| -> Option<(usize, u64)> {
        if member.is_example_or_tool() {
            return None;
        }
        Some((tier_index(member.tier.as_deref()?)?, member.order?))
    };

    let mut by_position: BTreeMap<(usize, u64), Vec<String>> = BTreeMap::new();
    for member in members.iter() {
        if let Some(at) = position(member) {
            by_position.entry(at).or_default().push(member.name.clone());
        }
    }
    for ((tier, order), names) in by_position {
        if names.len() > 1 {
            findings.push(Finding::DuplicateOrder {
                tier: tiers[tier].clone(),
                order,
                names,
            });
        }
    }

    // One entry per `(dependent, dependency)`: a crate may name a dependency
    // twice (a target-specific table beside the general one).
    let by_name = members.by_name();
    let edges: BTreeSet<(&str, &str)> = members
        .iter()
        .filter(|member| member.tier.is_some() && !member.is_example_or_tool())
        .flat_map(|member| {
            member
                .repo_deps()
                .filter(|dep| dep.kind != DependencyKind::Development)
                .filter(|dep| by_name.contains_key(dep.name.as_str()))
                .map(move |dep| (member.name(), dep.name.as_str()))
        })
        .collect();

    let describe = |member: &Member, (tier, order): (usize, u64)| Position {
        name: member.name.clone(),
        tier: tiers[tier].clone(),
        order,
    };
    // Every edge a rule of ADR-0081 refuses: the tier rule below and the kind
    // rule after it. An `edge-exceptions` entry for any other edge is stale.
    let mut refused: BTreeSet<(&str, &str)> = BTreeSet::new();
    for &(from_name, to_name) in &edges {
        let (from, to) = (by_name[from_name], by_name[to_name]);
        if to.tier_kind.as_deref() == Some(TOOL) {
            findings.push(Finding::OnTool {
                from: from_name.to_owned(),
                to: to_name.to_owned(),
            });
        }
        let (Some(from_at), Some(to_at)) = (position(from), position(to)) else {
            continue;
        };
        if to_at < from_at {
            continue;
        }
        refused.insert((from_name, to_name));
        if !from.edge_exceptions.iter().any(|entry| entry.to == to_name) {
            findings.push(Finding::Upward {
                from: describe(from, from_at),
                to: describe(to, to_at),
            });
        }
    }

    for (from_name, to_name, finding) in kind_refusals(members) {
        refused.insert((from_name, to_name));
        let from = by_name[from_name];
        if !from.edge_exceptions.iter().any(|entry| entry.to == to_name) {
            findings.push(finding);
        }
    }

    for member in members.iter() {
        let mut seen = BTreeSet::new();
        for entry in &member.edge_exceptions {
            let first = seen.insert(entry.to.as_str());
            if !first || !refused.contains(&(member.name(), entry.to.as_str())) {
                findings.push(Finding::Stale {
                    from: member.name.clone(),
                    to: entry.to.clone(),
                });
            }
            if !util::is_adr_number(&entry.exit) {
                findings.push(Finding::BadExit {
                    from: member.name.clone(),
                    to: entry.to.clone(),
                    exit: entry.exit.clone(),
                });
            }
        }
    }
    findings
}

/// The edges the kind rule refuses (ADR-0081 §3, ADR-0088 §2), each once,
/// with the finding it produces unless an `edge-exceptions` entry admits it:
///
/// - **names official**: a member that is neither `official` nor `tool`, nor
///   under `examples/` or `tools/`, depends on an `official` package in any
///   kind (normal, build or dev; an optional dependency is a normal one);
/// - **official on official**: an `official` package depends on another
///   `official` package in any kind, dev included (ADR-0028: the design
///   systems do not depend on each other in any form);
/// - **off the SDK**: an `official` package has a normal or build dependency
///   on a member outside [`SDK_SURFACE`].
fn kind_refusals(members: &Members) -> Vec<(&str, &str, Finding)> {
    let by_name = members.by_name();
    let mut refusals = BTreeMap::new();
    for member in members.iter() {
        if member.is_example_or_tool() || member.tier_kind.as_deref() == Some(TOOL) {
            continue;
        }
        let official = member.tier_kind.as_deref() == Some(OFFICIAL);
        for dep in member.repo_deps() {
            let Some(to) = by_name.get(dep.name.as_str()) else {
                continue;
            };
            let edge = (member.name(), to.name());
            let (from, target) = (edge.0.to_owned(), edge.1.to_owned());
            let to_official = to.tier_kind.as_deref() == Some(OFFICIAL);
            if official && to_official {
                refusals
                    .entry(edge)
                    .or_insert(Finding::OfficialOnOfficial { from, to: target });
            } else if official {
                if dep.kind != DependencyKind::Development && !SDK_SURFACE.contains(&to.name()) {
                    refusals
                        .entry(edge)
                        .or_insert(Finding::OffSdk { from, to: target });
                }
            } else if to_official {
                refusals
                    .entry(edge)
                    .or_insert(Finding::NamesOfficial { from, to: target });
            }
        }
    }
    refusals
        .into_iter()
        .map(|((from, to), finding)| (from, to, finding))
        .collect()
}

/// The train guard (ADR-0088 §5): [`TRAIN_GUARD`], when it is a member,
/// declares `links = "flui_train"`, and no other member does. A workspace
/// without the guard crate is not reported here; `cargo xtask reach` requires
/// it in the SDK's and the facade's builds.
pub(super) fn check_train_guard(members: &Members) -> Vec<Finding> {
    let mut findings = Vec::new();
    for member in members.iter() {
        let guards = member.links.as_deref() == Some(TRAIN_LINKS);
        let (name, rel) = (member.name.clone(), member.rel.clone());
        if member.name == TRAIN_GUARD && !guards {
            findings.push(Finding::NoTrainGuard { name, rel });
        } else if member.name != TRAIN_GUARD && guards {
            findings.push(Finding::SecondTrainGuard { name, rel });
        }
    }
    findings
}

/// Each `edge-exceptions` exit and each `reach-exceptions` exit or grant
/// names an ADR that exists under `docs/adr`; a reach warrant must also be an
/// `ADR-NNNN` number (an edge exit's format is `check_tiers`'s finding).
pub(super) fn check_adr_citations(root: &Path, members: &Members, findings: &mut Vec<String>) {
    let files = util::adr_files(root);
    for member in members.iter() {
        for entry in &member.edge_exceptions {
            if !util::is_adr_number(&entry.exit) {
                continue; // reported by `check_tiers`
            }
            if !util::adr_exists(&files, &entry.exit) {
                findings.push(format!(
                    "{}'s `edge-exceptions` entry for {} names {}, which has no file under \
                     docs/adr",
                    member.name, entry.to, entry.exit
                ));
            }
        }
        for entry in &member.reach_exceptions {
            let (key, adr) = (entry.warrant.key(), entry.warrant.adr());
            if !util::is_adr_number(adr) {
                findings.push(format!(
                    "{}'s `reach-exceptions` entry for {} names {key} \"{adr}\", which is not \
                     an `ADR-NNNN` number",
                    member.name, entry.to
                ));
            } else if !util::adr_exists(&files, adr) {
                findings.push(format!(
                    "{}'s `reach-exceptions` entry for {} names {adr}, which has no file under \
                     docs/adr",
                    member.name, entry.to
                ));
            }
        }
    }
}

/// A node of the self-test graph: `rel` decides crate or application.
fn node(rel: &str, flui: &Json, deps: &[(&str, DependencyKind)]) -> Node {
    let name = rel
        .rsplit('/')
        .nth(1)
        .expect("BUG: self-test paths are `<dir>/<name>/Cargo.toml`")
        .to_owned();
    Node {
        name,
        rel: rel.to_owned(),
        flui: flui.clone(),
        links: None,
        deps: deps
            .iter()
            .map(|&(name, kind)| Dep {
                name: name.to_owned(),
                kind,
                in_repo: true,
            })
            .collect(),
    }
}

/// A crate under `crates/` at `tier`/`order` of kind `kind`.
fn krate(name: &str, tier: &str, order: u64, kind: &str) -> (String, Json) {
    (
        format!("crates/{name}/Cargo.toml"),
        json!({ "tier": tier, "tier-kind": kind, "order": order }),
    )
}

/// The self-test graph: real tier names, silent edges of every legal shape,
/// and one planted violation per sub-rule.
fn self_test_members() -> Members {
    use DependencyKind::{Development as Dev, Normal};
    let crate_node = |name: &str, tier: &str, order: u64, deps: &[(&str, DependencyKind)]| {
        let (rel, flui) = krate(name, tier, order, "internal");
        node(&rel, &flui, deps)
    };
    let with = |(rel, mut flui): (String, Json), key: &str, value: Json| {
        flui[key] = value;
        (rel, flui)
    };
    let exception = |to: &str| json!([{ "to": to, "exit": "ADR-0081", "reason": "self-test" }]);
    let tool = json!({ "tier-kind": "tool" });

    let (h_pkg_rel, h_pkg) = with(
        krate("h-pkg", "H", 2, "internal"),
        "edge-exceptions",
        exception("pkg1"),
    );
    let (stale_rel, stale) = with(
        krate("s-stale", "S", 3, "internal"),
        "edge-exceptions",
        exception("v1"),
    );
    let (no_kind_rel, mut no_kind) = krate("v-nokind", "V", 3, "internal");
    no_kind
        .as_object_mut()
        .expect("BUG: a crate's flui table is an object")
        .remove("tier-kind");
    let (pkg_rel, pkg) = krate("pkg1", "pkg", 1, "official");
    let (pkg2_rel, pkg2) = krate("pkg2", "pkg", 2, "official");
    let (k_devx_rel, k_devx) = with(
        krate("k-devx", "K", 5, "internal"),
        "edge-exceptions",
        exception("pkg1"),
    );
    let (cli_rel, cli) = krate("h-cli", "H", 4, "tool");
    let (sdk_rel, sdk) = krate("flui-sdk", "K", 6, "evolving");
    let package = |name: &str, order: u64, kind: &str| {
        let (_, flui) = krate(name, "pkg", order, kind);
        (format!("packages/{name}/Cargo.toml"), flui)
    };
    let (pkg3_rel, pkg3) = package("pkg3", 3, "official");
    let (pkg_internal_rel, pkg_internal) = package("pkg-internal", 4, "internal");
    let (pkg_exc_rel, pkg_exc) = with(
        package("pkg-exc", 5, "official"),
        "edge-exceptions",
        exception("v1"),
    );
    let guard = |mut node: Node| {
        node.links = Some(TRAIN_LINKS.to_owned());
        node
    };

    let nodes = vec![
        // silent: the train guard where it belongs
        guard(crate_node(TRAIN_GUARD, "V", 4, &[])),
        // planted: a second crate with the guard's `links`
        guard(crate_node("v-guard", "V", 5, &[])),
        crate_node("v1", "V", 1, &[]),
        // planted: a second crate at V order 1
        crate_node("v-dup", "V", 1, &[]),
        // planted: a crate with a tier depends on a tool
        crate_node("v2", "V", 2, &[("t1", Normal)]),
        // planted: no `tier-kind`
        node(&no_kind_rel, &no_kind, &[]),
        crate_node("s1", "S", 1, &[("v1", Normal)]),
        // planted: S depends on H
        crate_node("s-up", "S", 2, &[("h1", Normal)]),
        // planted: an exception for an edge the rule admits
        node(&stale_rel, &stale, &[("v1", Normal)]),
        // planted: order 1 depends on order 2 in one tier
        crate_node("k-low", "K", 1, &[("k-high", Normal)]),
        crate_node("k-high", "K", 2, &[("s1", Normal)]),
        // silent: a dev edge may point up
        crate_node("k1", "K", 3, &[("h1", Dev), ("k-high", Normal)]),
        crate_node("h1", "H", 1, &[("k-high", Normal), ("k-high", Normal)]),
        // silent: H -> pkg with an exception; planted: its second H -> pkg
        // edge, which the exception does not name (upward, and names an
        // official package)
        node(&h_pkg_rel, &h_pkg, &[("pkg1", Normal), ("pkg2", Normal)]),
        // planted: H -> pkg without one (upward, and names an official package)
        crate_node("h2", "H", 3, &[("pkg1", Normal)]),
        // planted: a core crate's dev edge to an official package
        crate_node("k-dev", "K", 4, &[("pkg1", Dev)]),
        // silent: the same dev edge with an exception
        node(&k_devx_rel, &k_devx, &[("pkg1", Dev)]),
        // silent: a `tool` crate under crates/ may name an official package
        node(&cli_rel, &cli, &[("pkg1", Dev)]),
        node(&sdk_rel, &sdk, &[]),
        // planted: an official package's normal edge off the SDK
        node(&pkg_rel, &pkg, &[("h1", Normal)]),
        // silent: an official package's dev edge off the SDK; planted: its
        // dev edge to another official package
        node(&pkg2_rel, &pkg2, &[("h1", Dev), ("pkg1", Dev)]),
        // silent: a package on the SDK; planted: its edge off it
        node(&pkg3_rel, &pkg3, &[("flui-sdk", Normal), ("v1", Normal)]),
        // planted: a non-official member under packages/
        node(&pkg_internal_rel, &pkg_internal, &[]),
        // planted: a member under packages/ with an exception; the edge it
        // excuses stays silent
        node(&pkg_exc_rel, &pkg_exc, &[("v1", Normal)]),
        // silent: applications depend on anything
        node(
            "examples/ex/Cargo.toml",
            &tool,
            &[("h1", Normal), ("pkg1", Normal)],
        ),
        node("tools/t1/Cargo.toml", &tool, &[]),
    ];
    Members::from_nodes(nodes).expect("BUG: the self-test graph is well-typed")
}

/// The finding each planted violation must produce, and no other.
const EXPECTED: [(&str, &str, &str); 17] = [
    ("h2", "pkg1", "names official"),
    ("pkg2", "pkg1", "official on official"),
    ("h-pkg", "pkg2", "names official"),
    ("k-dev", "pkg1", "names official"),
    ("pkg1", "h1", "off sdk"),
    ("pkg3", "v1", "off sdk"),
    ("pkg-internal", "internal", "package not official"),
    ("pkg-exc", "edge-exceptions", "package excepted"),
    ("v-guard", TRAIN_LINKS, "second guard"),
    ("v-dup", "v1", "duplicate order"),
    ("v2", "t1", "on tool"),
    ("v-nokind", "tier-kind", "missing"),
    ("s-up", "h1", "upward"),
    ("s-stale", "v1", "stale exception"),
    ("k-low", "k-high", "upward"),
    ("h2", "pkg1", "upward"),
    ("h-pkg", "pkg2", "upward"),
];

type Identity = (String, String, &'static str);

/// `(missed, false positives)` of the tier rule over the self-test graph.
pub(super) fn self_test_diff() -> (Vec<Identity>, Vec<Identity>) {
    let tiers: Vec<String> = ["V", "C", "S", "R", "K", "H", "pkg"]
        .map(str::to_owned)
        .into();
    let members = self_test_members();
    let seen: BTreeSet<Identity> = check_tiers(&members, &tiers)
        .iter()
        .chain(&check_train_guard(&members))
        .map(Finding::identity)
        .collect();
    let expected: BTreeSet<Identity> = EXPECTED
        .iter()
        .map(|&(from, to, kind)| (from.to_owned(), to.to_owned(), kind))
        .collect();
    (
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    )
}

/// `cargo xtask workspace --self-test`.
pub(super) fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff();
    for (from, to, kind) in &missed {
        println!("self-test: MISSED ({from}, {to}, {kind})");
    }
    for (from, to, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({from}, {to}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "workspace: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}
