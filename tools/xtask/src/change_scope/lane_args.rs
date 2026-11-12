//! Turns a [`Scope`] into the lane a CI run takes and the cargo arguments
//! a CI job runs for it -- the one place the lane and the scoped
//! test policy live.
//!
//! The lane ([`Lane::decide`]) comes from the event and
//! the classification: `docs` and `tooling` compile nothing, `wide` runs every
//! Linux PR job over the workspace in parallel; `full` (main, the merge queue
//! and a manual run) adds Linux checks. A pull request
//! that compiles anything takes `wide`: one serial job over the changed crates
//! measured slower than the parallel jobs over everything (35 against 18
//! minutes, median of the runs of 2026-09-30), and hosted runners do not queue
//! for this repository. The scoped arguments:
//!
//! - tests exclude flui-native (its suite needs a display server: a separate
//!   headless leg runs it when it is in scope);
//! - the facade's catalogs, none on by default, join the run
//!   when `flui` is in scope (`--features flui/material,flui/cupertino`);
//! - cfg-gated code the Linux lane would never compile gets a check on its own
//!   target: source-derived native Cargo targets (mirroring cross-typecheck), and wasm32
//!   for the wasm-capable packages in scope (mirroring wasm-check; a package
//!   opts out with `[package.metadata.flui] wasm = false`);
//! - the per-feature clippy pass (`feature-scope`, feature-matrix's
//!   `cargo hack --each-feature` narrowed) covers the changed crates that have
//!   features and any crate whose Cargo.toml changed, in the full mode of a
//!   pull request too (a Cargo.lock change beside a crate's); a dependent's
//!   feature-gated edge into them is left to main's feature-matrix;
//! - the flui-app/flui iOS runner gets a macOS clippy leg whenever either is in
//!   scope, like the Android runner (it needs xcrun and runs locally on macOS);
//! - rustdoc -D warnings runs over the scope with its packages' `testing`
//!   features (the doc job's flags, narrowed): a moved item's broken intra-doc
//!   link otherwise merges green and fails main's `doc` job;
//! - doctests run over the scope's library packages (the `doc-test` job,
//!   narrowed): nextest never executes them; both name the facade's catalogs
//!   when `flui` is in scope, as the tests do.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The facade's design-system features, none on by default (ADR-0088 §6):
/// every scoped step that selects `flui` names them, so the Notes example is
/// built where the facade is tested.
const FACADE_CATALOGS: [&str; 2] = ["flui/material", "flui/cupertino"];

use super::classify::{Mode, Package, Repo, Scope, Workspace};

/// The GitHub event a run was started by (`github.event_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum Event {
    #[value(name = "pull_request")]
    PullRequest,
    #[value(name = "push")]
    Push,
    #[value(name = "merge_group")]
    MergeGroup,
    #[value(name = "workflow_dispatch")]
    WorkflowDispatch,
}

/// Which of ci.yml's jobs a run starts, from least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    /// Documentation only: `checks`, `plan`.
    Docs,
    /// Repository tooling: `deps` too.
    Tooling,
    /// A pull request that compiles anything: every Linux job over the whole
    /// workspace (`HEAVY_JOBS`).
    Wide,
    /// `main`, the merge queue and a manual run: `wide` plus additional Linux
    /// jobs (`FULL_JOBS`).
    Full,
}

impl Lane {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Tooling => "tooling",
            Self::Wide => "wide",
            Self::Full => "full",
        }
    }

    /// The lane for a run: the event first, then what the change touches.
    pub(super) fn decide(event: Event, mode: Mode, heavy_required: bool) -> Self {
        match event {
            Event::Push | Event::MergeGroup | Event::WorkflowDispatch => Self::Full,
            Event::PullRequest if heavy_required => Self::Wide,
            Event::PullRequest => match mode {
                Mode::Full | Mode::Packages => Self::Wide,
                Mode::None => Self::Tooling,
                Mode::Docs => Self::Docs,
            },
        }
    }
}

/// `cargo xtask affected`'s outputs (CI's `plan` reads `lane`, `mode`,
/// `heavy_required`, `trybuild_required` and `features_required`), one field
/// per output in output order, but `features_required`, which `hack_args` decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaneArgs {
    pub(super) lane: Lane,
    pub(super) mode: String,
    pub(super) heavy_required: bool,
    /// A changed file can change a trybuild expectation: a pull request runs
    /// `test-trybuild`.
    pub(super) trybuild_required: bool,
    /// A changed line can change what `send-positions` reports: a pull
    /// request runs `api-contracts`.
    pub(super) send_positions_required: bool,
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
    /// `feature-scope` runs: `hack_args` names a package.
    pub(super) fn features_required(&self) -> bool {
        !self.hack_args.is_empty()
    }

    /// `(key, value)` in output order; booleans as `true`/`false`.
    pub(super) fn fields(&self) -> [(&'static str, String); 22] {
        let b = |v: bool| if v { "true" } else { "false" }.to_owned();
        [
            ("lane", self.lane.as_str().to_owned()),
            ("mode", self.mode.clone()),
            ("heavy_required", b(self.heavy_required)),
            ("trybuild_required", b(self.trybuild_required)),
            ("send_positions_required", b(self.send_positions_required)),
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
            // the per-feature pass of `feature-scope` runs over `hack_args`
            ("features_required", b(self.features_required())),
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

/// The lane for `scope` on `event`, and its scoped arguments.
pub(super) fn lane_args(repo: &Repo, scope: &Scope, event: Event) -> anyhow::Result<LaneArgs> {
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
        let mut feats: Vec<String> = testing.iter().map(|n| format!("{n}/testing")).collect();
        // the facade's catalogs are off by default: name them, or rustdoc and
        // the doctests skip the Material-gated half of the facade
        if has("flui") {
            feats.extend(FACADE_CATALOGS.iter().map(|f| (*f).to_owned()));
        }
        if !feats.is_empty() {
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
        // `flui` has a library, so it is selected whenever `has("flui")`
        if has("flui") {
            let _ = write!(doctest_args, " --features {}", FACADE_CATALOGS.join(","));
        }
    }

    // The per-feature clippy pass (`feature-scope`) runs over the crates the
    // change is in that have features, and every crate whose manifest
    // changed. A dependent's feature-gated edge into them is left to main's
    // feature-matrix: for a change to flui-widgets those were 13 more
    // packages, which doubled the pass.
    let mut hack_args = String::new();
    if compiles {
        let packages = &repo.workspace()?.packages;
        let featured = |n: &str| {
            packages
                .get(n)
                .is_some_and(|pkg| pkg.features.keys().any(|f| f != "default"))
        };
        hack_args = p(scope
            .seeds
            .iter()
            .map(String::as_str)
            .filter(|n| featured(n))
            .chain(scope.manifests.iter().map(String::as_str)));
    }

    Ok(LaneArgs {
        lane: Lane::decide(event, scope.mode, scope.heavy_required),
        mode: scope.mode.as_str().to_owned(),
        heavy_required: scope.heavy_required,
        trybuild_required: scope.trybuild_input.is_some(),
        send_positions_required: scope.send_positions_input.is_some(),
        reason: scope.reason.clone(),
        packages: scope.packages.join(" "),
        pkg_args: if full {
            "--workspace".to_owned()
        } else {
            p(set.iter().copied())
        },
        test_args: if full {
            "--workspace --exclude flui-native --lib --bins --tests".to_owned()
        } else {
            let names: Vec<&str> = set
                .iter()
                .copied()
                .filter(|n| *n != "flui-native")
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
            format!("--features {}", FACADE_CATALOGS.join(","))
        } else {
            String::new()
        },
        platform: has("flui-native"),
        cross_platform: has("flui-native"),
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
        lane_args(repo(), &scope(files), Event::PullRequest).expect("lane args")
    }

    /// The lane a pull request changing `files` takes.
    fn pr_lane(files: &[&str]) -> Lane {
        lane_args(repo(), &scope(files), Event::PullRequest)
            .expect("lane args")
            .lane
    }

    fn whole_workspace_pr_takes_the_wide_lane() {
        // the lane machinery is a workspace-wide input: every Linux job runs
        let a = args(&["tools/xtask/src/change_scope/classify.rs"]);
        assert_eq!((a.lane, a.mode.as_str()), (Lane::Wide, "full"));
        assert!(
            a.reason.starts_with("workspace-wide input changed: "),
            "{}",
            a.reason
        );
    }

    fn heavy_triggers_on_a_pr_take_the_wide_lane_not_full() {
        for path in ["Cargo.lock", ".github/workflows/ci.yml", "deny.toml"] {
            assert_eq!(pr_lane(&[path]), Lane::Wide, "{path}");
        }
    }

    fn events_pick_their_lane() {
        use Event::{MergeGroup, PullRequest, Push, WorkflowDispatch};
        for mode in [Mode::Docs, Mode::None, Mode::Packages, Mode::Full] {
            for heavy in [false, true] {
                assert_eq!(Lane::decide(Push, mode, heavy), Lane::Full);
                assert_eq!(Lane::decide(MergeGroup, mode, heavy), Lane::Full);
                assert_eq!(Lane::decide(WorkflowDispatch, mode, heavy), Lane::Full);
            }
            assert_eq!(Lane::decide(PullRequest, mode, true), Lane::Wide);
        }
        let pr = |mode| Lane::decide(PullRequest, mode, false);
        assert_eq!(
            [
                pr(Mode::Docs),
                pr(Mode::None),
                pr(Mode::Packages),
                pr(Mode::Full)
            ],
            [Lane::Docs, Lane::Tooling, Lane::Wide, Lane::Wide]
        );
        let names: Vec<String> = [PullRequest, Push, MergeGroup, WorkflowDispatch]
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
            ["pull_request", "push", "merge_group", "workflow_dispatch"],
            "github.event_name spellings"
        );
    }

    fn a_package_change_takes_the_wide_lane_and_scopes_its_tests() {
        let a = args(&["packages/flui-material/src/lib.rs"]);
        assert_eq!((a.lane, a.mode.as_str()), (Lane::Wide, "packages"));
        assert_eq!(
            a.test_args,
            "-p flui -p flui-material -p flui-sdk --lib --bins --tests"
        );
    }

    fn platform_only_scope_has_no_test_args() {
        let only_platform = Scope {
            mode: Mode::Packages,
            packages: vec!["flui-native".to_owned()],
            seeds: vec!["flui-native".to_owned()],
            manifests: Vec::new(),
            heavy_required: false,
            trybuild_input: None,
            send_positions_input: None,
            reason: String::new(),
        };
        let a = lane_args(repo(), &only_platform, Event::PullRequest).expect("lane args");
        assert_eq!(a.test_args, "");
        assert!(a.platform, "its own leg runs it");
    }

    fn a_changed_manifest_gets_the_per_feature_pass() {
        // a package whose manifest changed, though it has no features of its own
        assert_eq!(
            args(&["packages/flui-material/Cargo.toml"]).hack_args,
            "-p flui-material"
        );
    }

    fn cfg_gated_backends_get_their_targets() {
        let a = args(&["crates/flui-native/src/lib.rs"]);
        assert_eq!(
            (a.cross_platform, a.cross_app, a.platform),
            (true, true, true)
        );
        assert!(!a.test_args.contains("flui-native")); // its suite runs in the headless leg
        let a = args(&["packages/flui-material/src/lib.rs"]);
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

    fn wasm_scope_skips_packages_that_cannot_target_wasm() {
        let no_wasm = no_wasm_packages(repo().workspace().expect("cargo metadata"));
        assert!(no_wasm.contains("flui-cli")); // from its manifest, not restated
        assert!(!no_wasm.contains("flui-native"));
        let a = args(&["crates/flui-native/src/lib.rs"]);
        assert!(!a.wasm_args.contains("-p flui-cli"));
        assert!(a.wasm_args.contains("-p flui-native"));
    }

    fn per_feature_pass_covers_the_changed_crates() {
        // the changed crate, not its dependents: flui-widgets reaches
        // flui-assets only under `asset-images`, which main's feature-matrix
        // compiles
        let a = args(&["crates/flui-assets/src/lib.rs"]);
        assert_eq!(a.hack_args, "-p flui-assets");
        assert!(a.features_required());
        // a lock-file change makes the scope the whole workspace and still
        // names the crate beside it: its `cfg(test)` code behind a feature is
        // what the pass compiles
        let a = args(&[
            "Cargo.lock",
            "crates/flui-runtime/src/presentation_scene.rs",
        ]);
        assert_eq!(a.mode, "full");
        assert_eq!(a.hack_args, "-p flui-runtime");
        // documentation and tooling alone run no pass
        assert!(!args(&["tools/xtask/src/main.rs"]).features_required());
    }

    fn a_design_system_change_builds_the_facade_with_its_catalogs() {
        // the facade turns no feature on by default, so its edge to
        // flui-material only exists under `material`: the scoped clippy and
        // tests name the catalogs, and flui-material has no features to pass
        let a = args(&["packages/flui-material/src/lib.rs"]);
        assert_eq!(a.features, "--features flui/material,flui/cupertino");
        assert_eq!(a.hack_args, "");
    }

    fn ios_leg_when_flui_app_or_the_facade_is_in_scope() {
        assert!(args(&["crates/flui-view/src/lib.rs"]).cross_ios);
        // the facade gates code on iOS too
        assert!(args(&["src/lib.rs"]).cross_ios);
        assert!(!args(&["crates/flui-cli/src/main.rs"]).cross_ios);
    }

    fn doctests_cover_the_scopes_library_packages() {
        let a = args(&["packages/flui-material/src/lib.rs"]);
        // flui-sdk is in scope through its dev-dependency on the facade
        assert_eq!(
            a.doctest_args,
            "-p flui -p flui-material -p flui-sdk --features flui/material,flui/cupertino"
        );
        // an included README is doctest source
        assert!(
            args(&["crates/flui-animation/README.md"])
                .doctest_args
                .contains("-p flui-animation")
        );
        assert_eq!(args(&["docs/x.md"]).doctest_args, "");
    }

    fn rustdoc_covers_the_scope_with_its_testing_features() {
        let a = args(&["packages/flui-material/src/lib.rs"]);
        assert!(a.doc_args.starts_with("-p flui -p flui-material"));
        assert!(a.doc_args.contains("--features flui/testing"));
        // a testing feature of a package outside the scope would be rejected by cargo
        assert!(!a.doc_args.contains("flui-rendering/testing"));
        assert_eq!(args(&["docs/x.md"]).doc_args, "");
    }

    fn rustdoc_and_doctests_name_the_facades_catalogs_when_it_is_in_scope() {
        // neither catalog is a default feature: without them the Material-gated
        // half of the facade is neither documented nor doctested
        let a = args(&["packages/flui-material/src/lib.rs"]);
        assert!(
            a.doc_args.contains("flui/material,flui/cupertino"),
            "{}",
            a.doc_args
        );
        assert!(
            a.doctest_args
                .ends_with(" --features flui/material,flui/cupertino"),
            "{}",
            a.doctest_args
        );
        // a feature of a package outside the selection would be rejected by cargo
        // (the CLI is a package the facade does not depend on)
        let cli = args(&["crates/flui-cli/src/main.rs"]);
        assert!(!cli.packages.split(' ').any(|n| n == "flui"));
        assert!(
            !cli.doc_args.contains("flui/") && !cli.doctest_args.contains("flui/"),
            "{} / {}",
            cli.doc_args,
            cli.doctest_args
        );
    }

    fn a_binary_only_scope_gets_no_lib_flag() {
        // cargo rejects `--lib` when no selected package has a library, which
        // a change touching only xtask (a binary) would otherwise hit
        let a = args(&["tools/xtask/src/fonts.rs"]);
        assert_eq!(a.test_args, "-p xtask --bins --tests");
        assert_eq!(a.wasm_args, "", "xtask sets `wasm = false`");
        let mixed = args(&[
            "tools/xtask/src/fonts.rs",
            "crates/flui-foundation/src/lib.rs",
        ]);
        assert!(
            mixed.test_args.ends_with(" --lib --bins --tests"),
            "{}",
            mixed.test_args
        );
    }

    fn the_wide_lane_selects_the_whole_workspace() {
        let a = lane_args(repo(), &Scope::whole_workspace(), Event::Push).expect("lane args");
        assert_eq!(a.lane, Lane::Full);
        assert_eq!(
            (a.pkg_args.as_str(), a.test_args.as_str()),
            (
                "--workspace",
                "--workspace --exclude flui-native --lib --bins --tests"
            )
        );
        assert!(
            a.wasm_args
                .starts_with("--workspace --exclude flui-assets --exclude flui-cli")
        );
        assert!(
            a.doc_args.starts_with("--workspace --features ")
                && a.doc_args.contains("flui/testing")
                && a.doc_args.contains("flui/material,flui/cupertino")
        );
        assert_eq!(
            (
                a.doctest_args.as_str(),
                a.hack_args.as_str(),
                a.packages.as_str()
            ),
            (
                "--workspace --features flui/material,flui/cupertino",
                "",
                ""
            )
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
        assert_eq!(args(&["typos.toml"]).lane, Lane::Tooling);
        assert_eq!(args(&["docs/x.md"]).lane, Lane::Docs);
    }

    #[test]
    fn lane_args_contract() {
        crate::table_test::run_table(
            "lane_args_contract",
            &[
                (
                    "whole_workspace_pr_takes_the_wide_lane",
                    whole_workspace_pr_takes_the_wide_lane as fn(),
                ),
                (
                    "heavy_triggers_on_a_pr_take_the_wide_lane_not_full",
                    heavy_triggers_on_a_pr_take_the_wide_lane_not_full as fn(),
                ),
                ("events_pick_their_lane", events_pick_their_lane as fn()),
                (
                    "a_package_change_takes_the_wide_lane_and_scopes_its_tests",
                    a_package_change_takes_the_wide_lane_and_scopes_its_tests as fn(),
                ),
                (
                    "platform_only_scope_has_no_test_args",
                    platform_only_scope_has_no_test_args as fn(),
                ),
                (
                    "a_changed_manifest_gets_the_per_feature_pass",
                    a_changed_manifest_gets_the_per_feature_pass as fn(),
                ),
                (
                    "cfg_gated_backends_get_their_targets",
                    cfg_gated_backends_get_their_targets as fn(),
                ),
                (
                    "wasm_scope_skips_packages_that_cannot_target_wasm",
                    wasm_scope_skips_packages_that_cannot_target_wasm as fn(),
                ),
                (
                    "per_feature_pass_covers_the_changed_crates",
                    per_feature_pass_covers_the_changed_crates as fn(),
                ),
                (
                    "a_design_system_change_builds_the_facade_with_its_catalogs",
                    a_design_system_change_builds_the_facade_with_its_catalogs as fn(),
                ),
                (
                    "rustdoc_and_doctests_name_the_facades_catalogs_when_it_is_in_scope",
                    rustdoc_and_doctests_name_the_facades_catalogs_when_it_is_in_scope as fn(),
                ),
                (
                    "ios_leg_when_flui_app_or_the_facade_is_in_scope",
                    ios_leg_when_flui_app_or_the_facade_is_in_scope as fn(),
                ),
                (
                    "doctests_cover_the_scopes_library_packages",
                    doctests_cover_the_scopes_library_packages as fn(),
                ),
                (
                    "rustdoc_covers_the_scope_with_its_testing_features",
                    rustdoc_covers_the_scope_with_its_testing_features as fn(),
                ),
                (
                    "a_binary_only_scope_gets_no_lib_flag",
                    a_binary_only_scope_gets_no_lib_flag as fn(),
                ),
                (
                    "the_wide_lane_selects_the_whole_workspace",
                    the_wide_lane_selects_the_whole_workspace as fn(),
                ),
                (
                    "nothing_compiles_for_tooling_or_docs",
                    nothing_compiles_for_tooling_or_docs as fn(),
                ),
            ],
        );
    }
}
