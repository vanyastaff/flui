//! Synthetic `cargo metadata --all-features` output, for the self-test and
//! the unit tests: the same [`Metadata`] the real run reads, so the join in
//! [`Graph::from_metadata`](super::resolve::Graph::from_metadata) and
//! [`Members::load`](super::super::Members::load) run on it unchanged.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cargo_metadata::{DependencyKind, Metadata};
use serde_json::{Value as Json, json};

/// Where the synthetic workspace claims to live.
pub(super) const ROOT: &str = "/ws";

/// A dependency declaration.
#[derive(Debug, Clone)]
pub(super) struct Dep {
    kind: DependencyKind,
    optional: bool,
    default_features: bool,
    features: Vec<String>,
    rename: Option<String>,
    target: Option<String>,
}

impl Dep {
    pub(super) fn normal() -> Self {
        Self {
            kind: DependencyKind::Normal,
            optional: false,
            default_features: true,
            features: Vec::new(),
            rename: None,
            target: None,
        }
    }

    pub(super) fn dev() -> Self {
        Self {
            kind: DependencyKind::Development,
            ..Self::normal()
        }
    }

    #[cfg(test)]
    pub(super) fn build() -> Self {
        Self {
            kind: DependencyKind::Build,
            ..Self::normal()
        }
    }

    pub(super) fn optional(self) -> Self {
        Self {
            optional: true,
            ..self
        }
    }

    #[cfg(test)]
    pub(super) fn no_default_features(self) -> Self {
        Self {
            default_features: false,
            ..self
        }
    }

    #[cfg(test)]
    pub(super) fn features(self, features: &[&str]) -> Self {
        Self {
            features: features.iter().map(|&feature| feature.to_owned()).collect(),
            ..self
        }
    }

    #[cfg(test)]
    pub(super) fn renamed(self, rename: &str) -> Self {
        Self {
            rename: Some(rename.to_owned()),
            ..self
        }
    }

    #[cfg(test)]
    pub(super) fn target(self, cfg: &str) -> Self {
        Self {
            target: Some(cfg.to_owned()),
            ..self
        }
    }
}

struct Package {
    name: String,
    version: String,
    lib: String,
    member: bool,
    flui: Json,
    features: BTreeMap<String, Vec<String>>,
    deps: Vec<(usize, Dep)>,
}

impl Package {
    fn id(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// A workspace under construction.
pub(super) struct Fixture {
    workspace: Json,
    packages: Vec<Package>,
}

impl Fixture {
    /// A workspace with the real tier names and `reach` as its
    /// `[workspace.metadata.flui.reach]`.
    pub(super) fn new(reach: Json) -> Self {
        Self {
            workspace: json!({
                "flui": {
                    "tiers": ["V", "C", "S", "R", "K", "H", "pkg"],
                    "reach": reach,
                }
            }),
            packages: Vec::new(),
        }
    }

    /// The rules the self-test and most unit tests use: the real shape,
    /// fewer names.
    pub(super) fn standard() -> Self {
        Self::new(json!({
            "generic-ffi": [
                { "name": "windows-sys", "reason": "raw Win32 bindings" },
                { "name": "windows_*", "reason": "import libraries" },
            ],
            "tier": {
                "K": { "forbid": ["flui-platform", "winit", "windows", "wgpu", "flui-engine"] },
                "S": { "extends": "K" },
                "C": { "extends": "S" },
                "V": { "extends": "S", "forbid": ["tokio"] },
                "R": { "extends": "K" },
                "pkg": { "extends": "K", "forbid": ["windows-*", "windows_*"] },
            }
        }))
    }

    fn push(&mut self, name: &str, version: &str, lib: &str, member: bool, flui: Json) {
        assert!(
            self.find(name, version).is_none(),
            "BUG: fixture package {name} {version} added twice"
        );
        self.packages.push(Package {
            name: name.to_owned(),
            version: version.to_owned(),
            lib: lib.to_owned(),
            member,
            flui,
            features: BTreeMap::new(),
            deps: Vec::new(),
        });
    }

    fn find(&self, name: &str, version: &str) -> Option<usize> {
        self.packages
            .iter()
            .position(|package| package.name == name && package.version == version)
    }

    fn at(&self, name: &str) -> usize {
        let found: Vec<usize> = (0..self.packages.len())
            .filter(|&at| self.packages[at].name == name)
            .collect();
        assert!(
            found.len() == 1,
            "BUG: fixture package {name} is not unique (name@version to choose)"
        );
        found[0]
    }

    /// `name` or `name@version`.
    fn spec(&self, spec: &str) -> usize {
        match spec.split_once('@') {
            Some((name, version)) => self
                .find(name, version)
                .unwrap_or_else(|| panic!("BUG: no fixture package {spec}")),
            None => self.at(spec),
        }
    }

    /// A workspace member of `tier` with extra `[package.metadata.flui]`
    /// keys from `flui` (an object, or `null`).
    pub(super) fn member(mut self, name: &str, tier: &str, flui: &Json) -> Self {
        let mut table = json!({ "tier": tier, "tier-kind": "internal", "order": 1 });
        if let Some(extra) = flui.as_object() {
            for (key, value) in extra {
                table[key] = value.clone();
            }
        }
        self.push(name, "0.1.0", &name.replace('-', "_"), true, table);
        self
    }

    /// A workspace member without a tier: an example.
    #[cfg(test)]
    pub(super) fn example(mut self, name: &str) -> Self {
        self.push(
            name,
            "0.1.0",
            &name.replace('-', "_"),
            true,
            json!({ "tier-kind": "tool" }),
        );
        self
    }

    /// A registry package.
    pub(super) fn external(self, name: &str) -> Self {
        self.external_version(name, "1.0.0", &name.replace('-', "_"))
    }

    /// A registry package at `version` whose library is named `lib`.
    pub(super) fn external_version(mut self, name: &str, version: &str, lib: &str) -> Self {
        self.push(name, version, lib, false, Json::Null);
        self
    }

    /// `[features] feature = values` on `package`.
    pub(super) fn feature(mut self, package: &str, feature: &str, values: &[&str]) -> Self {
        let at = self.spec(package);
        self.packages[at].features.insert(
            feature.to_owned(),
            values.iter().map(|&value| value.to_owned()).collect(),
        );
        self
    }

    /// `from` declares `dep` on `to` (`name` or `name@version`).
    pub(super) fn dep(mut self, from: &str, to: &str, dep: Dep) -> Self {
        let (from, to) = (self.spec(from), self.spec(to));
        self.packages[from].deps.push((to, dep));
        self
    }

    /// The `cargo metadata` document: every declared dependency, optional
    /// ones included, is in the resolve graph, as under `--all-features`.
    pub(super) fn metadata(&self) -> Metadata {
        let packages: Vec<Json> = self
            .packages
            .iter()
            .map(|package| {
                let dependencies: Vec<Json> = package
                    .deps
                    .iter()
                    .map(|(to, dep)| {
                        let target = &self.packages[*to];
                        json!({
                            "name": target.name,
                            "source": null,
                            "req": format!("={}", target.version),
                            "kind": kind(dep.kind),
                            "optional": dep.optional,
                            "uses_default_features": dep.default_features,
                            "features": dep.features,
                            "target": dep.target,
                            "rename": dep.rename,
                            "registry": null,
                            "path": target.member.then(|| format!("{ROOT}/crates/{}", target.name)),
                        })
                    })
                    .collect();
                json!({
                    "name": package.name,
                    "version": package.version,
                    "id": package.id(),
                    "source": null,
                    "dependencies": dependencies,
                    "targets": [],
                    "features": package.features,
                    "manifest_path": manifest(package),
                    "metadata": if package.member { json!({ "flui": package.flui }) } else { Json::Null },
                })
            })
            .collect();
        let nodes: Vec<Json> = self
            .packages
            .iter()
            .map(|package| {
                // one resolve edge per (package, extern name), carrying every
                // kind and target it is declared under
                let mut deps: BTreeMap<(String, String), Vec<Json>> = BTreeMap::new();
                for (to, dep) in &package.deps {
                    let target = &self.packages[*to];
                    let name = dep
                        .rename
                        .as_ref()
                        .unwrap_or(&target.lib)
                        .replace('-', "_");
                    deps.entry((target.id(), name))
                        .or_default()
                        .push(json!({ "kind": kind(dep.kind), "target": dep.target }));
                }
                let dependencies: Vec<&String> = deps.keys().map(|(id, _)| id).collect();
                let deps: Vec<Json> = deps
                    .iter()
                    .map(|((id, name), kinds)| json!({ "name": name, "pkg": id, "dep_kinds": kinds }))
                    .collect();
                json!({ "id": package.id(), "deps": deps, "dependencies": dependencies, "features": [] })
            })
            .collect();
        let members: Vec<String> = self
            .packages
            .iter()
            .filter(|package| package.member)
            .map(Package::id)
            .collect();
        serde_json::from_value(json!({
            "packages": packages,
            "workspace_members": members,
            "resolve": { "nodes": nodes, "root": null },
            "workspace_root": ROOT,
            "target_directory": format!("{ROOT}/target"),
            "metadata": self.workspace,
            "version": 1,
        }))
        .expect("BUG: the fixture is valid `cargo metadata` output")
    }

    /// The directory [`Members::load`](super::super::Members::load) takes
    /// paths relative to.
    pub(super) fn root() -> PathBuf {
        PathBuf::from(ROOT)
    }
}

fn kind(kind: DependencyKind) -> Json {
    match kind {
        DependencyKind::Development => json!("dev"),
        DependencyKind::Build => json!("build"),
        _ => Json::Null,
    }
}

fn manifest(package: &Package) -> String {
    if package.member {
        format!("{ROOT}/crates/{}/Cargo.toml", package.name)
    } else {
        format!("/registry/{}-{}/Cargo.toml", package.name, package.version)
    }
}
