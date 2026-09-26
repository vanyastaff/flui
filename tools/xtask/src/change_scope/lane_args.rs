//! Turns a [`Scope`] into the lane a run takes and the cargo arguments the
//! fast lane runs -- the one place the lane and test-scope policy lives, for
//! CI's `plan` job and `cargo xtask check-changed` alike.
//!
//! The lane ([`Lane::decide`]) comes from the event, the `full-ci` label and
//! the classification: `docs` and `tooling` compile nothing, `fast` is the
//! scoped `fast-lane` job, `wide` every Linux job over the whole workspace,
//! `full` adds the Windows and macOS jobs, `extended` the nightly-only platform
//! jobs on top. The fast lane's arguments:
//!
//! - tests exclude flui-platform (its suite needs a display server: a separate
//!   headless leg runs it when it is in scope);
//! - the facade's non-default catalogs join the run when `flui` is in scope
//!   (`--features flui/cupertino`);
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
//! - doctests run over the scope's library packages (the `doc-test` job,
//!   narrowed): nextest never executes them;
//! - CI's `fast-lane` runs its tests as `ci_test_args`: the workspace's
//!   [`TEST_SCOPE`](crate::tasks::TEST_SCOPE) build, narrowed to the scope by a
//!   nextest filterset, so it builds what the `workspace-tests` cache holds
//!   (`test_args`, the scoped build, stays for `check-changed`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::classify::{Mode, Package, Repo, Scope, Workspace};

/// More feature-gated dependents than this: the wide lane (see [`lane_args`]).
const MAX_FEATURE_GATED_DEPENDENTS: usize = 3;

/// The GitHub event a run was started by (`github.event_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum Event {
    #[value(name = "pull_request")]
    PullRequest,
    #[value(name = "push")]
    Push,
    #[value(name = "merge_group")]
    MergeGroup,
    #[value(name = "schedule")]
    Schedule,
    #[value(name = "workflow_dispatch")]
    WorkflowDispatch,
}

/// Which of ci.yml's jobs a run starts, from least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    /// Documentation only: `checks`, `plan`.
    Docs,
    /// Repository tooling or a standalone crate: `deps` and `standalone` too.
    Tooling,
    /// A set of packages: `deps`, `fast-lane` and, for the iOS runner, `fast-lane-ios`.
    Fast,
    /// The whole workspace on a pull request: every Linux job (`HEAVY_JOBS`).
    Wide,
    /// `main` and the merge queue: `wide` plus the Windows and macOS jobs (`FULL_JOBS`).
    Full,
    /// Nightly, dispatch and the `full-ci` label: `full` plus `EXTENDED_JOBS`.
    Extended,
}

impl Lane {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Tooling => "tooling",
            Self::Fast => "fast",
            Self::Wide => "wide",
            Self::Full => "full",
            Self::Extended => "extended",
        }
    }

    /// The lane for a run: the event first, then the `full-ci` label, then
    /// what the change touches. This is the only place the label's lane is
    /// decided.
    pub(super) fn decide(
        event: Event,
        full_ci_label: bool,
        mode: Mode,
        heavy_required: bool,
    ) -> Self {
        match event {
            Event::Schedule | Event::WorkflowDispatch => Self::Extended,
            Event::Push | Event::MergeGroup => Self::Full,
            Event::PullRequest if full_ci_label => Self::Extended,
            Event::PullRequest if heavy_required => Self::Wide,
            Event::PullRequest => match mode {
                Mode::Full => Self::Wide,
                Mode::Packages => Self::Fast,
                Mode::None => Self::Tooling,
                Mode::Docs => Self::Docs,
            },
        }
    }

    /// Whether the lane's jobs build the whole workspace.
    fn is_whole_workspace(self) -> bool {
        matches!(self, Self::Wide | Self::Full | Self::Extended)
    }
}

/// The `plan` outputs, one field per output, in output order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaneArgs {
    pub(super) lane: Lane,
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
    /// Standalone crate directories the change touches (own `[workspace]`,
    /// outside every member's graph), space-separated.
    pub(super) standalone: String,
    /// CI `fast-lane`'s nextest arguments: [`TEST_SCOPE`](crate::tasks::TEST_SCOPE)
    /// plus `-E` and the scope's packages as a filterset without spaces (the
    /// job word-splits it); empty when nothing but flui-platform is in scope.
    pub(super) ci_test_args: String,
}

impl LaneArgs {
    /// `(key, value)` in output order; booleans as `true`/`false`.
    pub(super) fn fields(&self) -> [(&'static str, String); 21] {
        let b = |v: bool| if v { "true" } else { "false" }.to_owned();
        [
            ("lane", self.lane.as_str().to_owned()),
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
            ("standalone", self.standalone.clone()),
            ("ci_test_args", self.ci_test_args.clone()),
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

/// CI's `plan` outputs: [`lane_args`], rebuilt over the whole workspace when
/// the lane builds the whole workspace but the classification scoped it (a
/// heavy-job input, many feature-gated dependents, the `full-ci` label). The
/// classification's reason stays. `check-changed` reads [`lane_args`] itself
/// and so keeps its scoped build.
pub(super) fn plan_args(
    repo: &Repo,
    scope: &Scope,
    event: Event,
    full_ci_label: bool,
) -> anyhow::Result<LaneArgs> {
    let args = lane_args(repo, scope, event, full_ci_label)?;
    if !args.lane.is_whole_workspace() || scope.mode == Mode::Full {
        return Ok(args);
    }
    let mut whole = Scope::whole_workspace();
    whole.reason = args.reason;
    whole.heavy_required = args.heavy_required;
    Ok(LaneArgs {
        lane: args.lane,
        ..lane_args(repo, &whole, event, full_ci_label)?
    })
}

/// The `test_args` of CI's `fast-lane`: [`TEST_SCOPE`](crate::tasks::TEST_SCOPE)
/// and a filterset naming `packages` but flui-platform (its own leg runs it),
/// or empty when no other package is in scope: an empty `-E ''` would not
/// parse. No spaces inside the filterset: the job word-splits the value.
fn ci_test_args(packages: &BTreeSet<&str>) -> String {
    let filter: Vec<String> = packages
        .iter()
        .filter(|n| **n != "flui-platform")
        .map(|n| format!("package({n})"))
        .collect();
    if filter.is_empty() {
        return String::new();
    }
    format!(
        "{} -E {}",
        crate::tasks::TEST_SCOPE.join(" "),
        filter.join("|")
    )
}

/// The lane and the fast lane's arguments for `scope`, on `event`.
pub(super) fn lane_args(
    repo: &Repo,
    scope: &Scope,
    event: Event,
    full_ci_label: bool,
) -> anyhow::Result<LaneArgs> {
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
    // per-feature pass; past a handful, that is the wide lane's sliced
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
        // get the default build only (the wide lane's feature-matrix covers the rest)
        hack_args = p(seeds
            .iter()
            .copied()
            .filter(|n| featured(n))
            .chain(scope.manifests.iter().map(String::as_str))
            .chain(gated.iter().copied()));
    }

    Ok(LaneArgs {
        lane: Lane::decide(event, full_ci_label, scope.mode, heavy_required),
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
            "--features flui/cupertino".to_owned()
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
        standalone: scope.standalone.join(" "),
        ci_test_args: if scope.mode == Mode::Packages {
            ci_test_args(&set)
        } else {
            String::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::super::classify::tests::{repo, scope};
    use super::*;

    fn args(files: &[&str]) -> LaneArgs {
        lane_args(repo(), &scope(files), Event::PullRequest, false).expect("lane args")
    }

    /// The lane a pull request changing `files` takes.
    fn pr_lane(files: &[&str], full_ci_label: bool) -> Lane {
        plan_args(repo(), &scope(files), Event::PullRequest, full_ci_label)
            .expect("plan args")
            .lane
    }

    #[test]
    fn whole_workspace_pr_takes_the_wide_lane() {
        // the lane machinery is a workspace-wide input: every Linux job runs,
        // not the whole workspace serially in fast-lane
        let a = plan_args(
            repo(),
            &scope(&["tools/xtask/src/change_scope/classify.rs"]),
            Event::PullRequest,
            false,
        )
        .expect("plan args");
        assert_eq!((a.lane, a.mode.as_str()), (Lane::Wide, "full"));
        assert!(
            a.reason.starts_with("workspace-wide input changed: "),
            "{}",
            a.reason
        );
    }

    #[test]
    fn heavy_triggers_on_a_pr_take_the_wide_lane_not_full() {
        for path in ["Cargo.lock", ".github/workflows/ci.yml", "deny.toml"] {
            assert_eq!(pr_lane(&[path], false), Lane::Wide, "{path}");
            assert_eq!(pr_lane(&[path], true), Lane::Extended, "{path}");
        }
    }

    #[test]
    fn events_pick_their_lane() {
        use Event::{MergeGroup, PullRequest, Push, Schedule, WorkflowDispatch};
        for mode in [Mode::Docs, Mode::None, Mode::Packages, Mode::Full] {
            for heavy in [false, true] {
                for label in [false, true] {
                    assert_eq!(Lane::decide(Push, label, mode, heavy), Lane::Full);
                    assert_eq!(Lane::decide(MergeGroup, label, mode, heavy), Lane::Full);
                    assert_eq!(Lane::decide(Schedule, label, mode, heavy), Lane::Extended);
                    assert_eq!(
                        Lane::decide(WorkflowDispatch, label, mode, heavy),
                        Lane::Extended
                    );
                }
                assert_eq!(Lane::decide(PullRequest, true, mode, heavy), Lane::Extended);
            }
            assert_eq!(Lane::decide(PullRequest, false, mode, true), Lane::Wide);
        }
        let pr = |mode| Lane::decide(PullRequest, false, mode, false);
        assert_eq!(
            [
                pr(Mode::Docs),
                pr(Mode::None),
                pr(Mode::Packages),
                pr(Mode::Full)
            ],
            [Lane::Docs, Lane::Tooling, Lane::Fast, Lane::Wide]
        );
        // feature-gated edges into many dependents need feature-matrix
        assert_eq!(pr_lane(&["crates/flui-view/src/lib.rs"], false), Lane::Wide);
        let a = plan_args(
            repo(),
            &scope(&["crates/flui-view/src/lib.rs"]),
            Event::PullRequest,
            false,
        )
        .expect("plan args");
        assert_eq!(
            (a.pkg_args.as_str(), a.ci_test_args.as_str()),
            ("--workspace", ""),
            "the wide lane's jobs build the whole workspace"
        );
        assert!(a.reason.contains("feature-matrix"), "{}", a.reason);
        // check-changed keeps its scoped build
        assert_ne!(
            args(&["crates/flui-view/src/lib.rs"]).pkg_args,
            "--workspace"
        );
        let names: Vec<String> = [PullRequest, Push, MergeGroup, Schedule, WorkflowDispatch]
            .iter()
            .map(|e| {
                clap::ValueEnum::to_possible_value(e)
                    .expect("a name")
                    .get_name()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "pull_request",
                "push",
                "merge_group",
                "schedule",
                "workflow_dispatch"
            ],
            "github.event_name spellings"
        );
    }

    #[test]
    fn fast_lane_builds_the_test_scope_and_filters() {
        let a = args(&["crates/flui-material/src/lib.rs"]);
        assert_eq!(a.lane, Lane::Fast);
        assert_eq!(
            a.ci_test_args,
            "--workspace --exclude flui-platform --locked --no-fail-fast --lib --bins --tests \
             --features flui/cupertino \
             -E package(flui)|package(flui-material)|package(flui-sdk)|package(flui-web-counter)"
        );
        // check-changed keeps the scoped build
        assert_eq!(
            a.test_args,
            "-p flui -p flui-material -p flui-sdk -p flui-web-counter --lib --bins --tests"
        );
    }

    #[test]
    fn platform_only_scope_has_no_test_args() {
        let only_platform = Scope {
            mode: Mode::Packages,
            packages: vec!["flui-platform".to_owned()],
            seeds: vec!["flui-platform".to_owned()],
            manifests: Vec::new(),
            standalone: Vec::new(),
            heavy_required: false,
            reason: String::new(),
        };
        let a = lane_args(repo(), &only_platform, Event::PullRequest, false).expect("lane args");
        assert_eq!((a.ci_test_args.as_str(), a.test_args.as_str()), ("", ""));
        assert!(a.platform, "its own leg runs it");
    }

    #[test]
    fn a_standalone_crate_runs_the_tooling_lane() {
        let a = args(&["tools/text-spike/src/main.rs"]);
        assert_eq!(
            (a.lane, a.standalone.as_str(), a.pkg_args.as_str()),
            (Lane::Tooling, "tools/text-spike", "")
        );
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
        assert_eq!(
            (a.cross_platform, a.cross_cli, a.cross_desktop_mcp),
            (false, false, false)
        );
        assert!(a.cross_app); // `flui` is in scope: its mobile runner is
        // The desktop MCP server's Windows and macOS backends compile only
        // for those targets: a change to it gets their cross clippy.
        let a = args(&["tools/desktop-mcp/src/main.rs"]);
        assert!(a.cross_desktop_mcp);
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
    fn many_feature_gated_dependents_take_the_wide_lane() {
        let a = args(&["crates/flui-view/src/lib.rs"]);
        assert!(a.heavy_required);
        assert_eq!(a.lane, Lane::Wide);
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
        // flui-sdk is in scope through its dev-dependency on the facade
        assert_eq!(a.doctest_args, "-p flui -p flui-material -p flui-sdk");
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
    fn the_wide_lane_selects_the_whole_workspace() {
        let a =
            lane_args(repo(), &Scope::whole_workspace(), Event::Push, false).expect("lane args");
        assert_eq!((a.lane, a.ci_test_args.as_str()), (Lane::Full, ""));
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
                && a.cross_desktop_mcp
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
                &a.ci_test_args,
                &a.standalone,
            ];
            assert!(empty.iter().all(|v| v.is_empty()), "{files:?}: {a:?}");
            assert!(
                !(a.platform || a.cross_app || a.cross_ios || a.wasm_facade),
                "{files:?}"
            );
        }
        assert_eq!(args(&["typos.toml"]).lane, Lane::Tooling);
        assert_eq!(args(&["docs/x.md"]).lane, Lane::Docs);
    }
}
