//! `cargo xtask check-changed`: CI's checks, scoped to a change, locally.
//!
//! The scope is `change_scope`'s, the computation behind CI's `plan` job, over
//! this branch's diff against the base PLUS uncommitted and untracked files:
//! the changed crates and their dependents.
//!
//! By default ([`Depth::Fast`]) the run is what is cheapest to catch here:
//! `cargo fmt`, clippy over the whole scope (a change that breaks a
//! dependent's build or lints fails here) and the changed crates' tests
//! without the `trybuild` and `nested-cargo` groups, each of which compiles a
//! project of its own. CI runs everything else and is the proof.
//!
//! `--full` ([`Depth::Full`]) is local CI parity: the wide lane's jobs
//! narrowed to the scope (engine-testing clippy, rustdoc, doctests, the
//! native, wasm32 and per-feature clippies, flui-platform's suites) and every
//! test of the scope, the changed crates' first and the dependents' only if
//! those pass; a step whose target or tool this host lacks is skipped with a
//! message naming the fix, since CI runs it anyway.
//!
//! Every nextest pass keeps the whole scope's cargo arguments and picks tests
//! with a filterset, so feature resolution matches clippy's and CI's and the
//! `--full` passes share one build; the dependents' test binaries are still
//! built, only not run.
//!
//! The compiling steps run once per input set: a run whose tree (untracked
//! files included), merge base, machine, toolchain, Cargo configuration,
//! environment (all but a few result-neutral variables) and step list already
//! passed is skipped with a line naming the earlier run, and a run superseded
//! while it waited for the host lock, by a newer `check-changed` of the same
//! checkout asking the same of a newer tree, exits 3 without claiming its
//! tree (`crate::verified`; `--no-cache` runs regardless). The scope computed
//! before the wait only probes that record: under the lock the checkout is
//! snapshotted first and, if it changed while the run waited, scoped and
//! format-checked again; the run is recorded only if the checkout still
//! matches that snapshot when the steps end.
//! While the steps hold the lock they use the machine
//! (`crate::host_lock::Parallelism`). The depth changes the step list, so a
//! `--full` run and a fast one are recorded apart.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use super::exec::{Cmd, Host, Runner, Step, installed, installed_targets};
use super::{
    WASM_TARGET, driver_tests, engine_testing_clippy, hack_passes, native, reload_fixture_build,
    wasm_facade_check,
};
use crate::change_scope;
use crate::util::repo_root;
use crate::verified::{Checks, Key, RunInputs, Snapshot, Verifier};

/// How much of CI's work a run repeats locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Depth {
    /// `cargo fmt`, clippy over the scope, and the changed crates' tests
    /// without the groups that compile projects of their own.
    Fast,
    /// `--full`: every step CI's lane runs for the scope, the dependents'
    /// tests included.
    Full,
}

/// The tests a fast run leaves out: each compiles a project of its own
/// (`.config/nextest.toml` names the groups), so together they outweigh the
/// rest of a crate's suite many times over.
const NO_NESTED_COMPILES: &str = "not (group(trybuild) | group(nested-cargo))";

/// Which of the scope's crates the run tests.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tests {
    /// Every crate in scope: it holds no dependents, or it is the whole
    /// workspace, which names no changed crates to split by.
    Scope,
    /// Only these changed crates; their dependents' tests are left to CI.
    Changed(Vec<String>),
    /// `--full`: these changed crates' tests, then, if they pass, the
    /// dependents'.
    ChangedFirst(Vec<String>),
}

impl Tests {
    /// Whether `package`'s tests run.
    fn runs(&self, package: &str) -> bool {
        match self {
            Self::Scope | Self::ChangedFirst(_) => true,
            Self::Changed(changed) => changed.iter().any(|name| name == package),
        }
    }
}

/// The scope's inputs: `change_scope`'s `affected` outputs, typed, and which
/// of its crates are tested.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lane {
    /// `docs`, `none`, `packages` or `full`.
    mode: String,
    heavy_required: bool,
    reason: String,
    /// The scope, space-separated; empty for the whole workspace.
    packages: String,
    pkg_args: String,
    test_args: String,
    features: String,
    platform: bool,
    wasm_args: String,
    wasm_facade: bool,
    hack_args: String,
    doc_args: String,
    doctest_args: String,
    tests: Tests,
    depth: Depth,
}

impl Lane {
    /// Reads the `(key, value)` pairs `change_scope` produces and the
    /// packages a changed file belongs to, for a run of `depth`.
    fn from_fields(
        fields: impl IntoIterator<Item = (&'static str, String)>,
        seeds: &[String],
        depth: Depth,
    ) -> anyhow::Result<Self> {
        let mut fields: BTreeMap<&str, String> = fields.into_iter().collect();
        let mut text = |key: &str| {
            fields
                .remove(key)
                .with_context(|| format!("BUG: change_scope gave no `{key}`"))
        };
        let flag = |key: &str, value: String| match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => bail!("BUG: change_scope gave `{key}={other}`, not a boolean"),
        };
        Ok(Self {
            mode: text("mode")?,
            heavy_required: flag("heavy_required", text("heavy_required")?)?,
            reason: text("reason")?,
            packages: text("packages")?,
            pkg_args: text("pkg_args")?,
            test_args: text("test_args")?,
            features: text("features")?,
            platform: flag("platform", text("platform")?)?,
            wasm_args: text("wasm_args")?,
            wasm_facade: flag("wasm_facade", text("wasm_facade")?)?,
            hack_args: text("hack_args")?,
            doc_args: text("doc_args")?,
            doctest_args: text("doctest_args")?,
            tests: Tests::Scope,
            depth,
        }
        .with_seeds(seeds, depth))
    }

    /// The lane, run at `depth`, testing only the `seeds` in its scope, or at
    /// [`Depth::Full`] the seeds first and then the rest, when the scope also
    /// holds crates that are not seeds. A scope holding no seed (a change
    /// outside every crate that still selects some, a workspace manifest say)
    /// is tested whole.
    fn with_seeds(mut self, seeds: &[String], depth: Depth) -> Self {
        let scope: Vec<&str> = self.packages.split_whitespace().collect();
        let changed: Vec<String> = seeds
            .iter()
            .filter(|s| scope.contains(&s.as_str()))
            .cloned()
            .collect();
        self.depth = depth;
        self.tests = if !changed.is_empty() && changed.len() < scope.len() {
            match depth {
                Depth::Fast => Tests::Changed(changed),
                Depth::Full => Tests::ChangedFirst(changed),
            }
        } else {
            Tests::Scope
        };
        self
    }

    /// The crates in scope whose tests are left to CI.
    fn untested(&self) -> Vec<&str> {
        self.packages
            .split_whitespace()
            .filter(|name| !self.tests.runs(name))
            .collect()
    }

    /// Whether the change needs no compiling step (`docs` or `none`).
    fn compiles_nothing(&self) -> bool {
        matches!(self.mode.as_str(), "docs" | "none")
    }

    /// The scope as the run reports it.
    fn scope(&self) -> &str {
        if self.packages.is_empty() {
            "<whole workspace>"
        } else {
            &self.packages
        }
    }

    /// Whether flui-engine is in scope (an empty list is the whole workspace).
    fn has_engine(&self) -> bool {
        self.packages.is_empty() || self.packages.split_whitespace().any(|p| p == "flui-engine")
    }
}

/// `path` made absolute against `cwd` and resolved the way `realpath` does,
/// without requiring it to exist: symlinks in its longest existing prefix are
/// resolved, `.` and `..` after it are applied lexically.
fn resolve(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = cwd.join(path);
    let (base, rest) = absolute
        .ancestors()
        .find_map(|dir| {
            let real = std::fs::canonicalize(dir).ok()?;
            let rest = absolute.strip_prefix(dir).ok()?.to_path_buf();
            Some((real, rest))
        })
        .unwrap_or_else(|| (PathBuf::new(), absolute.clone()));
    let mut resolved = base;
    for component in rest.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

/// Whether a `CARGO_TARGET_DIR` of `target` (relative to `cwd`) lies outside
/// `checkout`.
fn outside_checkout(target: &Path, cwd: &Path, checkout: &Path) -> bool {
    !resolve(target, cwd).starts_with(resolve(checkout, cwd))
}

/// `args` (`-p NAME` pairs, `--features PKG/FEATURE,...` lists and other
/// flags, as `change_scope` renders them) narrowed to the packages `tests`
/// runs: a feature of a package dropped is dropped with it, since cargo
/// rejects one of an unselected package. Empty when no package is left.
fn tested_packages(args: &str, tests: &Tests) -> String {
    let mut kept = Vec::new();
    let mut any_package = false;
    let mut tokens = args.split_whitespace();
    while let Some(token) = tokens.next() {
        match token {
            "-p" | "--package" => {
                let name = tokens.next().unwrap_or_default();
                if tests.runs(name) {
                    any_package = true;
                    kept.extend([token.to_owned(), name.to_owned()]);
                }
            }
            "--features" => {
                let features: Vec<&str> = tokens
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|feature| feature.split_once('/').is_none_or(|(p, _)| tests.runs(p)))
                    .collect();
                if !features.is_empty() {
                    kept.extend([token.to_owned(), features.join(",")]);
                }
            }
            "--workspace" => {
                any_package = true;
                kept.push(token.to_owned());
            }
            other => kept.push(other.to_owned()),
        }
    }
    if any_package {
        kept.join(" ")
    } else {
        String::new()
    }
}

/// The compiling steps for `lane`, after `cargo fmt`. A fast lane runs only
/// clippy and its tests; a full one also uses `targets` (the installed rustup
/// targets), `have_hack` (whether cargo-hack is installed) and `native` (the
/// native per-target steps).
fn plan(
    lane: &Lane,
    host: Host,
    targets: &BTreeSet<String>,
    have_hack: bool,
    native: &[Step],
) -> Vec<Step> {
    let have = |target: &str| targets.contains(target);
    let full = lane.depth == Depth::Full;
    let mut steps: Vec<Step> = vec![
        Cmd::cargo(["clippy"])
            .split(&lane.pkg_args)
            .args(["--all-targets", "--locked", "--", "-D", "warnings"])
            .into(),
    ];
    if full && lane.has_engine() {
        steps.push(engine_testing_clippy().into());
    }
    if !full {
        steps.push(Step::Note(
            "check-changed: fast: no trybuild or nested-cargo tests, rustdoc, doctests, cross or per-feature clippy (CI runs them; --full runs them here)"
                .to_owned(),
        ));
    }
    if !lane.test_args.is_empty() {
        let split_driver = host == Host::Windows && lane.packages.is_empty();
        if split_driver {
            steps.push(driver_tests().into());
        }
        const FIXTURE: &str = "flui-hot-reload-lifecycle-fixture";
        if lane.packages.is_empty()
            || (lane.packages.split_whitespace().any(|name| name == FIXTURE)
                && lane.tests.runs(FIXTURE))
        {
            steps.push(reload_fixture_build().into());
        }
        // Every pass has the whole scope's cargo arguments, so it builds once
        // with the features CI unifies; a filterset only picks the tests.
        let tests = |filter: Option<String>| {
            let mut tests = Cmd::cargo(["nextest", "run"])
                .split(&lane.test_args)
                .args(["--locked", "--no-fail-fast", "--no-tests=pass"])
                .split(&lane.features);
            if split_driver {
                tests = tests.args(["--exclude", "xtask"]);
            }
            let filter = match (filter, full) {
                (filter, true) => filter,
                (None, false) => Some(NO_NESTED_COMPILES.to_owned()),
                (Some(filter), false) => Some(format!("{filter} & {NO_NESTED_COMPILES}")),
            };
            if let Some(filter) = filter {
                tests = tests.args(["-E".to_owned(), filter]);
            }
            Step::from(tests)
        };
        let filter = |changed: &[String]| {
            let packages: Vec<String> = changed
                .iter()
                .map(|name| format!("package(={name})"))
                .collect();
            match packages.as_slice() {
                [one] => one.clone(),
                many => format!("({})", many.join(" | ")),
            }
        };
        match &lane.tests {
            Tests::Scope => steps.push(tests(None)),
            Tests::Changed(changed) => {
                steps.push(Step::Note(format!(
                    "check-changed: the changed crates' tests: {}; the dependents' run in CI (--full runs them here): {}",
                    changed.join(" "),
                    lane.untested().join(" ")
                )));
                steps.push(tests(Some(filter(changed))));
            }
            Tests::ChangedFirst(changed) => {
                let changed_filter = filter(changed);
                steps.push(Step::Note(format!(
                    "check-changed: the changed crates' tests first: {}",
                    changed.join(" ")
                )));
                steps.push(tests(Some(changed_filter.clone())));
                steps.push(Step::Note(
                    "check-changed: then the dependents' tests (--full)".to_owned(),
                ));
                let dependents = if changed.len() == 1 {
                    format!("not ({changed_filter})")
                } else {
                    format!("not {changed_filter}")
                };
                steps.push(tests(Some(dependents)));
            }
        }
    }
    // flui-platform's suite needs a display server on Linux and is left to
    // CI elsewhere; its compiler checks are nested compiles, run with --full.
    let platform_suite = || {
        if host == Host::Linux {
            super::platform_suite_linux()
                .args(["-E", "not (group(trybuild))"])
                .into()
        } else {
            Step::Note(
                "check-changed: flui-platform is in scope, but its native suite needs xvfb-run (Linux); CI runs it"
                    .to_owned(),
            )
        }
    };
    let tests_platform = lane.platform && lane.tests.runs("flui-platform");
    if !full {
        if tests_platform {
            steps.push(platform_suite());
        }
        return steps;
    }
    // rustdoc stays over the whole scope: like clippy it only type-checks, and
    // a dependent's intra-doc link to an item the change renamed or removed
    // fails `-D warnings` there.
    if !lane.doc_args.is_empty() {
        steps.push(
            Cmd::cargo(["doc"])
                .split(&lane.doc_args)
                .args(["--no-deps", "--locked", "--document-private-items"])
                .env("RUSTDOCFLAGS", "-D warnings")
                .into(),
        );
    }
    // Doctests are tests: each example compiles and links a binary of its
    // own, so only the tested crates' run.
    let doctest_args = tested_packages(&lane.doctest_args, &lane.tests);
    if !doctest_args.is_empty() {
        steps.push(
            Cmd::cargo(["test"])
                .split(&doctest_args)
                .args(["--locked", "--doc"])
                .into(),
        );
    }
    steps.extend_from_slice(native);
    if !lane.wasm_args.is_empty() {
        if have(WASM_TARGET) {
            steps.push(
                Cmd::cargo(["clippy"])
                    .split(&lane.wasm_args)
                    .args(["--locked", "--target", WASM_TARGET])
                    .args(["--", "-D", "warnings"])
                    .into(),
            );
            if lane.wasm_facade {
                steps.push(wasm_facade_check().into());
            }
        } else {
            steps.push(Step::Note(format!(
                "check-changed: skipped wasm32 (rustup target add {WASM_TARGET}; CI runs it)"
            )));
        }
    }
    if !lane.hack_args.is_empty() {
        if have_hack {
            steps.extend(hack_passes(&lane.hack_args).map(Step::from));
        } else {
            steps.push(Step::Note(
                "check-changed: skipped per-feature clippy of changed manifests (cargo install --locked cargo-hack; CI runs it)"
                    .to_owned(),
            ));
        }
    }
    if tests_platform {
        steps.push(super::platform_compiler_tests().into());
        steps.push(platform_suite());
    }
    steps
}

/// Admit a local native command only when its target and native toolchain exist.
fn local_native_step(
    target: &str,
    step: Step,
    host: Host,
    targets: &BTreeSet<String>,
    have_tool: impl Fn(&str) -> bool,
    have_android_ndk: impl Fn() -> (bool, bool),
) -> Step {
    if targets.contains(target) {
        if target == super::ANDROID_TARGET {
            let (compiler, archiver) = have_android_ndk();
            if !compiler || !archiver {
                return Step::Note(format!(
                    "check-changed: skipped native source checks on {target} (install the Android NDK and configure CC_aarch64_linux_android and AR_aarch64_linux_android; CI runs it)"
                ));
            }
        }
        let wrapper = if target == super::WINDOWS_TARGET && host != Host::Windows {
            Some("cargo-xwin")
        } else if target == super::MACOS_TARGET && host != Host::MacOs {
            Some("cargo-zigbuild")
        } else {
            None
        };
        if let Some(tool) = wrapper
            && !have_tool(tool)
        {
            return Step::Note(format!(
                "check-changed: skipped native source checks on {target} (cargo install --locked {tool}; CI runs it)"
            ));
        }
        step
    } else {
        Step::Note(format!(
            "check-changed: skipped native source checks on {target} (rustup target add {target}; CI runs it)"
        ))
    }
}

/// `cargo xtask check-changed`.
pub(super) fn run(
    runner: Runner,
    base: &str,
    depth: Depth,
    use_cache: bool,
) -> anyhow::Result<ExitCode> {
    // Every task's cargo runs from the repository root, so a relative
    // CARGO_TARGET_DIR is relative to it.
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        let root = repo_root();
        if outside_checkout(Path::new(&target), &root, &root) {
            eprintln!(
                "check-changed: CARGO_TARGET_DIR={} is outside this checkout; a target shared between worktrees links stale code (docs/testing.md). Unset it.",
                Path::new(&target).display()
            );
            return Ok(ExitCode::from(2));
        }
    }
    if runner.dry_run {
        let scoped = prepare(runner, base, depth)?;
        if !scoped.lane.compiles_nothing() {
            runner.steps(&scoped.steps)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    // Taken before planning, so a plan never stands for an older tree than
    // the snapshot it is keyed with (`crate::verified::Verifier::verify`).
    let root = repo_root();
    let before = Snapshot::of(&root, base);
    let scoped = prepare(runner, base, depth)?;
    if scoped.lane.compiles_nothing() {
        return Ok(ExitCode::SUCCESS);
    }
    verify_once(runner, base, depth, before, scoped, use_cache)
}

/// Scopes the checkout as it is now, reports the scope, and checks
/// formatting, which every plan stands on: before the wait as a fast
/// failure, and again under the lock for a tree that changed meanwhile.
fn prepare(runner: Runner, base: &str, depth: Depth) -> anyhow::Result<Scoped> {
    let scoped = Scoped::now(base, depth)?;
    let lane = &scoped.lane;
    println!("check-changed: {}", lane.reason);
    if lane.heavy_required {
        println!(
            "check-changed: this change reaches beyond the scoped checks (see the reason above): CI checks the whole workspace; locally, consider cargo xtask ci-full"
        );
    }
    runner.run(&Cmd::cargo(["fmt", "--all", "--", "--check"]))?;
    if lane.compiles_nothing() {
        println!("check-changed: nothing to compile ({})", lane.mode);
    } else {
        println!("check-changed: packages: {}", lane.scope());
    }
    Ok(scoped)
}

/// The scope of the checkout as it is when computed, and the steps it runs.
struct Scoped {
    lane: Lane,
    /// Empty when the lane compiles nothing.
    steps: Vec<Step>,
}

impl Scoped {
    /// Classifies the checkout's change against `base` and plans its steps
    /// for this host at `depth`.
    fn now(base: &str, depth: Depth) -> anyhow::Result<Self> {
        let scoped = change_scope::worktree_lane(base)?;
        let lane = Lane::from_fields(scoped.fields, &scoped.seeds, depth)?;
        if lane.compiles_nothing() {
            return Ok(Self {
                lane,
                steps: Vec::new(),
            });
        }
        if depth == Depth::Fast {
            // A fast plan names no target, tool or native step.
            let steps = plan(&lane, Host::current(), &BTreeSet::new(), false, &[]);
            return Ok(Self { lane, steps });
        }
        let have_hack = !lane.hack_args.is_empty() && installed("cargo", &["hack", "--version"]);
        let targets = installed_targets();
        let mut native_steps = Vec::new();
        for (target, step) in native::plans(
            &repo_root(),
            &lane.packages,
            Host::current(),
            native::FeatureSet::RequiredTargets,
        )? {
            native_steps.push(local_native_step(
                target,
                step,
                Host::current(),
                &targets,
                |tool| installed(tool, &["--version"]),
                || {
                    (
                        crate::doctor::android_compiler_available(&crate::doctor::android_tool(
                            "CC", "clang",
                        )),
                        crate::doctor::android_archiver_available(&crate::doctor::android_tool(
                            "AR", "llvm-ar",
                        )),
                    )
                },
            ));
        }
        let steps = plan(&lane, Host::current(), &targets, have_hack, &native_steps);
        Ok(Self { lane, steps })
    }

    /// The steps as the key hashes them, one line each.
    fn rendered(&self) -> String {
        self.steps
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `check-changed` as `crate::verified` runs it: the checkout's snapshot,
/// its key, a fresh plan, and the plan's steps.
struct CheckChanged<'a> {
    runner: Runner,
    base: &'a str,
    depth: Depth,
    root: PathBuf,
    inputs: RunInputs,
}

impl Checks for CheckChanged<'_> {
    type Plan = Scoped;

    fn snapshot(&mut self) -> anyhow::Result<Snapshot> {
        Snapshot::of(&self.root, self.base)
    }

    fn key(&self, snapshot: &Snapshot, plan: &Scoped) -> Key {
        self.inputs.key(snapshot, &plan.rendered())
    }

    fn reread_inputs(&mut self) {
        self.inputs = RunInputs::of_this_run(&self.root);
    }

    fn replan(&mut self) -> anyhow::Result<Scoped> {
        prepare(self.runner, self.base, self.depth)
    }

    fn run(&mut self, plan: &Scoped) -> anyhow::Result<()> {
        self.runner.steps(&plan.steps)
    }
}

/// Runs the scope's steps once per verified key (`crate::verified`): skipped
/// when the tree, base, toolchain, environment and steps already passed.
/// Only the compiling steps wait for other heavy runs (`main` leaves this
/// command's lock to here). `planned` is the scope made after `before` was
/// taken; under the lock a changed checkout is prepared again.
fn verify_once(
    runner: Runner,
    base: &str,
    depth: Depth,
    before: anyhow::Result<Snapshot>,
    planned: Scoped,
    use_cache: bool,
) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let mut checks = CheckChanged {
        runner,
        base,
        depth,
        inputs: RunInputs::of_this_run(&root),
        root,
    };
    let settings = crate::host_lock::LockSettings::from_env();
    let mut out = std::io::stdout();
    let outcome = Verifier::new(settings.state_dir(), &checks.root, use_cache, &mut out).verify(
        &mut checks,
        before,
        planned,
        crate::host_lock::HeavyRunLock::for_this_run,
    )?;
    Ok(outcome.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(ToString::to_string).collect()
    }

    /// The lane `affected` computes for a change to flui-material.
    fn material() -> Lane {
        Lane {
            mode: "packages".to_owned(),
            heavy_required: false,
            reason: "changed: flui-material; plus 2 dependents".to_owned(),
            packages: "flui flui-material flui-web-counter".to_owned(),
            pkg_args: "-p flui -p flui-material -p flui-web-counter".to_owned(),
            test_args: "-p flui -p flui-material -p flui-web-counter --lib --bins --tests"
                .to_owned(),
            features: "--features flui/cupertino".to_owned(),
            platform: false,
            wasm_args: "-p flui -p flui-material -p flui-web-counter --lib --bins".to_owned(),
            wasm_facade: true,
            hack_args: "-p flui-material".to_owned(),
            doc_args: "-p flui -p flui-material -p flui-web-counter --features flui/testing"
                .to_owned(),
            doctest_args: "-p flui -p flui-material --features flui/material,flui/cupertino"
                .to_owned(),
            tests: Tests::Scope,
            depth: Depth::Full,
        }
    }

    fn all_targets() -> BTreeSet<String> {
        super::super::PLATFORM_TARGETS
            .into_iter()
            .chain([WASM_TARGET])
            .map(str::to_owned)
            .collect()
    }

    fn missing_cross_wrappers_leave_native_steps_runnable() {
        for (target, tool) in [
            (super::super::WINDOWS_TARGET, "cargo-xwin"),
            (super::super::MACOS_TARGET, "cargo-zigbuild"),
        ] {
            for available in [false, true] {
                let step = local_native_step(
                    target,
                    Cmd::cargo(["clippy"]).into(),
                    Host::Linux,
                    &all_targets(),
                    |requested| {
                        assert_eq!(requested, tool);
                        available
                    },
                    || panic!("non-Android target must not probe the NDK"),
                );
                let text = step.to_string();
                if available {
                    assert_eq!(text, "$ cargo clippy");
                } else {
                    assert!(text.contains("skipped") && text.contains(tool), "{text}");
                }
            }
        }
    }

    fn android_without_an_ndk_leaves_other_native_checks_runnable() {
        let dir = tempfile::tempdir().expect("temporary missing-tool directory");
        let missing_archiver = dir.path().join("missing-android-archiver");
        for (compiler, archiver) in [(false, false), (true, false), (false, true), (true, true)] {
            let step = local_native_step(
                super::super::ANDROID_TARGET,
                Cmd::cargo(["clippy"]).into(),
                Host::Linux,
                &all_targets(),
                |_| panic!("Android does not need a cross wrapper"),
                || {
                    (
                        compiler,
                        crate::doctor::android_archiver_available(if archiver {
                            std::ffi::OsStr::new("cargo")
                        } else {
                            missing_archiver.as_os_str()
                        }),
                    )
                },
            );
            let text = step.to_string();
            if compiler && archiver {
                assert_eq!(text, "$ cargo clippy");
            } else {
                assert!(
                    text.contains("skipped") && text.contains("Android NDK"),
                    "{text}"
                );
            }
        }
    }

    fn a_package_change_runs_the_scoped_commands() {
        assert_eq!(
            lines(&plan(&material(), Host::Linux, &all_targets(), true, &[])),
            [
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --all-targets --locked -- -D warnings",
                "$ cargo nextest run -p flui -p flui-material -p flui-web-counter --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino",
                "$ RUSTDOCFLAGS='-D warnings' cargo doc -p flui -p flui-material -p flui-web-counter --features flui/testing --no-deps --locked --document-private-items",
                "$ cargo test -p flui -p flui-material --features flui/material,flui/cupertino --locked --doc",
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --lib --bins --locked --target wasm32-unknown-unknown -- -D warnings",
                "$ cargo check -p flui --locked --target wasm32-unknown-unknown --no-default-features --features hot-reload",
                "$ cargo hack clippy -p flui-material --locked --each-feature --keep-going -- -D warnings",
                "$ cargo hack clippy -p flui-material --locked --each-feature --keep-going --tests --benches --examples -- -D warnings",
            ]
        );
        let native = [Step::Note("discovered native coverage".to_owned())];
        let on_mac = lines(&plan(
            &material(),
            Host::MacOs,
            &all_targets(),
            true,
            &native,
        ));
        assert!(on_mac.contains(&"discovered native coverage".to_owned()));
    }

    fn the_whole_workspace_also_lints_the_engine_testing_code() {
        let mut lane = material();
        lane.packages = String::new();
        lane.pkg_args = "--workspace".to_owned();
        let steps = lines(&plan(&lane, Host::Linux, &all_targets(), true, &[]));
        assert_eq!(
            &steps[..2],
            [
                "$ cargo clippy --workspace --all-targets --locked -- -D warnings",
                "$ cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings",
            ]
        );
        assert!(steps.iter().any(
            |step| step == "$ cargo build -p flui-hot-reload-lifecycle-fixture --lib --locked"
        ));
        lane.packages = "flui-engine-extra".to_owned();
        assert!(!lane.has_engine());
        lane.packages = "flui flui-engine".to_owned();
        assert!(lane.has_engine());
    }

    /// By default a change below its dependents runs exactly clippy over the
    /// whole scope and the changed crates' tests without the nested-compile
    /// groups. The nextest pass keeps the scope's cargo arguments, so it
    /// shares clippy's feature resolution and CI's.
    fn a_low_layer_change_by_default_lints_the_scope_and_tests_the_changed_crates() {
        let fast = |seeds: &[&str]| {
            let seeds: Vec<String> = seeds.iter().map(|s| (*s).to_owned()).collect();
            let lane = material().with_seeds(&seeds, Depth::Fast);
            lines(&plan(&lane, Host::Linux, &all_targets(), true, &[]))
        };
        let nextest = "$ cargo nextest run -p flui -p flui-material -p flui-web-counter --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino";
        assert_eq!(
            fast(&["flui-material"]),
            [
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --all-targets --locked -- -D warnings".to_owned(),
                "check-changed: fast: no trybuild or nested-cargo tests, rustdoc, doctests, cross or per-feature clippy (CI runs them; --full runs them here)".to_owned(),
                "check-changed: the changed crates' tests: flui-material; the dependents' run in CI (--full runs them here): flui flui-web-counter".to_owned(),
                format!("{nextest} -E 'package(=flui-material) & not (group(trybuild) | group(nested-cargo))'"),
            ]
        );
        assert_eq!(
            fast(&["flui", "flui-material"]).last().map(String::as_str),
            Some(
                format!("{nextest} -E '(package(=flui) | package(=flui-material)) & not (group(trybuild) | group(nested-cargo))'")
                    .as_str()
            ),
            "several changed crates are one group under the exclusion"
        );
    }

    /// `--full` is CI's lane over the scope: every step of a whole-scope run,
    /// with the changed crates' tests first and the dependents' after (one
    /// build: both passes have the scope's cargo arguments), nested-compile
    /// groups included.
    fn full_runs_the_whole_lane_and_the_dependents_tests() {
        let seeds = ["flui-material".to_owned()];
        let full = lines(&plan(
            &material().with_seeds(&seeds, Depth::Full),
            Host::Linux,
            &all_targets(),
            true,
            &[],
        ));
        let whole = lines(&plan(&material(), Host::Linux, &all_targets(), true, &[]));
        let tests = whole
            .iter()
            .position(|step| step.starts_with("$ cargo nextest"))
            .expect("the lane tests");
        assert_eq!(full[..tests], whole[..tests], "one clippy over the scope");
        let pass = &whole[tests];
        assert_eq!(
            full[tests..tests + 4],
            [
                "check-changed: the changed crates' tests first: flui-material".to_owned(),
                format!("{pass} -E 'package(=flui-material)'"),
                "check-changed: then the dependents' tests (--full)".to_owned(),
                format!("{pass} -E 'not (package(=flui-material))'"),
            ]
        );
        assert_eq!(
            full[tests + 4..],
            whole[tests + 1..],
            "rustdoc, the scope's doctests, wasm32 and per-feature clippy"
        );
        let mut two = material();
        two.packages = "flui flui-material flui-web-counter flui-other".to_owned();
        let two = lines(&plan(
            &two.with_seeds(
                &["flui".to_owned(), "flui-material".to_owned()],
                Depth::Full,
            ),
            Host::Linux,
            &all_targets(),
            true,
            &[],
        ));
        assert!(
            two.contains(&format!(
                "{pass} -E 'not (package(=flui) | package(=flui-material))'"
            )),
            "{two:?}"
        );
    }

    /// A dependent's own suites (flui-platform's, the reload fixture's) are
    /// tests too: left to CI unless `--full`. A changed flui-platform's
    /// suite runs at either depth; its compiler checks only with `--full`.
    fn a_dependents_platform_and_fixture_suites_run_only_when_full() {
        let lane = Lane {
            packages: "flui-hot-reload-lifecycle-fixture flui-platform flui-platform-api"
                .to_owned(),
            platform: true,
            ..material()
        };
        let suites = |seed: &str, depth| {
            let steps = lines(&plan(
                &lane.clone().with_seeds(&[seed.to_owned()], depth),
                Host::Linux,
                &all_targets(),
                true,
                &[],
            ));
            [
                "-p flui-hot-reload-lifecycle-fixture --lib",
                "-p flui-platform --test compiler_guards",
                "xvfb-run",
            ]
            .map(|suite| steps.iter().any(|step| step.contains(suite)))
        };
        assert_eq!(suites("flui-platform-api", Depth::Fast), [false; 3]);
        assert_eq!(suites("flui-platform-api", Depth::Full), [true; 3]);
        assert_eq!(suites("flui-platform", Depth::Fast), [false, false, true]);
        assert_eq!(suites("flui-platform", Depth::Full), [true; 3]);
    }

    /// A docs-only change compiles nothing at either depth; an xtask-only
    /// change, which has no dependents, tests xtask whole by default.
    fn docs_and_xtask_changes_keep_their_scope() {
        let docs = Lane {
            mode: "docs".to_owned(),
            ..material()
        };
        for depth in [Depth::Fast, Depth::Full] {
            assert!(docs.clone().with_seeds(&[], depth).compiles_nothing());
        }
        let xtask = Lane {
            packages: "xtask".to_owned(),
            pkg_args: "-p xtask".to_owned(),
            test_args: "-p xtask --bins --tests".to_owned(),
            features: String::new(),
            ..material()
        }
        .with_seeds(&["xtask".to_owned()], Depth::Fast);
        assert_eq!(
            lines(&plan(&xtask, Host::Linux, &all_targets(), true, &[])),
            [
                "$ cargo clippy -p xtask --all-targets --locked -- -D warnings",
                "check-changed: fast: no trybuild or nested-cargo tests, rustdoc, doctests, cross or per-feature clippy (CI runs them; --full runs them here)",
                "$ cargo nextest run -p xtask --bins --tests --locked --no-fail-fast --no-tests=pass -E 'not (group(trybuild) | group(nested-cargo))'",
            ]
        );
    }

    /// With nothing to narrow (only changed crates in scope, the whole
    /// workspace, no changed crate in scope, a docs-only or xtask-only
    /// change), every crate in scope is tested, at either depth.
    fn a_scope_without_dependents_tests_every_crate_in_it() {
        let docs = Lane {
            mode: "docs".to_owned(),
            packages: String::new(),
            pkg_args: String::new(),
            test_args: String::new(),
            features: String::new(),
            wasm_args: String::new(),
            wasm_facade: false,
            hack_args: String::new(),
            doc_args: String::new(),
            doctest_args: String::new(),
            ..material()
        };
        let xtask = Lane {
            packages: "xtask".to_owned(),
            pkg_args: "-p xtask".to_owned(),
            test_args: "-p xtask --bins --tests".to_owned(),
            features: String::new(),
            wasm_args: String::new(),
            wasm_facade: false,
            hack_args: String::new(),
            doc_args: "-p xtask".to_owned(),
            doctest_args: String::new(),
            ..material()
        };
        let mut workspace = material();
        workspace.packages = String::new();
        let cases = [
            (
                material(),
                vec!["flui", "flui-material", "flui-web-counter"],
            ),
            (workspace, vec!["flui-material"]),
            (material(), vec!["flui-unrelated"]),
            (docs, vec![]),
            (xtask, vec!["xtask"]),
        ];
        for (lane, seeds) in cases {
            let seeds: Vec<String> = seeds.into_iter().map(str::to_owned).collect();
            for depth in [Depth::Fast, Depth::Full] {
                let whole = Lane {
                    depth,
                    ..lane.clone()
                };
                let whole = lines(&plan(&whole, Host::Linux, &all_targets(), true, &[]));
                let narrowed = lane.clone().with_seeds(&seeds, depth);
                assert_eq!(narrowed.tests, Tests::Scope, "{seeds:?} {depth:?}");
                assert_eq!(
                    lines(&plan(&narrowed, Host::Linux, &all_targets(), true, &[])),
                    whole,
                    "{seeds:?} {depth:?}"
                );
            }
        }
    }

    fn the_lane_reads_every_field_change_scope_gives() {
        let fields = change_scope::worktree_lane("HEAD")
            .expect("classifies")
            .fields;
        let lane =
            Lane::from_fields(fields.clone(), &[], Depth::Fast).expect("every field is there");
        assert!(["docs", "none", "packages", "full"].contains(&lane.mode.as_str()));
        let mut missing = fields.clone();
        missing.retain(|(key, _)| *key != "wasm_facade");
        let error = Lane::from_fields(missing, &[], Depth::Fast).expect_err("a field is missing");
        assert!(error.to_string().contains("wasm_facade"), "{error}");
        let mut bad = fields;
        for (key, value) in &mut bad {
            if *key == "platform" {
                *value = "yes".to_owned();
            }
        }
        assert!(Lane::from_fields(bad, &[], Depth::Fast).is_err());
    }

    fn windows_workspace_testing_preserves_the_driver_suite() {
        let mut lane = material();
        lane.packages = String::new();
        lane.pkg_args = "--workspace".to_owned();
        lane.test_args = "--workspace --exclude flui-platform --lib --bins --tests".to_owned();
        let nextest = |host| {
            lines(&plan(&lane, host, &all_targets(), true, &[]))
                .into_iter()
                .filter(|line| line.starts_with("$ cargo nextest run"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            nextest(Host::Windows),
            [
                "$ cargo nextest run -p xtask --bins --tests --locked --no-fail-fast --no-tests=pass",
                "$ cargo nextest run --workspace --exclude flui-platform --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino --exclude xtask",
            ]
        );
        for host in [Host::Linux, Host::MacOs] {
            assert_eq!(
                nextest(host),
                [
                    "$ cargo nextest run --workspace --exclude flui-platform --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino",
                ]
            );
        }
    }

    fn platform_compiler_checks_run_only_when_in_scope_on_every_host() {
        for host in [Host::Linux, Host::Windows, Host::MacOs, Host::Other] {
            let mut lane = material();
            for in_scope in [false, true] {
                lane.platform = in_scope;
                let commands = lines(&plan(&lane, host, &all_targets(), true, &[]));
                assert_eq!(
                    commands
                        .iter()
                        .filter(|line| line.contains("-p flui-platform --test compiler_guards"))
                        .count(),
                    usize::from(in_scope),
                    "{host:?} in scope {in_scope}: {commands:?}"
                );
                if in_scope && host == Host::Linux {
                    let native = commands
                        .iter()
                        .find(|line| line.contains("xvfb-run"))
                        .expect("Linux native suite remains scheduled");
                    assert!(native.ends_with("-E 'not (group(trybuild))'"), "{native}");
                }
            }
        }
    }

    fn a_target_dir_outside_the_checkout_is_refused() {
        let scratch = std::env::temp_dir().join(format!("xtask-checkout-{}", std::process::id()));
        let checkout = scratch.join("flui");
        std::fs::create_dir_all(&checkout).expect("mkdir");
        let inside = [
            Path::new("target"),
            Path::new("target/nested/../sub"),
            Path::new("./not-yet-built"),
        ];
        for target in inside {
            assert!(
                !outside_checkout(target, &checkout, &checkout),
                "{target:?}"
            );
        }
        assert!(!outside_checkout(
            &checkout.join("target"),
            &scratch,
            &checkout
        ));
        let outside = [
            Path::new("../elsewhere"),
            Path::new("target/../../flui-other"),
        ];
        for target in outside {
            assert!(outside_checkout(target, &checkout, &checkout), "{target:?}");
        }
        // a sibling whose name merely starts with the checkout's
        assert!(outside_checkout(
            &scratch.join("flui-wt"),
            &checkout,
            &checkout
        ));
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn check_changed_contract() {
        crate::table_test::run_table(
            "check_changed_contract",
            &[
                (
                    "android_without_an_ndk_leaves_other_native_checks_runnable",
                    android_without_an_ndk_leaves_other_native_checks_runnable as fn(),
                ),
                (
                    "missing_cross_wrappers_leave_native_steps_runnable",
                    missing_cross_wrappers_leave_native_steps_runnable as fn(),
                ),
                (
                    "a_package_change_runs_the_scoped_commands",
                    a_package_change_runs_the_scoped_commands as fn(),
                ),
                (
                    "a_low_layer_change_by_default_lints_the_scope_and_tests_the_changed_crates",
                    a_low_layer_change_by_default_lints_the_scope_and_tests_the_changed_crates
                        as fn(),
                ),
                (
                    "full_runs_the_whole_lane_and_the_dependents_tests",
                    full_runs_the_whole_lane_and_the_dependents_tests as fn(),
                ),
                (
                    "a_dependents_platform_and_fixture_suites_run_only_when_full",
                    a_dependents_platform_and_fixture_suites_run_only_when_full as fn(),
                ),
                (
                    "docs_and_xtask_changes_keep_their_scope",
                    docs_and_xtask_changes_keep_their_scope as fn(),
                ),
                (
                    "a_scope_without_dependents_tests_every_crate_in_it",
                    a_scope_without_dependents_tests_every_crate_in_it as fn(),
                ),
                (
                    "the_whole_workspace_also_lints_the_engine_testing_code",
                    the_whole_workspace_also_lints_the_engine_testing_code as fn(),
                ),
                (
                    "the_lane_reads_every_field_change_scope_gives",
                    the_lane_reads_every_field_change_scope_gives as fn(),
                ),
                (
                    "windows_workspace_testing_preserves_the_driver_suite",
                    windows_workspace_testing_preserves_the_driver_suite as fn(),
                ),
                (
                    "a_target_dir_outside_the_checkout_is_refused",
                    a_target_dir_outside_the_checkout_is_refused as fn(),
                ),
                (
                    "platform_compiler_checks_run_only_when_in_scope_on_every_host",
                    platform_compiler_checks_run_only_when_in_scope_on_every_host as fn(),
                ),
            ],
        );
    }
}
