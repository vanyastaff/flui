//! One root's build, resolved from the graph of every build.
//!
//! `cargo metadata` resolves features once for the whole workspace: an example
//! that depends on the facade with its defaults turns them on for every other
//! root, so its own resolution cannot say what `cargo build -p flui
//! --no-default-features` builds. `cargo metadata --all-features` still lists
//! every edge any root can activate, so this module takes its package and
//! dependency tables and resolves one root at a time, as resolver 2 does on
//! every target: dev edges are dropped, normal and build edges kept, and
//! every declared entry for one dependency key counts as one dependency,
//! whatever its target.
//!
//! Build-dependency and proc-macro features are unified with normal ones here,
//! where resolver 2 keeps them apart; that can only activate more, never less.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;

use anyhow::{Context, bail, ensure};
use cargo_metadata::{DependencyKind, Metadata, PackageId};

/// Every package and every normal or build edge any root can activate.
#[derive(Debug)]
pub(super) struct Graph {
    pub(super) packages: Vec<Package>,
    by_name: BTreeMap<String, Vec<usize>>,
}

/// A package: its feature table and its declared normal and build
/// dependencies, each joined to the package it resolved to.
#[derive(Debug)]
pub(super) struct Package {
    pub(super) name: String,
    /// A workspace member.
    pub(super) member: bool,
    pub(super) features: BTreeMap<String, Vec<String>>,
    pub(super) deps: Vec<Edge>,
}

/// One declared dependency, resolved.
#[derive(Debug)]
pub(super) struct Edge {
    pub(super) to: usize,
    /// The name features use for it: the rename, or the package name.
    pub(super) key: String,
    pub(super) optional: bool,
    pub(super) default_features: bool,
    pub(super) features: Vec<String>,
}

impl Graph {
    /// Joins each package's declared dependencies with the resolve graph's
    /// edges: by package name, plus the rename when there is one, and by
    /// kind. A resolve edge carries the library name (`xml` for `xml-rs`), so
    /// it never matches a declaration by itself.
    pub(super) fn from_metadata(metadata: &Metadata) -> anyhow::Result<Self> {
        let resolve = metadata
            .resolve
            .as_ref()
            .context("`cargo metadata` reported no resolve graph")?;
        let index: HashMap<&PackageId, usize> = metadata
            .packages
            .iter()
            .enumerate()
            .map(|(at, package)| (&package.id, at))
            .collect();
        let members: HashSet<&PackageId> = metadata.workspace_members.iter().collect();
        let nodes: HashMap<&PackageId, &cargo_metadata::Node> =
            resolve.nodes.iter().map(|node| (&node.id, node)).collect();

        let mut packages = Vec::with_capacity(metadata.packages.len());
        for package in &metadata.packages {
            let mut deps = Vec::new();
            // a package outside the resolve graph is in no build
            if let Some(node) = nodes.get(&package.id) {
                for dependency in &package.dependencies {
                    if dependency.kind == DependencyKind::Development {
                        continue;
                    }
                    let extern_name = dependency
                        .rename
                        .as_ref()
                        .map(|rename| rename.replace('-', "_"));
                    let mut targets: Vec<usize> = node
                        .deps
                        .iter()
                        .filter(|resolved| {
                            resolved
                                .dep_kinds
                                .iter()
                                .any(|kind| kind.kind == dependency.kind)
                                && metadata.packages[index[&resolved.pkg]].name.as_str()
                                    == dependency.name
                                && extern_name
                                    .as_ref()
                                    .is_none_or(|name| resolved.name == *name)
                        })
                        .map(|resolved| index[&resolved.pkg])
                        .collect();
                    if targets.len() > 1 {
                        targets
                            .retain(|&to| dependency.req.matches(&metadata.packages[to].version));
                    }
                    // an optional dependency no feature of the whole graph
                    // enables is absent from the resolve graph, and so from
                    // every build
                    ensure!(
                        !targets.is_empty() || dependency.optional,
                        "{}'s dependency {} is missing from the resolve graph",
                        package.name,
                        dependency.name
                    );
                    let key = dependency
                        .rename
                        .clone()
                        .unwrap_or_else(|| dependency.name.clone());
                    deps.extend(targets.into_iter().map(|to| Edge {
                        to,
                        key: key.clone(),
                        optional: dependency.optional,
                        default_features: dependency.uses_default_features,
                        features: dependency.features.clone(),
                    }));
                }
            }
            packages.push(Package {
                name: package.name.to_string(),
                member: members.contains(&package.id),
                features: package.features.clone(),
                deps,
            });
        }
        Ok(Self::new(packages))
    }

    fn new(packages: Vec<Package>) -> Self {
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (at, package) in packages.iter().enumerate() {
            by_name.entry(package.name.clone()).or_default().push(at);
        }
        Self { packages, by_name }
    }

    /// Every package named `name`: a crate may be in the graph at two
    /// versions.
    pub(super) fn named(&self, name: &str) -> &[usize] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// The workspace member named `name`.
    pub(super) fn member(&self, name: &str) -> anyhow::Result<usize> {
        self.named(name)
            .iter()
            .copied()
            .find(|&at| self.packages[at].member)
            .with_context(|| format!("{name} is not a workspace member"))
    }

    pub(super) fn name(&self, at: usize) -> &str {
        &self.packages[at].name
    }
}

/// The features a root build is asked for, as on a cargo command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Selection {
    /// The defaults, plus these.
    Defaults(Vec<String>),
    /// `--no-default-features`, plus these.
    NoDefaults(Vec<String>),
    /// `--all-features`.
    All,
}

impl Selection {
    /// Reads `""`, `--features a,b`, `--no-default-features [--features a,b]`
    /// or `--all-features`; any other flag is an error.
    pub(super) fn parse(combo: &str) -> anyhow::Result<Self> {
        let (mut no_defaults, mut all) = (false, false);
        let mut features = Vec::new();
        let mut words = combo.split_whitespace();
        while let Some(word) = words.next() {
            match word {
                "--no-default-features" => no_defaults = true,
                "--all-features" => all = true,
                "--features" => features.extend(
                    words
                        .next()
                        .with_context(|| format!("`--features` needs a list in `{combo}`"))?
                        .split(',')
                        .filter(|feature| !feature.is_empty())
                        .map(str::to_owned),
                ),
                other => bail!("unknown flag `{other}` in feature selection `{combo}`"),
            }
        }
        if all {
            ensure!(
                !no_defaults && features.is_empty(),
                "`--all-features` stands alone in `{combo}`"
            );
            return Ok(Self::All);
        }
        Ok(if no_defaults {
            Self::NoDefaults(features)
        } else {
            Self::Defaults(features)
        })
    }
}

impl fmt::Display for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("--all-features"),
            Self::Defaults(features) if features.is_empty() => Ok(()),
            Self::Defaults(features) => write!(f, "--features {}", features.join(",")),
            Self::NoDefaults(features) if features.is_empty() => {
                f.write_str("--no-default-features")
            }
            Self::NoDefaults(features) => {
                write!(f, "--no-default-features --features {}", features.join(","))
            }
        }
    }
}

/// What one root builds: the activated edges and each package's features.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Build {
    /// Per package, the packages its activated normal and build edges reach.
    pub(super) edges: Vec<BTreeSet<usize>>,
    /// Per package, its enabled features.
    pub(super) features: Vec<BTreeSet<String>>,
    /// Every package in the build, the root included.
    pub(super) reached: BTreeSet<usize>,
}

impl Build {
    /// Every package reachable from `starts`, `starts` included.
    pub(super) fn closure(&self, starts: impl IntoIterator<Item = usize>) -> BTreeSet<usize> {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        for start in starts {
            if seen.insert(start) {
                queue.push_back(start);
            }
        }
        while let Some(at) = queue.pop_front() {
            for &next in &self.edges[at] {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        seen
    }
}

/// A step of the resolution.
enum Work {
    /// The package joins the build: its non-optional edges activate.
    Add(usize),
    /// Enable a feature, or apply one entry of a feature's list.
    Enable(usize, String),
    /// Activate the optional dependency with this key.
    Activate(usize, String),
    /// `key/feature` (`strong`) or `key?/feature`.
    Forward {
        at: usize,
        key: String,
        feature: String,
        strong: bool,
    },
}

/// Resolution state, per package.
struct State<'g> {
    graph: &'g Graph,
    edges: Vec<BTreeSet<usize>>,
    features: Vec<BTreeSet<String>>,
    added: Vec<bool>,
    /// Activated optional dependency keys.
    active: Vec<BTreeSet<String>>,
    /// Features forwarded to a dependency key, applied when it activates.
    forwarded: Vec<BTreeMap<String, BTreeSet<String>>>,
    queue: VecDeque<Work>,
}

impl State<'_> {
    fn package(&self, at: usize) -> &Package {
        &self.graph.packages[at]
    }

    /// An edge activates: its target joins, with the edge's features, its
    /// defaults when it keeps them, and whatever was forwarded to its key.
    fn link(&mut self, at: usize, edge: usize) {
        let graph = self.graph;
        let edge = &graph.packages[at].deps[edge];
        self.edges[at].insert(edge.to);
        self.queue.push_back(Work::Add(edge.to));
        if edge.default_features {
            self.queue
                .push_back(Work::Enable(edge.to, "default".to_owned()));
        }
        let forwarded = self.forwarded[at].get(&edge.key).into_iter().flatten();
        for feature in edge.features.iter().chain(forwarded) {
            self.queue.push_back(Work::Enable(edge.to, feature.clone()));
        }
    }

    fn has_optional(&self, at: usize, key: &str) -> bool {
        self.package(at)
            .deps
            .iter()
            .any(|edge| edge.optional && edge.key == key)
    }

    /// One entry of a feature list, or one feature asked for on the root.
    fn apply(&mut self, at: usize, value: &str) -> anyhow::Result<()> {
        if let Some(key) = value.strip_prefix("dep:") {
            self.queue.push_back(Work::Activate(at, key.to_owned()));
        } else if let Some((key, feature)) = value.split_once("?/") {
            self.forward(at, key, feature, false);
        } else if let Some((key, feature)) = value.split_once('/') {
            self.forward(at, key, feature, true);
        } else {
            return self.enable(at, value);
        }
        Ok(())
    }

    fn forward(&mut self, at: usize, key: &str, feature: &str, strong: bool) {
        self.queue.push_back(Work::Forward {
            at,
            key: key.to_owned(),
            feature: feature.to_owned(),
            strong,
        });
    }

    fn enable(&mut self, at: usize, feature: &str) -> anyhow::Result<()> {
        if !self.features[at].insert(feature.to_owned()) {
            return Ok(());
        }
        let graph = self.graph;
        let package = &graph.packages[at];
        if let Some(values) = package.features.get(feature) {
            for value in values {
                self.apply(at, value)?;
            }
        } else if self.has_optional(at, feature) {
            // the implicit feature of an optional dependency
            self.queue.push_back(Work::Activate(at, feature.to_owned()));
        } else if feature != "default" {
            bail!("{} has no feature `{feature}`", package.name);
        }
        Ok(())
    }

    fn step(&mut self, work: Work) -> anyhow::Result<()> {
        let graph = self.graph;
        match work {
            Work::Add(at) => {
                if !std::mem::replace(&mut self.added[at], true) {
                    for edge in 0..graph.packages[at].deps.len() {
                        if !graph.packages[at].deps[edge].optional {
                            self.link(at, edge);
                        }
                    }
                }
            }
            Work::Enable(at, value) => self.apply(at, &value)?,
            Work::Activate(at, key) => {
                if self.active[at].insert(key.clone()) {
                    for edge in 0..graph.packages[at].deps.len() {
                        let declared = &graph.packages[at].deps[edge];
                        if declared.optional && declared.key == key {
                            self.link(at, edge);
                        }
                    }
                }
            }
            Work::Forward {
                at,
                key,
                feature,
                strong,
            } => {
                let package = &graph.packages[at];
                ensure!(
                    package.deps.iter().any(|edge| edge.key == key),
                    "{} forwards `{feature}` to `{key}`, which is not one of its dependencies",
                    package.name
                );
                self.forwarded[at]
                    .entry(key.clone())
                    .or_default()
                    .insert(feature.clone());
                if strong && self.has_optional(at, &key) {
                    self.queue.push_back(Work::Activate(at, key.clone()));
                    // the implicit feature of the same name, when the
                    // dependency has one
                    if package.features.get(&key).is_some_and(|values| {
                        values.len() == 1 && values[0] == format!("dep:{key}")
                    }) {
                        self.queue.push_back(Work::Enable(at, key.clone()));
                    }
                }
                let active = self.active[at].contains(&key);
                for edge in &package.deps {
                    if edge.key == key && (!edge.optional || active) {
                        self.queue.push_back(Work::Enable(edge.to, feature.clone()));
                    }
                }
            }
        }
        Ok(())
    }
}

/// What `cargo build -p <root> <selection>` builds, on every target.
pub(super) fn resolve(graph: &Graph, root: usize, selection: &Selection) -> anyhow::Result<Build> {
    let count = graph.packages.len();
    let mut state = State {
        graph,
        edges: vec![BTreeSet::new(); count],
        features: vec![BTreeSet::new(); count],
        added: vec![false; count],
        active: vec![BTreeSet::new(); count],
        forwarded: vec![BTreeMap::new(); count],
        queue: VecDeque::new(),
    };
    state.queue.push_back(Work::Add(root));
    let asked: Vec<String> = match selection {
        Selection::All => graph.packages[root].features.keys().cloned().collect(),
        Selection::Defaults(features) => std::iter::once("default".to_owned())
            .chain(features.iter().cloned())
            .collect(),
        Selection::NoDefaults(features) => features.clone(),
    };
    for feature in asked {
        state.queue.push_back(Work::Enable(root, feature));
    }
    while let Some(work) = state.queue.pop_front() {
        state.step(work)?;
    }
    let reached = (0..count).filter(|&at| state.added[at]).collect();
    Ok(Build {
        edges: state.edges,
        features: state.features,
        reached,
    })
}
