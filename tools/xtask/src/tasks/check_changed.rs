//! `cargo xtask check-changed`: CI's fast lane, locally.
//!
//! The scope is `change_scope`'s, the computation behind CI's `plan` job, over
//! this branch's diff against the base PLUS uncommitted and untracked files, so
//! CI and a local run pick the same packages from the same inputs. The steps
//! are the fast-lane job's, in its order; a step whose target or tool this host
//! lacks is skipped with a message naming the fix, since CI runs it anyway.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use super::exec::{Cmd, Host, Runner, Step, installed, installed_targets};
use super::{
    ANDROID_TARGET, IOS_TARGET, MACOS_TARGET, PLATFORM_TARGETS, WASM_TARGET, WINDOWS_TARGET,
    android_runner, cli_windows, desktop_mcp_clippy, engine_testing_clippy, hack_passes,
    ios_runner, platform_clippy, wasm_facade_check,
};
use crate::change_scope;
use crate::util::repo_root;

/// The fast lane's inputs: `change_scope`'s `plan` outputs, typed.
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
    cross_platform: bool,
    cross_app: bool,
    cross_cli: bool,
    cross_desktop_mcp: bool,
    cross_ios: bool,
    wasm_args: String,
    wasm_facade: bool,
    hack_args: String,
    doc_args: String,
    doctest_args: String,
}

impl Lane {
    /// Reads the `(key, value)` pairs `change_scope` produces.
    fn from_fields(
        fields: impl IntoIterator<Item = (&'static str, String)>,
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
            cross_platform: flag("cross_platform", text("cross_platform")?)?,
            cross_app: flag("cross_app", text("cross_app")?)?,
            cross_cli: flag("cross_cli", text("cross_cli")?)?,
            cross_desktop_mcp: flag("cross_desktop_mcp", text("cross_desktop_mcp")?)?,
            cross_ios: flag("cross_ios", text("cross_ios")?)?,
            wasm_args: text("wasm_args")?,
            wasm_facade: flag("wasm_facade", text("wasm_facade")?)?,
            hack_args: text("hack_args")?,
            doc_args: text("doc_args")?,
            doctest_args: text("doctest_args")?,
        })
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

/// The compiling steps for `lane`, after `cargo fmt`, in the fast-lane job's
/// order. `targets` are the installed rustup targets; `have_hack` says whether
/// cargo-hack is.
fn plan(lane: &Lane, host: Host, targets: &BTreeSet<String>, have_hack: bool) -> Vec<Step> {
    let have = |target: &str| targets.contains(target);
    let mut steps: Vec<Step> = vec![
        Cmd::cargo(["clippy"])
            .split(&lane.pkg_args)
            .args(["--all-targets", "--locked", "--", "-D", "warnings"])
            .into(),
    ];
    if lane.has_engine() {
        steps.push(engine_testing_clippy().into());
    }
    if !lane.test_args.is_empty() {
        steps.push(
            Cmd::cargo(["nextest", "run"])
                .split(&lane.test_args)
                .args(["--locked", "--no-fail-fast", "--no-tests=pass"])
                .split(&lane.features)
                .into(),
        );
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
    // cfg-gated code this host's build never compiles (the fast lane's commands)
    if lane.cross_platform {
        for target in PLATFORM_TARGETS {
            steps.push(if have(target) {
                platform_clippy(target).into()
            } else {
                Step::Note(format!(
                    "check-changed: skipped flui-platform on {target} (rustup target add {target}; CI runs it)"
                ))
            });
        }
    }
    if lane.cross_app {
        steps.push(if have(ANDROID_TARGET) {
            android_runner().into()
        } else {
            Step::Note(format!(
                "check-changed: skipped the android runner (rustup target add {ANDROID_TARGET}; CI runs it)"
            ))
        });
    }
    if lane.cross_cli {
        steps.push(if have(WINDOWS_TARGET) {
            cli_windows().into()
        } else {
            Step::Note(format!(
                "check-changed: skipped flui-cli on windows (rustup target add {WINDOWS_TARGET}; CI runs it)"
            ))
        });
    }
    if lane.cross_desktop_mcp {
        for target in [WINDOWS_TARGET, MACOS_TARGET] {
            steps.push(if have(target) {
                desktop_mcp_clippy(target).into()
            } else {
                Step::Note(format!(
                    "check-changed: skipped flui-desktop-mcp on {target} (rustup target add {target}; CI runs it)"
                ))
            });
        }
    }
    if lane.cross_ios {
        steps.push(if host == Host::MacOs && have(IOS_TARGET) {
            ios_runner().into()
        } else {
            Step::Note(format!(
                "check-changed: skipped the iOS runner (needs macOS + rustup target add {IOS_TARGET}; CI's fast-lane-ios runs it)"
            ))
        });
    }
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
        steps.push(if host == Host::Linux {
            super::platform_suite_linux().into()
        } else {
            Step::Note(
                "check-changed: flui-platform is in scope, but its suite needs xvfb-run (Linux); CI runs it"
                    .to_owned(),
            )
        });
    }
    steps
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
    let lane = Lane::from_fields(change_scope::worktree_lane(base)?)?;
    println!("check-changed: {}", lane.reason);
    if lane.heavy_required {
        println!(
            "check-changed: only CI's wide-lane jobs check part of this change (see the reason above): the PR will run the wide lane (every Linux job); locally, consider cargo xtask ci-full"
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
    runner.steps(&plan(
        &lane,
        Host::current(),
        &installed_targets(),
        have_hack,
    ))?;
    Ok(ExitCode::SUCCESS)
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
            features: "--features flui/cupertino,flui/localizations".to_owned(),
            platform: false,
            cross_platform: false,
            cross_app: true,
            cross_cli: false,
            cross_desktop_mcp: false,
            cross_ios: true,
            wasm_args: "-p flui -p flui-material -p flui-web-counter --lib --bins".to_owned(),
            wasm_facade: true,
            hack_args: "-p flui-material".to_owned(),
            doc_args: "-p flui -p flui-material -p flui-web-counter --features flui/testing"
                .to_owned(),
            doctest_args: "-p flui -p flui-material".to_owned(),
        }
    }

    fn all_targets() -> BTreeSet<String> {
        PLATFORM_TARGETS
            .into_iter()
            .chain([WASM_TARGET])
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_package_change_runs_the_fast_lane_commands() {
        assert_eq!(
            lines(&plan(&material(), Host::Linux, &all_targets(), true)),
            [
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --all-targets --locked -- -D warnings",
                "$ cargo nextest run -p flui -p flui-material -p flui-web-counter --lib --bins --tests --locked --no-fail-fast --no-tests=pass --features flui/cupertino,flui/localizations",
                "$ RUSTDOCFLAGS='-D warnings' cargo doc -p flui -p flui-material -p flui-web-counter --features flui/testing --no-deps --locked --document-private-items",
                "$ cargo test -p flui -p flui-material --locked --doc",
                "$ CC_aarch64_linux_android=clang CFLAGS_aarch64_linux_android=--target=aarch64-linux-android21 AR_aarch64_linux_android=ar cargo clippy -p flui-app -p flui --locked --target aarch64-linux-android -- -D warnings",
                "check-changed: skipped the iOS runner (needs macOS + rustup target add aarch64-apple-ios; CI's fast-lane-ios runs it)",
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --lib --bins --locked --target wasm32-unknown-unknown -- -D warnings",
                "$ cargo check -p flui --locked --target wasm32-unknown-unknown --no-default-features --features hot-reload",
                "$ cargo hack clippy -p flui-material --locked --each-feature --keep-going -- -D warnings",
                "$ cargo hack clippy -p flui-material --locked --each-feature --keep-going --tests --benches --examples -- -D warnings",
            ]
        );
        let on_mac = lines(&plan(&material(), Host::MacOs, &all_targets(), true));
        assert!(on_mac.contains(
            &"$ cargo clippy -p flui-app -p flui --locked --target aarch64-apple-ios -- -D warnings"
                .to_owned()
        ));
    }

    #[test]
    fn missing_targets_and_tools_are_skipped_with_the_fix() {
        let mut lane = material();
        lane.cross_platform = true;
        lane.cross_cli = true;
        lane.cross_desktop_mcp = true;
        lane.platform = true;
        let steps = lines(&plan(&lane, Host::Windows, &BTreeSet::new(), false));
        for expected in [
            "check-changed: skipped flui-platform on aarch64-apple-darwin (rustup target add aarch64-apple-darwin; CI runs it)",
            "check-changed: skipped the android runner (rustup target add aarch64-linux-android; CI runs it)",
            "check-changed: skipped flui-cli on windows (rustup target add x86_64-pc-windows-msvc; CI runs it)",
            "check-changed: skipped flui-desktop-mcp on aarch64-apple-darwin (rustup target add aarch64-apple-darwin; CI runs it)",
            "check-changed: skipped wasm32 (rustup target add wasm32-unknown-unknown; CI runs it)",
            "check-changed: skipped per-feature clippy of changed manifests (cargo install --locked cargo-hack; CI runs it)",
            "check-changed: flui-platform is in scope, but its suite needs xvfb-run (Linux); CI runs it",
        ] {
            assert!(
                steps.contains(&expected.to_owned()),
                "{expected}\n{steps:#?}"
            );
        }
        let linux = lines(&plan(&lane, Host::Linux, &all_targets(), true));
        assert_eq!(
            linux.last().map(String::as_str),
            Some(
                "$ FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast"
            )
        );
        for target in PLATFORM_TARGETS {
            let line = format!(
                "$ cargo clippy -p flui-platform --locked --all-targets --features a11y --target {target} -- -D warnings"
            );
            assert!(linux.contains(&line), "{line}");
        }
        assert!(linux.contains(
            &"$ cargo clippy -p flui-cli --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings"
                .to_owned()
        ));
        for target in ["x86_64-pc-windows-msvc", "aarch64-apple-darwin"] {
            let line = format!(
                "$ cargo clippy -p flui-desktop-mcp --locked --all-targets --target {target} -- -D warnings"
            );
            assert!(linux.contains(&line), "{line}");
        }
    }

    #[test]
    fn the_whole_workspace_also_lints_the_engine_testing_code() {
        let mut lane = material();
        lane.packages = String::new();
        lane.pkg_args = "--workspace".to_owned();
        let steps = lines(&plan(&lane, Host::Linux, &all_targets(), true));
        assert_eq!(
            &steps[..2],
            [
                "$ cargo clippy --workspace --all-targets --locked -- -D warnings",
                "$ cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings",
            ]
        );
        lane.packages = "flui-engine-extra".to_owned();
        assert!(!lane.has_engine());
        lane.packages = "flui flui-engine".to_owned();
        assert!(lane.has_engine());
    }

    #[test]
    fn empty_lists_skip_their_steps() {
        let mut lane = material();
        for list in [
            &mut lane.test_args,
            &mut lane.doc_args,
            &mut lane.doctest_args,
            &mut lane.wasm_args,
            &mut lane.hack_args,
        ] {
            list.clear();
        }
        lane.cross_app = false;
        lane.cross_ios = false;
        assert_eq!(
            lines(&plan(&lane, Host::Linux, &all_targets(), true)),
            [
                "$ cargo clippy -p flui -p flui-material -p flui-web-counter --all-targets --locked -- -D warnings"
            ]
        );
    }

    #[test]
    fn the_lane_reads_every_field_change_scope_gives() {
        let fields = change_scope::worktree_lane("HEAD").expect("classifies");
        let lane = Lane::from_fields(fields.clone()).expect("every field is there");
        assert!(["docs", "none", "packages", "full"].contains(&lane.mode.as_str()));
        let mut missing = fields.clone();
        missing.retain(|(key, _)| *key != "wasm_facade");
        let error = Lane::from_fields(missing).expect_err("a field is missing");
        assert!(error.to_string().contains("wasm_facade"), "{error}");
        let mut bad = fields;
        for (key, value) in &mut bad {
            if *key == "platform" {
                *value = "yes".to_owned();
            }
        }
        assert!(Lane::from_fields(bad).is_err());
    }

    #[test]
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
}
