//! `cargo xtask check-changed`: CI's checks, scoped to a change, locally.
//!
//! The scope is `change_scope`'s, the computation behind CI's `plan` job, over
//! this branch's diff against the base PLUS uncommitted and untracked files:
//! the changed crates and their dependents. The steps are the wide lane's jobs
//! narrowed to that scope (clippy, tests, rustdoc, doctests, the cross and
//! per-feature clippies); a step whose target or tool this host lacks is
//! skipped with a message naming the fix, since CI runs it anyway. When the
//! scope holds dependents, clippy and the tests run on the changed crates
//! first and stop there on a failure, before the dependents' fan-out.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use super::exec::{Cmd, Host, Runner, Step, installed, installed_targets};
use super::{
    WASM_TARGET, driver_tests, engine_testing_clippy, hack_passes, native, reload_fixture_build,
    wasm_facade_check,
};
use crate::change_scope::{self, Stage, StagePart};
use crate::util::repo_root;

/// The scope's inputs: `change_scope`'s `affected` outputs, typed, and its
/// clippy-then-nextest passes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lane {
    /// `docs`, `none`, `packages` or `full`.
    mode: String,
    heavy_required: bool,
    reason: String,
    /// The scope, space-separated; empty for the whole workspace.
    packages: String,
    /// The clippy and nextest passes, in order; together they cover `packages`.
    stages: Vec<Stage>,
    platform: bool,
    wasm_args: String,
    wasm_facade: bool,
    hack_args: String,
    doc_args: String,
    doctest_args: String,
}

impl Lane {
    /// Reads the `(key, value)` pairs `change_scope` produces; `stages` are
    /// its clippy-then-nextest passes over the same scope.
    fn from_fields(
        fields: impl IntoIterator<Item = (&'static str, String)>,
        stages: Vec<Stage>,
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
            stages,
            platform: flag("platform", text("platform")?)?,
            wasm_args: text("wasm_args")?,
            wasm_facade: flag("wasm_facade", text("wasm_facade")?)?,
            hack_args: text("hack_args")?,
            doc_args: text("doc_args")?,
            doctest_args: text("doctest_args")?,
        })
    }
}

/// Whether the space-separated `packages` select `name` (an empty list is the
/// whole workspace).
fn selects(packages: &str, name: &str) -> bool {
    packages.is_empty() || packages.split_whitespace().any(|p| p == name)
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

/// One stage's clippy, then its tests (`--no-fail-fast` within the stage).
fn stage_steps(stage: &Stage, host: Host) -> Vec<Step> {
    let mut steps: Vec<Step> = vec![
        Cmd::cargo(["clippy"])
            .split(&stage.pkg_args)
            .args(["--all-targets", "--locked", "--", "-D", "warnings"])
            .into(),
    ];
    if selects(&stage.packages, "flui-engine") {
        steps.push(engine_testing_clippy().into());
    }
    if !stage.test_args.is_empty() {
        let split_driver = host == Host::Windows && stage.packages.is_empty();
        if split_driver {
            steps.push(driver_tests().into());
        }
        if selects(&stage.packages, "flui-hot-reload-lifecycle-fixture") {
            steps.push(reload_fixture_build().into());
        }
        let mut tests = Cmd::cargo(["nextest", "run"])
            .split(&stage.test_args)
            .args(["--locked", "--no-fail-fast", "--no-tests=pass"])
            .split(&stage.features);
        if split_driver {
            tests = tests.args(["--exclude", "xtask"]);
        }
        steps.push(tests.into());
    }
    steps
}

/// The compiling steps for `lane`, after `cargo fmt`: each stage's clippy and
/// tests in turn (a failing stage stops the run), then the scope-wide checks. `targets` are the installed rustup targets; `have_hack` says whether
/// cargo-hack is.
fn plan(
    lane: &Lane,
    host: Host,
    targets: &BTreeSet<String>,
    have_hack: bool,
    native: &[Step],
) -> Vec<Step> {
    let have = |target: &str| targets.contains(target);
    let mut steps: Vec<Step> = Vec::new();
    for (index, stage) in lane.stages.iter().enumerate() {
        if let Some(part) = stage.part {
            let covers = match part {
                StagePart::Changed => "the changed crates",
                StagePart::Dependents => "their dependents",
            };
            steps.push(Step::Note(format!(
                "check-changed: stage {}/{}, clippy and tests of {covers}: {}",
                index + 1,
                lane.stages.len(),
                stage.packages
            )));
        }
        steps.extend(stage_steps(stage, host));
    }
    if lane.stages.len() > 1 {
        steps.push(Step::Note(
            "check-changed: the remaining checks, over the whole scope".to_owned(),
        ));
    }
    if !lane.doc_args.is_empty() {
        steps.push(
            Cmd::cargo(["doc"])
                .split(&lane.doc_args)
                .args(["--no-deps", "--locked", "--document-private-items"])
                .env("RUSTDOCFLAGS", "-D warnings")
                .into(),
        );
    }
    if !lane.doctest_args.is_empty() {
        steps.push(
            Cmd::cargo(["test"])
                .split(&lane.doctest_args)
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
    if lane.platform {
        steps.push(super::platform_compiler_tests().into());
        steps.push(if host == Host::Linux {
            super::platform_suite_linux()
                .args(["-E", "not (group(trybuild))"])
                .into()
        } else {
            Step::Note(
                "check-changed: flui-platform is in scope, but its native suite needs xvfb-run (Linux); CI runs it"
                    .to_owned(),
            )
        });
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
pub(super) fn run(runner: Runner, base: &str) -> anyhow::Result<ExitCode> {
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
    let scoped = change_scope::worktree_lane(base)?;
    let lane = Lane::from_fields(scoped.fields, scoped.stages)?;
    println!("check-changed: {}", lane.reason);
    if lane.heavy_required {
        println!(
            "check-changed: this change reaches beyond the scoped checks (see the reason above): CI checks the whole workspace; locally, consider cargo xtask ci-full"
        );
    }
    runner.run(&Cmd::cargo(["fmt", "--all", "--", "--check"]))?;
    if matches!(lane.mode.as_str(), "docs" | "none") {
        println!("check-changed: nothing to compile ({})", lane.mode);
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "check-changed: packages: {}",
        if lane.packages.is_empty() {
            "<whole workspace>"
        } else {
            &lane.packages
        }
    );
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
    runner.steps(&plan(
        &lane,
        Host::current(),
        &targets,
        have_hack,
        &native_steps,
    ))?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(ToString::to_string).collect()
    }

    /// The lane `affected` computes for a change to flui-material, as one stage.
    fn material() -> Lane {
        Lane {
            mode: "packages".to_owned(),
            heavy_required: false,
            reason: "changed: flui-material; plus 2 dependents".to_owned(),
            packages: "flui flui-material flui-web-counter".to_owned(),
            stages: vec![Stage {
                part: None,
                packages: "flui flui-material flui-web-counter".to_owned(),
                pkg_args: "-p flui -p flui-material -p flui-web-counter".to_owned(),
                test_args: "-p flui -p flui-material -p flui-web-counter --lib --bins --tests"
                    .to_owned(),
                features: "--features flui/cupertino".to_owned(),
            }],
            platform: false,
            wasm_args: "-p flui -p flui-material -p flui-web-counter --lib --bins".to_owned(),
            wasm_facade: true,
            hack_args: "-p flui-material".to_owned(),
            doc_args: "-p flui -p flui-material -p flui-web-counter --features flui/testing"
                .to_owned(),
            doctest_args: "-p flui -p flui-material".to_owned(),
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
                "$ cargo test -p flui -p flui-material --locked --doc",
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

    /// `material()` widened to the whole workspace.
    fn whole_workspace() -> Lane {
        let mut lane = material();
        lane.packages = String::new();
        lane.stages[0].packages = String::new();
        lane.stages[0].pkg_args = "--workspace".to_owned();
        lane.stages[0].test_args =
            "--workspace --exclude flui-platform --lib --bins --tests".to_owned();
        lane
    }

    fn a_scope_with_dependents_checks_the_changed_crates_first() {
        let mut lane = material();
        lane.stages = vec![
            Stage {
                part: Some(StagePart::Changed),
                packages: "flui-material".to_owned(),
                pkg_args: "-p flui-material".to_owned(),
                test_args: "-p flui-material --lib --bins --tests".to_owned(),
                features: String::new(),
            },
            Stage {
                part: Some(StagePart::Dependents),
                packages: "flui flui-web-counter".to_owned(),
                pkg_args: "-p flui -p flui-web-counter".to_owned(),
                test_args: "-p flui -p flui-web-counter --lib --bins --tests".to_owned(),
                features: "--features flui/cupertino".to_owned(),
            },
        ];
        let steps = lines(&plan(&lane, Host::Linux, &all_targets(), true, &[]));
        assert_eq!(
            &steps[..7],
            [
                "check-changed: stage 1/2, clippy and tests of the changed crates: flui-material",
                "$ cargo clippy -p flui-material --all-targets --locked -- -D warnings",
                "$ cargo nextest run -p flui-material --lib --bins --tests --locked --no-fail-fast --no-tests=pass",
                "check-changed: stage 2/2, clippy and tests of their dependents: flui flui-web-counter",
                "$ cargo clippy -p flui -p flui-web-counter --all-targets --locked -- -D warnings",
                "$ cargo nextest run -p flui -p flui-web-counter --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino",
                "check-changed: the remaining checks, over the whole scope",
            ]
        );
        // the scope-wide steps are those of the unsplit lane
        let unsplit = lines(&plan(&material(), Host::Linux, &all_targets(), true, &[]));
        assert_eq!(steps[7..], unsplit[2..]);
    }

    fn the_whole_workspace_also_lints_the_engine_testing_code() {
        let lane = whole_workspace();
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
        assert!(!selects("flui-engine-extra", "flui-engine"));
        assert!(selects("flui flui-engine", "flui-engine"));
    }

    fn the_lane_reads_every_field_change_scope_gives() {
        let scoped = change_scope::worktree_lane("HEAD").expect("classifies");
        let fields = scoped.fields;
        let lane = Lane::from_fields(fields.clone(), scoped.stages).expect("every field is there");
        assert!(["docs", "none", "packages", "full"].contains(&lane.mode.as_str()));
        let mut missing = fields.clone();
        missing.retain(|(key, _)| *key != "wasm_facade");
        let error = Lane::from_fields(missing, Vec::new()).expect_err("a field is missing");
        assert!(error.to_string().contains("wasm_facade"), "{error}");
        let mut bad = fields;
        for (key, value) in &mut bad {
            if *key == "platform" {
                *value = "yes".to_owned();
            }
        }
        assert!(Lane::from_fields(bad, Vec::new()).is_err());
    }

    fn windows_workspace_testing_preserves_the_driver_suite() {
        let lane = whole_workspace();
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
                    "a_scope_with_dependents_checks_the_changed_crates_first",
                    a_scope_with_dependents_checks_the_changed_crates_first as fn(),
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
