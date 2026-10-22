//! The feature axis of `cargo xtask module-dag`: a crate laid out by feature,
//! a directory per feature holding its model beside one file per platform
//! (ADR-0151 §10).
//!
//! Every module path belongs to one [`Node`], and the rules ([`rule`]) judge
//! the edges between nodes:
//!
//! - `hosts`, and everything under it, is one node that may name anything and
//!   that nothing else names;
//! - a `base` module and everything under it is one node that names no
//!   feature;
//! - a feature's *model* is the feature module and its submodules that no
//!   platform word names;
//! - a *platform node* is the child of a feature named by a platform word and
//!   everything under it, or, below a group (`apple`), a child the group lists
//!   (`macos`) and everything under it; the group module itself is a platform
//!   node of its own, what the children share;
//! - a `crate-level` module (the table, the platform trait, the error
//!   vocabulary) is unchecked as a source and free as a target.
//!
//! Every top-level module compiled outside tests has one of those roles or is
//! `transparent`; one that has none is a finding, so a directory the layout
//! does not name (a `shared/` or a `platforms/` coming back) cannot appear
//! unchecked. The platform words and the groups are vocabulary, not modules
//! the crate must have: a feature has a file only for the platforms it fills.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::{Context, bail};
use serde_json::{Map, Value as Json};

use super::source::{Depth, Reach, Scan};
use super::{
    Declaration, Finding, Outcome, admitted, check_exceptions, edges_between, names, relay_items,
};

mod planted;

pub(super) use planted::{EXPECTED, PLANTED};

/// The keys of the feature axis; `hosts` is required beside them.
pub(super) const KEYS: [&str; 6] = [
    "features",
    "platforms",
    "groups",
    "base",
    "hosts",
    "crate-level",
];

/// `features` and the keys beside it.
#[derive(Debug)]
pub(super) struct FeatureAxis {
    /// The top-level modules that are features.
    features: Vec<String>,
    /// The module names of the platform files, in std's vocabulary.
    platforms: Vec<String>,
    /// The platforms that open into children where code differs, with those
    /// children.
    groups: BTreeMap<String, Vec<String>>,
    /// The modules every feature builds on.
    base: Vec<String>,
    /// The module that picks and wires a host.
    hosts: String,
    /// The top-level modules that belong to no feature and are no base: what
    /// every part of the crate may name.
    crate_level: Vec<String>,
}

impl FeatureAxis {
    /// Reads the feature keys of the `modules` table; a shape the check
    /// cannot read is an error.
    pub(super) fn from_table(table: &Map<String, Json>, rel: &str) -> anyhow::Result<Self> {
        let list = |key: &str| match table.get(key) {
            None => Ok(Vec::new()),
            Some(value) => names(value, &format!("`{key}`"), rel),
        };
        let groups = match table.get("groups") {
            None => BTreeMap::new(),
            Some(value) => value
                .as_object()
                .with_context(|| {
                    format!("{rel}: `groups` must be a table of `platform = [\"child\", ...]`")
                })?
                .iter()
                .map(|(group, children)| {
                    Ok((
                        group.clone(),
                        names(
                            children,
                            &format!("the children of the group `{group}`"),
                            rel,
                        )?,
                    ))
                })
                .collect::<anyhow::Result<_>>()?,
        };
        let Some(hosts) = table.get("hosts") else {
            bail!("{rel}: the feature keys need `hosts`, the module that wires a host");
        };
        let hosts = hosts
            .as_str()
            .with_context(|| format!("{rel}: `hosts` must be a module name"))?
            .to_owned();
        Ok(Self {
            features: list("features")?,
            platforms: list("platforms")?,
            groups,
            base: list("base")?,
            hosts,
            crate_level: list("crate-level")?,
        })
    }
}

/// The part of the crate that an edge joins: a module and what belongs to it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    /// The hosts module.
    Hosts(String),
    /// A base module.
    Base(String),
    /// A top-level module of none of the roles.
    Crate(String),
    /// A feature's model.
    Model(String),
    /// A platform file of a feature, or the group module `platform` opens
    /// into children (`child` is `None`), or one of those children.
    Platform {
        feature: String,
        platform: String,
        child: Option<String>,
    },
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hosts(name) | Self::Base(name) | Self::Crate(name) | Self::Model(name) => {
                f.write_str(name)
            }
            Self::Platform {
                feature,
                platform,
                child: None,
            } => write!(f, "{feature}::{platform}"),
            Self::Platform {
                feature,
                platform,
                child: Some(child),
            } => write!(f, "{feature}::{platform}::{child}"),
        }
    }
}

impl FeatureAxis {
    /// The node a module path belongs to.
    fn node(&self, module: &[String]) -> Node {
        let Some((top, below)) = module.split_first() else {
            // The root is never a source or a target of an edge.
            return Node::Crate(String::new());
        };
        if *top == self.hosts {
            return Node::Hosts(top.clone());
        }
        if self.base.contains(top) {
            return Node::Base(top.clone());
        }
        if !self.features.contains(top) {
            return Node::Crate(top.clone());
        }
        let Some(platform) = below.first().filter(|word| self.platforms.contains(*word)) else {
            return Node::Model(top.clone());
        };
        let child = below
            .get(1)
            .filter(|name| {
                self.groups
                    .get(platform)
                    .is_some_and(|children| children.contains(*name))
            })
            .cloned();
        Node::Platform {
            feature: top.clone(),
            platform: platform.clone(),
            child,
        }
    }
}

/// A rule of the feature axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rule {
    /// Nothing outside `hosts` names it.
    Hosts,
    /// A base module names no feature.
    Base,
    /// A feature names another feature's model, never its platform files.
    ForeignPlatform,
    /// A platform file names no other platform's file.
    OtherPlatform,
}

impl Rule {
    /// The word the self-test files a finding under.
    pub(super) fn kind(self) -> &'static str {
        match self {
            Self::Hosts => "names hosts",
            Self::Base => "base names a feature",
            Self::ForeignPlatform => "names another feature's platform file",
            Self::OtherPlatform => "names another platform's file",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hosts => "nothing outside the hosts module names it",
            Self::Base => "a base module names no feature",
            Self::ForeignPlatform => {
                "a feature names another feature's model, never its platform files"
            }
            Self::OtherPlatform => "a platform file names no other platform's file",
        })
    }
}

/// The rule the edge `from -> to` breaks, if any. The nodes differ.
fn rule(from: &Node, to: &Node) -> Option<Rule> {
    match (from, to) {
        (Node::Hosts(_), _) => None,
        (_, Node::Hosts(_)) => Some(Rule::Hosts),
        (Node::Base(_), Node::Model(_) | Node::Platform { .. }) => Some(Rule::Base),
        (
            Node::Model(feature) | Node::Platform { feature, .. },
            Node::Platform { feature: other, .. },
        ) if feature != other => Some(Rule::ForeignPlatform),
        // Within one feature: platforms differ, or two children of one group do.
        (
            Node::Platform {
                platform, child, ..
            },
            Node::Platform {
                platform: other,
                child: other_child,
                ..
            },
        ) => {
            let siblings = matches!((child, other_child), (Some(a), Some(b)) if a != b);
            (platform != other || siblings).then_some(Rule::OtherPlatform)
        }
        _ => None,
    }
}

/// Checks `scan` against the feature axis of `declaration`.
pub(super) fn check(
    declaration: &Declaration,
    spec: &FeatureAxis,
    scan: &Scan,
    adr_files: &[String],
) -> Outcome {
    let mut findings = Vec::new();
    declared_names(declaration, spec, scan, &mut findings);
    declared_words(spec, &mut findings);

    let transparent: BTreeSet<String> = declaration
        .transparent
        .iter()
        .filter(|name| scan.modules.contains_key(*name))
        .cloned()
        .collect();
    relay_items(&transparent, scan, &mut findings);

    let reach = Reach {
        depth: Depth::Full,
        transparent: &transparent,
    };
    let edges = edges_between(
        scan,
        reach,
        |module: &[String]| spec.node(module),
        |module: &[String]| module.join("::"),
        &mut findings,
    );

    let mut refused = BTreeSet::new();
    let mut named = BTreeMap::new();
    for ((from, to), sites) in edges {
        let (from_name, to_name) = (from.to_string(), to.to_string());
        if let Some(rule) = rule(&from, &to) {
            refused.insert((from_name.clone(), to_name.clone()));
            if !admitted(&declaration.exceptions, &from_name, &to_name) {
                findings.push(Finding::Forbidden {
                    rule,
                    from: from_name.clone(),
                    to: to_name.clone(),
                    sites: sites.clone(),
                });
            }
        }
        named.insert((from_name, to_name), sites);
    }

    check_exceptions(
        &declaration.exceptions,
        |name| {
            let path: Vec<String> = name.split("::").map(str::to_owned).collect();
            scan.tree.contains_key(&path)
        },
        &refused,
        adr_files,
        &mut findings,
    );

    let features = spec
        .features
        .iter()
        .filter(|name| scan.modules.contains_key(*name))
        .count();
    let base = spec
        .base
        .iter()
        .filter(|name| scan.modules.contains_key(*name))
        .count();
    let platform_files: BTreeSet<Node> = scan
        .tree
        .keys()
        .map(|module| spec.node(module))
        .filter(|node| matches!(node, Node::Platform { .. }))
        .collect();
    let summary = format!(
        "{} modules ({features} features with {} platform files, {base} base), {} edges, {} \
         exceptions checked",
        scan.tree.len(),
        platform_files.len(),
        named.len(),
        declaration.exceptions.len()
    );
    Outcome {
        findings,
        edges: named,
        summary,
    }
}

/// The findings about the top-level names the declaration gives roles: each is
/// a top-level module of the crate, in one role only, and every top-level
/// module compiled outside tests has one.
fn declared_names(
    declaration: &Declaration,
    spec: &FeatureAxis,
    scan: &Scan,
    findings: &mut Vec<Finding>,
) {
    let mut seen = BTreeSet::new();
    let mut repeated = BTreeSet::new();
    let declared = spec
        .features
        .iter()
        .chain(&spec.base)
        .chain(std::iter::once(&spec.hosts))
        .chain(&spec.crate_level)
        .chain(&declaration.transparent);
    for name in declared {
        if !seen.insert(name.as_str()) {
            if repeated.insert(name.as_str()) {
                findings.push(Finding::Duplicate { name: name.clone() });
            }
        } else if name.contains("::") {
            findings.push(Finding::Nested { name: name.clone() });
        } else if !scan.modules.contains_key(name) {
            findings.push(Finding::Unknown { name: name.clone() });
        }
    }
    for module in scan.modules.keys() {
        if !seen.contains(module.as_str()) {
            findings.push(Finding::Undeclared {
                module: module.clone(),
            });
        }
    }
}

/// The findings about the platform words and the groups: each word is
/// declared once, each group splits a platform, and a child belongs to one
/// group and is no platform of its own.
fn declared_words(spec: &FeatureAxis, findings: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    let mut repeated = BTreeSet::new();
    for platform in &spec.platforms {
        if platform.contains("::") {
            findings.push(Finding::Nested {
                name: platform.clone(),
            });
        } else if !seen.insert(platform.as_str()) && repeated.insert(platform.as_str()) {
            findings.push(Finding::Duplicate {
                name: platform.clone(),
            });
        }
    }

    let mut seen = BTreeSet::new();
    let mut repeated = BTreeSet::new();
    for (group, children) in &spec.groups {
        if !spec.platforms.contains(group) {
            findings.push(Finding::UnknownGroup {
                group: group.clone(),
            });
        }
        for child in children {
            if spec.platforms.contains(child) {
                findings.push(Finding::ChildIsPlatform {
                    group: group.clone(),
                    child: child.clone(),
                });
            } else if child.contains("::") {
                findings.push(Finding::Nested {
                    name: child.clone(),
                });
            } else if !seen.insert(child.as_str()) && repeated.insert(child.as_str()) {
                findings.push(Finding::Duplicate {
                    name: child.clone(),
                });
            }
        }
    }
}
