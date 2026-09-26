//! The tier rule (ADR-0081 §1) and the `tier-kind` declarations (§3).
//!
//! The rule itself is pure over [`Members`], so `--self-test` runs it on a
//! built-in graph without cargo or a disk; only the exit citations of
//! `edge-exceptions` read `docs/adr`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::Context;
use cargo_metadata::DependencyKind;
use regex::Regex;
use serde_json::{Value as Json, json};

use super::{Dep, Member, Members, Node};

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

/// The keys a crate with a tier declares.
const TIER_KEYS: [&str; 3] = ["tier", "tier-kind", "order"];

static ADR_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ADR-\d{4}$").expect("BUG: static regex is valid"));

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
    /// An `edge-exceptions` entry for an edge the rule admits or that does
    /// not exist.
    Stale { from: String, to: String },
    /// An `edge-exceptions` exit that is not an ADR number.
    BadExit {
        from: String,
        to: String,
        exit: String,
    },
}

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
            Self::Stale { from, to } => (from.clone(), to.clone(), "stale exception"),
            Self::BadExit { from, to, .. } => (from.clone(), to.clone(), "bad exit"),
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
            Self::Stale { from, to } => write!(
                f,
                "{from} lists `edge-exceptions` for {to}, but has no normal or build \
                 dependency on it that the tier rule refuses; remove the entry"
            ),
            Self::BadExit { from, to, exit } => write!(
                f,
                "{from}'s `edge-exceptions` entry for {to} names exit \"{exit}\", which is not \
                 an `ADR-NNNN` number"
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
            if !ADR_NUMBER.is_match(&entry.exit) {
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

/// Each `edge-exceptions` exit names an ADR that exists under `docs/adr`.
pub(super) fn check_exception_citations(
    root: &Path,
    members: &Members,
    findings: &mut Vec<String>,
) {
    let files: Vec<String> = std::fs::read_dir(root.join("docs").join("adr"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    for member in members.iter() {
        for entry in &member.edge_exceptions {
            if !ADR_NUMBER.is_match(&entry.exit) {
                continue; // reported by `check_tiers`
            }
            let prefix = format!("{}-", entry.exit);
            let exact = format!("{}.md", entry.exit);
            if !files
                .iter()
                .any(|file| file.starts_with(&prefix) || *file == exact)
            {
                findings.push(format!(
                    "{}'s `edge-exceptions` entry for {} names {}, which has no file under \
                     docs/adr",
                    member.name, entry.to, entry.exit
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

    let nodes = vec![
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
        // edge, which the exception does not name
        node(&h_pkg_rel, &h_pkg, &[("pkg1", Normal), ("pkg2", Normal)]),
        // planted: H -> pkg without one
        crate_node("h2", "H", 3, &[("pkg1", Normal)]),
        node(&pkg_rel, &pkg, &[("h1", Normal)]),
        node(&pkg2_rel, &pkg2, &[]),
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
const EXPECTED: [(&str, &str, &str); 8] = [
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
    let seen: BTreeSet<Identity> = check_tiers(&self_test_members(), &tiers)
        .iter()
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
