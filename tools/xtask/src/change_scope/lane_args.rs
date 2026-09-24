//! Turns a [`Scope`] into the cargo arguments the fast lane runs -- the one
//! place the test-scope policy lives, for CI's `plan` job and
//! `cargo xtask check-changed` alike:
//!
//! - tests exclude flui-platform (its suite needs a display server: a separate
//!   headless leg runs it when it is in scope);
//! - the facade's non-default catalogs join the run when `flui` is in scope
//!   (`--features flui/cupertino,flui/localizations`);
//! - cfg-gated code the Linux lane would never compile gets a check on its own
//!   target: flui-platform's four backends, the flui-app/flui mobile runner,
//!   the flui-cli Windows paths (mirroring the cross-typecheck job), and wasm32
//!   for the wasm-capable packages in scope (mirroring wasm-check; a package
//!   opts out with `[package.metadata.flui] wasm = false`);
//! - the per-feature clippy pass (feature-matrix's `cargo hack --each-feature`)
//!   covers the changed crates that have features, any crate whose Cargo.toml
//!   changed, and every dependent in scope whose edge to an in-scope package
//!   only exists under one of its features (optional, or named in a feature):
//!   the default build never compiles the code on that edge;
//! - the flui-app/flui iOS runner gets a macOS clippy leg whenever either is in
//!   scope, like the Android runner (it needs xcrun, so it is a separate job,
//!   `fast-lane-ios`);
//! - rustdoc -D warnings runs over the scope with its packages' `testing`
//!   features (the doc job's flags, narrowed): a moved item's broken intra-doc
//!   link otherwise merges green and fails main's `doc` job;
//! - doctests run over the scope's library packages (the heavy `doc-test` job,
//!   narrowed): nextest never executes them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::classify::{Mode, Package, Repo, Scope, Workspace};

/// More feature-gated dependents than this: the heavy lane (see [`lane_args`]).
const MAX_FEATURE_GATED_DEPENDENTS: usize = 3;

/// The fast lane's inputs, one field per `plan` output, in output order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaneArgs {
    pub(super) mode: String,
    pub(super) heavy_required: bool,
    pub(super) reason: String,
    pub(super) packages: String,
    pub(super) pkg_args: String,
    pub(super) test_args: String,
    pub(super) features: String,
    pub(super) platform: bool,
    pub(super) cross_platform: bool,
    pub(super) cross_app: bool,
    pub(super) cross_cli: bool,
    pub(super) cross_desktop_mcp: bool,
    pub(super) cross_ios: bool,
    pub(super) wasm_args: String,
    pub(super) wasm_facade: bool,
    pub(super) hack_args: String,
    pub(super) doc_args: String,
    pub(super) doctest_args: String,
}

impl LaneArgs {
    /// `(key, value)` in output order; booleans as `true`/`false`.
    pub(super) fn fields(&self) -> [(&'static str, String); 18] {
        let b = |v: bool| if v { "true" } else { "false" }.to_owned();
        [
            ("mode", self.mode.clone()),
            ("heavy_required", b(self.heavy_required)),
            ("reason", self.reason.clone()),
            ("packages", self.packages.clone()),
            ("pkg_args", self.pkg_args.clone()),
            ("test_args", self.test_args.clone()),
            ("features", self.features.clone()),
            ("platform", b(self.platform)),
            ("cross_platform", b(self.cross_platform)),
            ("cross_app", b(self.cross_app)),
            ("cross_cli", b(self.cross_cli)),
            // flui-desktop-mcp's Windows and macOS backends, invisible to the
            // Linux build: its own cross clippy whenever it is in scope
            ("cross_desktop_mcp", b(self.cross_desktop_mcp)),
            // the flui-app / flui iOS runner: its clippy needs macOS (xcrun);
            // the facade gates code on iOS too, so either one in scope runs it
            ("cross_ios", b(self.cross_ios)),
            ("wasm_args", self.wasm_args.clone()),
            ("wasm_facade", b(self.wasm_facade)),
            ("hack_args", self.hack_args.clone()),
            ("doc_args", self.doc_args.clone()),
            ("doctest_args", self.doctest_args.clone()),
        ]
    }
}

/// `-p a -p b`, sorted.
/// Target-kind flags for a test or lint run over `names`: `--lib` only when at
/// least one of them has a library target, because cargo rejects `--lib` when
/// none does (a change touching only `xtask`, which is a binary).
fn target_flags<'a>(
    packages: &BTreeMap<String, Package>,
    names: impl IntoIterator<Item = &'a str>,
    rest: &str,
) -> String {
    let any_lib = names.into_iter().any(|n| {
        packages.get(n).is_some_and(|pkg| {
            pkg.targets.iter().any(|t| {
                t.kind.iter().any(|k| {
                    matches!(
                        k.as_str(),
                        "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                    )
                })
            })
        })
    });
    if any_lib {
        format!("--lib {rest}")
    } else {
        rest.to_owned()
    }
}

fn p<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let names: BTreeSet<&str> = names.into_iter().collect();
    names
        .into_iter()
        .map(|n| format!("-p {n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The packages whose manifest says they do not build for wasm32.
pub(super) fn no_wasm_packages(workspace: &Workspace) -> BTreeSet<String> {
    workspace
        .packages
        .values()
        .filter(|package| !package.builds_for_wasm())
        .map(|package| package.name.clone())
        .collect()
}

/// The feature names `default` turns on, transitively, within `pkg`.
fn default_features(pkg: &Package) -> BTreeSet<&str> {
    let mut on = BTreeSet::new();
    let mut todo = vec!["default"];
    while let Some(f) = todo.pop() {
        let Some(values) = pkg.features.get(f) else {
            continue;
        };
        if !on.insert(f) {
            continue;
        }
        todo.extend(
            values
                .iter()
                .map(String::as_str)
                .filter(|v| !v.contains('/') && !v.starts_with("dep:")),
        );
    }
    on
}

/// Packages in `scope` whose dependency on another in-scope package is
/// compiled only under a non-default feature: the dependency is optional and no
/// default feature activates it, or a non-default feature names it (`dep:x`,
/// `x/f`, `x?/f`) -- code behind `cfg(feature = ...)` then reaches into it. A
/// change to that package can break the code on the edge while the default
/// build stays green.
fn feature_gated_dependents<'a>(
    packages: &BTreeMap<String, Package>,
    scope: &BTreeSet<&'a str>,
) -> BTreeSet<&'a str> {
    let mut found = BTreeSet::new();
    for &name in scope {
        let Some(pkg) = packages.get(name) else {
            continue;
        };
        let defaults = default_features(pkg);
        let values = |on: bool| -> Vec<&str> {
            pkg.features
                .iter()
                .filter(|(f, _)| defaults.contains(f.as_str()) == on)
                .flat_map(|(_, vs)| vs.iter().map(String::as_str))
                .collect()
        };
        let (on_by_default, off_by_default) = (values(true), values(false));
        for dep in &pkg.dependencies {
            if !scope.contains(dep.name.as_str())
                || dep.name == name
                || dep.kind.as_deref() == Some("dev")
            {
                continue;
            }
            let key = dep.rename.as_deref().unwrap_or(&dep.name);
            let names = |vals: &[&str]| {
                vals.iter().any(|v| {
                    *v == key
                        || v.strip_prefix("dep:") == Some(key)
                        || v.strip_prefix(key)
                            .is_some_and(|rest| rest.starts_with('/') || rest.starts_with("?/"))
                })
            };
            if (dep.optional && !names(&on_by_default)) || names(&off_by_default) {
                found.insert(name);
                break;
            }
        }
    }
    found
}

/// The fast lane's arguments for `scope`.
pub(super) fn lane_args(repo: &Repo, scope: &Scope) -> anyhow::Result<LaneArgs> {
    let full = scope.mode == Mode::Full;
    let compiles = matches!(scope.mode, Mode::Packages | Mode::Full);
    let set: BTreeSet<&str> = scope.packages.iter().map(String::as_str).collect();
    let has = |name: &str| full || set.contains(name);

    let wasm_args = if !compiles {
        String::new()
    } else if full {
        let no_wasm = no_wasm_packages(repo.workspace()?);
        let excludes: Vec<String> = no_wasm.iter().map(|n| format!("--exclude {n}")).collect();
        format!("--workspace {} --lib --bins", excludes.join(" "))
    } else {
        let no_wasm = no_wasm_packages(repo.workspace()?);
        let names: Vec<&str> = set
            .iter()
            .copied()
            .filter(|n| !no_wasm.contains(*n))
            .collect();
        if names.is_empty() {
            String::new()
        } else {
            let packages = &repo.workspace()?.packages;
            format!(
                "{} {}",
                p(names.iter().copied()),
                target_flags(packages, names.iter().copied(), "--bins")
            )
        }
    };

    // rustdoc -D warnings over the scope, narrowed; `--features x/testing` is
    // only valid for a selected package.
    let mut doc_args = String::new();
    let mut doctest_args = String::new();
    if compiles {
        let packages = &repo.workspace()?.packages;
        let testing: Vec<&str> = packages
            .values()
            .filter(|pkg| {
                pkg.features.contains_key("testing") && (full || set.contains(pkg.name.as_str()))
            })
            .map(|pkg| pkg.name.as_str())
            .collect();
        doc_args = if full {
            "--workspace".to_owned()
        } else {
            p(set.iter().copied())
        };
        if !testing.is_empty() {
            let feats: Vec<String> = testing.iter().map(|n| format!("{n}/testing")).collect();
            let _ = write!(doc_args, " --features {}", feats.join(","));
        }
        // doctests (nextest runs none): `cargo test --doc -p` errors on a
        // package without a library target
        doctest_args = if full {
            "--workspace".to_owned()
        } else {
            let lib_kinds = ["lib", "rlib", "dylib", "proc-macro"];
            p(set.iter().copied().filter(|n| {
                packages.get(*n).is_some_and(|pkg| {
                    pkg.targets
                        .iter()
                        .any(|t| t.kind.iter().any(|k| lib_kinds.contains(&k.as_str())))
                })
            }))
        };
    }

    // Dependents that compile the change only under a feature get the
    // per-feature pass; past a handful, that is the heavy lane's sliced
    // feature-matrix job, not a fast-lane step.
    let mut heavy_required = scope.heavy_required;
    let mut reason = scope.reason.clone();
    let mut hack_args = String::new();
    if scope.mode == Mode::Packages {
        let packages = &repo.workspace()?.packages;
        let featured = |n: &str| {
            packages
                .get(n)
                .is_some_and(|pkg| pkg.features.keys().any(|f| f != "default"))
        };
        let seeds: BTreeSet<&str> = scope.seeds.iter().map(String::as_str).collect();
        let gated: BTreeSet<&str> = feature_gated_dependents(packages, &set)
            .into_iter()
            .filter(|n| featured(n) && !seeds.contains(n))
            .collect();
        if gated.len() > MAX_FEATURE_GATED_DEPENDENTS {
            heavy_required = true;
            let first: Vec<&str> = gated.iter().copied().take(4).collect();
            let _ = write!(
                reason,
                "; {} dependents reach the change only under a feature ({}, ...): the feature-matrix job covers them",
                gated.len(),
                first.join(", ")
            );
        }
        // per-feature clippy for the crates the change is IN (seeds) that have
        // features, for any crate whose manifest changed, and for dependents
        // whose edge into the scope only a feature compiles; other dependents
        // get the default build only (the heavy feature-matrix covers the rest)
        hack_args = p(seeds
            .iter()
            .copied()
            .filter(|n| featured(n))
            .chain(scope.manifests.iter().map(String::as_str))
            .chain(gated.iter().copied()));
    }

    Ok(LaneArgs {
        mode: scope.mode.as_str().to_owned(),
        heavy_required,
        reason,
        packages: scope.packages.join(" "),
        pkg_args: if full {
            "--workspace".to_owned()
        } else {
            p(set.iter().copied())
        },
        test_args: if full {
            "--workspace --exclude flui-platform --lib --bins --tests".to_owned()
        } else {
            let names: Vec<&str> = set
                .iter()
                .copied()
                .filter(|n| *n != "flui-platform")
                .collect();
            if names.is_empty() {
                String::new()
            } else {
                let packages = &repo.workspace()?.packages;
                format!(
                    "{} {}",
                    p(names.iter().copied()),
                    target_flags(packages, names.iter().copied(), "--bins --tests")
                )
            }
        },
        features: if has("flui") {
            "--features flui/cupertino,flui/localizations".to_owned()
        } else {
            String::new()
        },
        platform: has("flui-platform"),
        cross_platform: has("flui-platform"),
        cross_app: has("flui-app") || has("flui"),
        cross_cli: has("flui-cli"),
        cross_desktop_mcp: has("flui-desktop-mcp"),
        cross_ios: has("flui-app") || has("flui"),
        wasm_args,
        wasm_facade: has("flui"),
        hack_args,
        doc_args,
        doctest_args,
    })
}

#[cfg(test)]
mod tests {
    use super::super::classify::tests::{repo, scope};
    use super::*;

    fn args(files: &[&str]) -> LaneArgs {
        lane_args(repo(), &scope(files)).expect("lane args")
    }

    #[test]
    fn a_changed_manifest_gets_the_per_feature_pass() {
        assert_eq!(
            args(&["crates/flui-material/Cargo.toml"]).hack_args,
            "-p flui-material"
        );
    }

    #[test]
    fn cfg_gated_backends_get_their_targets() {
        let a = args(&["crates/flui-platform/src/lib.rs"]);
        assert_eq!(
            (a.cross_platform, a.cross_app, a.platform),
            (true, true, true)
        );
        assert!(!a.test_args.contains("flui-platform")); // its suite runs in the headless leg
        let a = args(&["crates/flui-material/src/lib.rs"]);
        assert_eq!((a.cross_platform, a.cross_cli), (false, false));
        assert!(a.cross_app); // `flui` is in scope: its mobile runner is
    }

    #[test]
    fn wasm_scope_skips_packages_that_cannot_target_wasm() {
        let no_wasm = no_wasm_packages(repo().workspace().expect("cargo metadata"));
        assert!(no_wasm.contains("flui-cli")); // from its manifest, not restated
        assert!(!no_wasm.contains("flui-platform"));
        let a = args(&["crates/flui-platform/src/lib.rs"]);
        assert!(!a.wasm_args.contains("-p flui-cli"));
        assert!(a.wasm_args.contains("-p flui-platform"));
    }

    #[test]
    fn per_feature_pass_covers_feature_gated_dependents() {
        // flui-widgets reaches flui-assets only through its optional
        // `asset-images` edge: a source-only flui-assets change must compile
        // that edge, which the default build never does
        let a = args(&["crates/flui-assets/src/lib.rs"]);
        assert!(a.hack_args.contains("-p flui-assets"));
        assert!(a.hack_args.contains("-p flui-widgets"));
        assert!(!a.heavy_required);
    }

    #[test]
    fn default_on_optional_edges_need_no_per_feature_pass() {
        // `flui` takes flui-material through its default `material` feature
        assert_eq!(args(&["crates/flui-material/src/lib.rs"]).hack_args, "");
    }

    #[test]
    fn many_feature_gated_dependents_take_the_heavy_lane() {
        let a = args(&["crates/flui-view/src/lib.rs"]);
        assert!(a.heavy_required);
        assert!(a.reason.contains("feature-matrix"));
    }

    #[test]
    fn ios_leg_when_flui_app_or_the_facade_is_in_scope() {
        assert!(args(&["crates/flui-view/src/lib.rs"]).cross_ios);
        // the facade gates code on iOS too
        assert!(args(&["src/lib.rs"]).cross_ios);
        assert!(!args(&["crates/flui-cli/src/main.rs"]).cross_ios);
    }

    #[test]
    fn doctests_cover_the_scopes_library_packages() {
        let a = args(&["crates/flui-material/src/lib.rs"]);
        assert_eq!(a.doctest_args, "-p flui -p flui-material");
        // flui-web-counter is in scope but has no rlib: `cargo test --doc -p` would reject it
        assert!(a.packages.contains("flui-web-counter"));
        assert!(!a.doctest_args.contains("flui-web-counter"));
        // an included README is doctest source
        assert!(
            args(&["crates/flui-animation/README.md"])
                .doctest_args
                .contains("-p flui-animation")
        );
        assert_eq!(args(&["docs/x.md"]).doctest_args, "");
    }

    #[test]
    fn rustdoc_covers_the_scope_with_its_testing_features() {
        let a = args(&["crates/flui-material/src/lib.rs"]);
        assert!(a.doc_args.starts_with("-p flui -p flui-material"));
        assert!(a.doc_args.contains("--features flui/testing"));
        // a testing feature of a package outside the scope would be rejected by cargo
        assert!(!a.doc_args.contains("flui-rendering/testing"));
        assert_eq!(args(&["docs/x.md"]).doc_args, "");
    }

    #[test]
    fn a_binary_only_scope_gets_no_lib_flag() {
        // cargo rejects `--lib` when no selected package has a library, which
        // a change touching only xtask (a binary) would otherwise hit
        let a = args(&["tools/xtask/src/fonts.rs"]);
        assert_eq!(a.test_args, "-p xtask --bins --tests");
        assert_eq!(a.wasm_args, "", "xtask sets `wasm = false`");
        let mixed = args(&[
            "tools/xtask/src/fonts.rs",
            "crates/flui-geometry/src/lib.rs",
        ]);
        assert!(
            mixed.test_args.ends_with(" --lib --bins --tests"),
            "{}",
            mixed.test_args
        );
    }

    #[test]
    fn the_heavy_lane_selects_the_whole_workspace() {
        let a = lane_args(repo(), &Scope::whole_workspace()).expect("lane args");
        assert_eq!(
            (a.pkg_args.as_str(), a.test_args.as_str()),
            (
                "--workspace",
                "--workspace --exclude flui-platform --lib --bins --tests"
            )
        );
        assert!(
            a.wasm_args
                .starts_with("--workspace --exclude flui-assets --exclude flui-cli")
        );
        assert!(
            a.doc_args.starts_with("--workspace --features ")
                && a.doc_args.contains("flui/testing")
        );
        assert_eq!(
            (
                a.doctest_args.as_str(),
                a.hack_args.as_str(),
                a.packages.as_str()
            ),
            ("--workspace", "", "")
        );
        assert!(
            a.cross_platform
                && a.cross_app
                && a.cross_cli
                && a.cross_ios
                && a.wasm_facade
                && a.platform
        );
    }

    #[test]
    fn nothing_compiles_for_tooling_or_docs() {
        for files in [&["typos.toml"][..], &["docs/x.md"][..]] {
            let a = args(files);
            let empty = [
                &a.pkg_args,
                &a.test_args,
                &a.features,
                &a.wasm_args,
                &a.hack_args,
                &a.doc_args,
                &a.doctest_args,
            ];
            assert!(empty.iter().all(|v| v.is_empty()), "{files:?}: {a:?}");
            assert!(
                !(a.platform || a.cross_app || a.cross_ios || a.wasm_facade),
                "{files:?}"
            );
        }
    }
}
