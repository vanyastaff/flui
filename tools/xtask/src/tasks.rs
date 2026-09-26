//! Composite tasks: what the CI jobs run, as one command each, so a local run
//! and CI cannot drift.
//!
//! Each task prints every command before it runs it and stops at the first
//! failure, except `checks` and `deps`, which run every check and then report
//! all that failed. `--dry-run` prints the commands without running them. The
//! command lists are built by plain functions ([`test_plan`],
//! [`cross_typecheck_plan`], ...) so the unit tests below pin them to the CI
//! steps they mirror.

mod check_changed;
mod checks;
mod deps;
mod exec;
pub(crate) mod facade;
mod web;

use std::path::Path;
use std::process::ExitCode;

use exec::{Cmd, Host, NO_ARGS, Runner, Step, host_binary, installed, parsed, target_dir};

use crate::doc_strict;

const WINDOWS_TARGET: &str = "x86_64-pc-windows-msvc";
const MACOS_TARGET: &str = "aarch64-apple-darwin";
const ANDROID_TARGET: &str = "aarch64-linux-android";
/// The device triple, not `-sim`: the two differ in the slice, not in the API
/// surface a lint sees (`cargo xtask device ios-sim` executes the simulator one).
const IOS_TARGET: &str = "aarch64-apple-ios";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// flui-platform's backends as they ship: MSVC (what `gpu-test` runs on), not
/// the GNU ABI, and aarch64 for macOS, Android and iOS.
const PLATFORM_TARGETS: [&str; 4] = [WINDOWS_TARGET, MACOS_TARGET, ANDROID_TARGET, IOS_TARGET];

/// The local test scope, one slice for the whole suite (docs/testing.md,
/// "What `cargo xtask test` runs"):
/// - `--features flui/cupertino,flui/localizations`: the facade's non-default
///   catalogs join the workspace run through feature unification, instead of a
///   second `-p flui --features ...` run that re-resolved features for flui's
///   graph alone and so rebuilt every shared crate under a second hash. The
///   default-feature facade (material only) is then not tested here, CI's
///   `test` job included (it runs this scope); its `cargo build --workspace
///   --all-targets` compiles it, and `feature-matrix` lints it.
/// - `--lib --bins --tests`: build and run what has tests without LINKING the
///   ~60 examples, which `cargo nextest run` otherwise links on every run.
///   Examples still compile in `lint` (`--all-targets`); CI's `test` job and
///   `ci-full`'s `cargo build --workspace --all-targets` link them.
/// - flui-platform is excluded: it runs on its own (see [`platform_suite`]).
///
/// CI's `fast-lane` builds the same scope and narrows the run with a nextest
/// filterset (`change_scope`'s `ci_test_args`), so it builds what the `test`
/// job's cache holds.
pub(crate) const TEST_SCOPE: [&str; 10] = [
    "--workspace",
    "--exclude",
    "flui-platform",
    "--locked",
    "--no-fail-fast",
    "--lib",
    "--bins",
    "--tests",
    "--features",
    "flui/cupertino,flui/localizations",
];

/// Options every task takes.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct RunOpts {
    /// Print the commands instead of running them.
    #[arg(long)]
    dry_run: bool,
}

impl RunOpts {
    fn runner(self) -> Runner {
        Runner {
            dry_run: self.dry_run,
        }
    }
}

/// `-- -D warnings`: every lint step denies warnings, as CI does.
fn deny_warnings(cmd: Cmd) -> Cmd {
    cmd.args(["--", "-D", "warnings"])
}

/// Clippy of one flui-platform backend. `--features a11y`: the UIA /
/// NSAccessibility bridges are feature-gated and this is the only gate that
/// compiles them; the a11y-off configuration is a strict subset (no
/// `cfg(not(a11y))` code exists), so checking with the feature supersedes
/// checking without. `--all-targets` so the per-OS test targets compile too.
fn platform_clippy(target: &str) -> Cmd {
    deny_warnings(Cmd::cargo([
        "clippy",
        "-p",
        "flui-platform",
        "--locked",
        "--all-targets",
        "--features",
        "a11y",
        "--target",
        target,
    ]))
}

/// The flui-app / flui Android runner (`cfg(target_os = "android")`, so no
/// flui-platform line compiles it). `psm`'s C shim (via `stacker`) is
/// cross-compiled by the host clang when told the triple: a compile-only lint
/// needs no NDK. Library targets only: the tests are host-run.
fn android_runner() -> Cmd {
    deny_warnings(
        Cmd::cargo([
            "clippy",
            "-p",
            "flui-app",
            "-p",
            "flui",
            "--locked",
            "--target",
            ANDROID_TARGET,
        ])
        .env("CC_aarch64_linux_android", "clang")
        .env(
            "CFLAGS_aarch64_linux_android",
            "--target=aarch64-linux-android21",
        )
        .env("AR_aarch64_linux_android", "ar"),
    )
}

/// The flui-app / flui iOS runner. Needs macOS: `psm`'s shim finds the Apple
/// SDK through `xcrun`.
fn ios_runner() -> Cmd {
    deny_warnings(Cmd::cargo([
        "clippy", "-p", "flui-app", "-p", "flui", "--locked", "--target", IOS_TARGET,
    ]))
}

/// flui-desktop-mcp's UI Automation, capture and input backends, which sit
/// behind `cfg(windows)` / `cfg(target_os = "macos")`: on Linux only its
/// unsupported fallbacks compile.
fn desktop_mcp_clippy(target: &str) -> Cmd {
    deny_warnings(Cmd::cargo([
        "clippy",
        "-p",
        "flui-desktop-mcp",
        "--locked",
        "--all-targets",
        "--target",
        target,
    ]))
}

/// flui-cli's Windows paths, which no Linux job compiles.
fn cli_windows() -> Cmd {
    deny_warnings(Cmd::cargo([
        "clippy",
        "-p",
        "flui-cli",
        "--locked",
        "--all-targets",
        "--target",
        WINDOWS_TARGET,
    ]))
}

/// The facade's `hot-reload` feature on wasm32: it has no web runner, but
/// enabling it must stay a well-formed build rather than expose native-only
/// imports.
fn wasm_facade_check() -> Cmd {
    Cmd::cargo([
        "check",
        "-p",
        "flui",
        "--locked",
        "--target",
        WASM_TARGET,
        "--no-default-features",
        "--features",
        "hot-reload",
    ])
}

/// flui-engine's `testing` code: the readback suites and the
/// deterministic-replay tests, which the workspace pass never compiles, so a
/// break there is invisible until the GPU job.
fn engine_testing_clippy() -> Cmd {
    deny_warnings(Cmd::cargo([
        "clippy",
        "-p",
        "flui-engine",
        "--all-targets",
        "--locked",
        "--features",
        "testing",
    ]))
}

/// cargo-hack's per-feature clippy over `packages` (`--workspace` or `-p ...`),
/// in two passes: libs/bins without dev targets (the library as its consumers
/// see it: under resolver v2 a dev-dependency's features unify only while dev
/// targets build, so a single `--all-targets` pass can hide a missing feature
/// edge), then the tests, benches and examples themselves.
fn hack_passes(packages: &str) -> [Cmd; 2] {
    let pass = || {
        Cmd::cargo(["hack", "clippy"]).split(packages).args([
            "--locked",
            "--each-feature",
            "--keep-going",
        ])
    };
    [
        deny_warnings(pass()),
        deny_warnings(pass().args(["--tests", "--benches", "--examples"])),
    ]
}

/// flui-platform's suite on Linux. `--all-features` is needed just to compile
/// the winit backend (invisible under `default = ["desktop"]`);
/// `FLUI_HEADLESS=1` routes `current_platform()` to the `HeadlessPlatform`
/// mock, so most of the suite needs no display server; the few winit-internals
/// tests that construct `WinitPlatform::new()` need a real (if virtual) X11
/// connection for clipboard init, which `xvfb-run` supplies.
fn platform_suite_linux() -> Cmd {
    Cmd::new("xvfb-run")
        .args(["-a", "cargo", "nextest", "run", "-p", "flui-platform"])
        .args(["--locked", "--all-features", "--no-fail-fast"])
        .env("FLUI_HEADLESS", "1")
}

/// flui-platform's suite on `host`: under Xvfb on Linux, directly on Windows
/// (CI's `platform-windows` job needs neither), skipped elsewhere.
fn platform_suite(host: Host) -> Step {
    match host {
        Host::Linux => platform_suite_linux().into(),
        Host::Windows => Cmd::cargo(["nextest", "run", "-p", "flui-platform"])
            .args(["--locked", "--all-features", "--no-fail-fast"])
            .into(),
        Host::MacOs | Host::Other => Step::Note(
            "Skipping flui-platform tests on this host: the CI-mirroring invocation needs xvfb-run (Linux-only) for the winit backend X11-dependent tests; see docs/testing.md."
                .to_owned(),
        ),
    }
}

/// `cargo nextest run` over [`TEST_SCOPE`], limited to the nested-cargo test
/// group (.config/nextest.toml) or to everything outside it. The filterset is
/// the only `-E`: a second one would be ORed with it, not intersected.
fn scoped_nextest(nested_cargo: bool) -> Cmd {
    let filterset = if nested_cargo {
        "group(nested-cargo)"
    } else {
        "not group(nested-cargo)"
    };
    Cmd::cargo(["nextest", "run"])
        .args(TEST_SCOPE)
        .args(["-E", filterset])
}

/// The test suite: everything outside the nested-cargo group, flui-platform on
/// its own, then the nested-cargo group (the tests that run a `cargo` of their
/// own on a project they generate; they dominate the wall-clock, so they run
/// last, and `fast` leaves them out). Same scope in both stages, so nothing
/// is rebuilt, and the two filtersets are complements, so together they are
/// the whole suite.
fn test_plan(host: Host, fast: bool) -> Vec<Step> {
    let nested = scoped_nextest(true);
    let mut steps = vec![scoped_nextest(false).into(), platform_suite(host)];
    if fast {
        steps.push(Step::Note(format!(
            "test --fast: SKIPPED the nested-cargo group (trybuild compile_fail suites, flui-cli cli_create::generated_*, flui::facade_consumer; filter in .config/nextest.toml). Run them with: {nested}"
        )));
    } else {
        steps.push(nested.into());
    }
    steps
}

/// Clippy exactly as CI's `clippy` job runs it: the workspace, then
/// flui-engine's `testing` code. Both `--locked`, because that is what the job
/// runs; the second is not optional (see [`engine_testing_clippy`]).
fn lint_plan() -> Vec<Step> {
    vec![
        deny_warnings(Cmd::cargo([
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
        ]))
        .into(),
        engine_testing_clippy().into(),
    ]
}

/// CI's `cross-typecheck` job: clippy (not check: `cfg(windows)` / `macos` /
/// `android` code is invisible to every other lint gate, and check-only let
/// ~100 deny-level violations accumulate unseen) of flui-platform's per-OS
/// backends, the mobile runners, flui-cli's Windows paths and flui-desktop-mcp's
/// Windows and macOS backends. No link, no
/// tests: green means "compiles clean under the workspace lints", nothing more.
/// The iOS runner needs macOS (CI runs it in `cli-macos`), so it is skipped
/// with a message elsewhere.
fn cross_typecheck_plan(host: Host) -> Vec<Step> {
    let mut steps: Vec<Step> = PLATFORM_TARGETS
        .into_iter()
        .map(|target| platform_clippy(target).into())
        .collect();
    steps.push(if host == Host::MacOs {
        ios_runner().into()
    } else {
        Step::Note(
            "cross-typecheck: skipped the iOS runner (its C shim needs xcrun: macOS only; CI's cli-macos job runs it)"
                .to_owned(),
        )
    });
    steps.push(android_runner().into());
    steps.push(cli_windows().into());
    steps.push(desktop_mcp_clippy(WINDOWS_TARGET).into());
    steps.push(desktop_mcp_clippy(MACOS_TARGET).into());
    steps
}

/// CI's `test-features` job: the suites behind features the default run never
/// enables (flui-assets and flui-widgets default to `default = []`). The
/// facade's non-default catalogs are in [`TEST_SCOPE`]. The last step names
/// the `signals` features, which are now accepted and ignored (signals are
/// always compiled, ADR-0085 §5); it mirrors the job until the job drops it.
fn test_features_plan() -> Vec<Step> {
    let nextest =
        |args: &[&str]| Step::from(Cmd::cargo(["nextest", "run"]).args(args.iter().copied()));
    vec![
        // one feature resolution for both crates: the features are additive,
        // so every test a per-feature run would select still compiles
        nextest(&[
            "-p",
            "flui-assets",
            "-p",
            "flui-widgets",
            "--locked",
            "--no-fail-fast",
            "--features",
            "flui-assets/full,flui-widgets/images,flui-widgets/asset-images,flui-widgets/network-images",
        ]),
        // not a subset of the run above: without `asset-images`, `Image` is
        // the `StatelessView` impl a consumer of `images` alone builds, which
        // no other run compiles with a test
        nextest(&[
            "-p",
            "flui-widgets",
            "--locked",
            "--no-fail-fast",
            "--features",
            "images",
            "--test",
            "image",
        ]),
        nextest(&[
            "-p",
            "flui-view",
            "-p",
            "flui-testing",
            "-p",
            "flui-widgets",
            "-p",
            "flui-app",
            "--features",
            "flui-view/signals,flui-testing/signals,flui-widgets/signals,flui-app/signals",
            "--locked",
            "--no-fail-fast",
        ]),
    ]
}

/// One slice of CI's `feature-matrix` job, or all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slice {
    /// The combinations, then the per-feature passes over the whole workspace.
    All,
    /// Shard `index` of `count` of the per-feature passes. cargo-hack's
    /// `--partition` splits its command list into contiguous blocks, so the
    /// shards cover every package and feature exactly once by construction,
    /// as long as every shard runs the same cargo-hack version.
    Shard { index: usize, count: usize },
    /// Each supported facade feature combination, built on its own.
    Combinations,
}

impl Slice {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "all" => return Ok(Self::All),
            "combinations" => return Ok(Self::Combinations),
            _ => {}
        }
        let shard = text.split_once('/').and_then(|(index, count)| {
            let index = index.parse().ok()?;
            let count = count.parse().ok()?;
            (1..=count)
                .contains(&index)
                .then_some(Self::Shard { index, count })
        });
        shard.ok_or_else(|| {
            format!("expected `all`, `combinations` or `k/N` with 1 <= k <= N, got `{text}`")
        })
    }
}

/// The per-feature passes of `slice`; none for the combinations.
///
/// flui-engine's wgpu backend features get no combination run of their own:
/// they only forward to wgpu's features and no FLUI code is behind them, so
/// every pair compiles the same FLUI code, and each backend already builds on
/// its native target in gpu-test, cli-macos and wasm-check.
fn feature_matrix_plan(slice: Slice) -> Vec<Step> {
    let packages = match slice {
        Slice::All => "--workspace".to_owned(),
        Slice::Shard { index, count } => format!("--workspace --partition {index}/{count}"),
        Slice::Combinations => return Vec::new(),
    };
    hack_passes(&packages).map(Step::from).into()
}

/// CI's `gpu-test` job: the flui-engine readback suite, then the facade's
/// composited-layer update readback (it needs the render pipeline and the
/// headless renderer, which only the facade depends on together).
/// `--test-threads 1`: each test builds its own `wgpu::Device`, and a software
/// rasterizer serializes submission, so parallel tests saturate it.
fn gpu_test_plan() -> Vec<Step> {
    vec![
        Cmd::cargo([
            "nextest",
            "run",
            "-p",
            "flui-engine",
            "--features",
            "testing",
            "--lib",
        ])
        .args(["--locked", "--no-fail-fast", "--test-threads", "1"])
        .into(),
        Cmd::cargo(["nextest", "run", "-p", "flui", "--no-default-features"])
            .args(["--features", "gpu-readback-tests"])
            .args(["--test", "composited_layer_update_readback"])
            .args(["--locked", "--no-fail-fast", "--test-threads", "1"])
            .into(),
    ]
}

/// CI's `miri` job: the workspace's densest unsafe and arena code under Miri.
/// - flui-rendering `pipeline::owner`: the subtree arena's raw-pointer walks,
///   the PipelineCell checkout, an owner-local frame and a reentrant layout.
/// - flui-view `owner::global_key`: the GlobalKey plane (ADR-0050).
/// - flui-engine `wgpu::surface_lease` and `cancelling_renderer_new`: the
///   wgpu-free surface-lease protocol and `Renderer::new`'s cancellation,
///   GPU-free by construction.
///
/// `CARGO_BUILD_WARNINGS=warn`, as the CI job sets it: nightly adds lints and
/// deprecations ahead of stable, and this run exists for undefined behavior,
/// not lints.
fn miri_plan() -> Vec<Step> {
    [
        ("flui-rendering", "pipeline::owner"),
        ("flui-view", "owner::global_key"),
        ("flui-engine", "wgpu::surface_lease"),
        ("flui-engine", "cancelling_renderer_new"),
    ]
    .into_iter()
    .map(|(package, filter)| {
        Cmd::cargo([
            "+nightly", "miri", "test", "-p", package, "--lib", "--locked", filter,
        ])
        .env("CARGO_BUILD_WARNINGS", "warn")
        .into()
    })
    .collect()
}

/// CI's `bench-compile` job: the criterion benches compiled and linked, not run.
fn bench_compile_plan() -> Vec<Step> {
    vec![Cmd::cargo(["bench", "-p", "flui-rendering", "--no-run", "--locked"]).into()]
}

/// The workflow linters CI's `checks` job runs: actionlint (workflow
/// semantics) and zizmor (the workflows' security audit). A linter that is
/// not `installed` is skipped with a message, not failed: neither is a cargo
/// tool, and `cargo xtask doctor full` says how to get each.
fn workflow_lint_plan(installed: impl Fn(&str) -> bool) -> Vec<Step> {
    [("actionlint", &[][..]), ("zizmor", &["."][..])]
        .into_iter()
        .map(|(program, args)| {
            if installed(program) {
                Cmd::new(program).args(args.iter().copied()).into()
            } else {
                Step::Note(format!(
                    "{program}: not installed, skipped (`cargo xtask doctor full` says how to \
                     install it); CI's `checks` job runs it"
                ))
            }
        })
        .collect()
}

/// The live smoke: a real windowed demo (built with `--locked`, as CI builds
/// it) driven by the `flui-live-smoke` harness. X11: real XTEST input under
/// Xvfb (launch, mid-drag tracking, scroll, a clean WM_DELETE close, then a
/// programmatic `PlatformWindow::close`, issue #919). Wayland: close-path
/// teardown ordering under a headless weston, the post-quit `wl_proxy`
/// use-after-free class (issue #713) X11 cannot see; the harness skips with a
/// message when weston is absent. Linux only (see [`live_smoke_stage`]).
fn live_smoke_plan(wayland: bool, target_dir: &Path) -> Vec<Step> {
    let harness = host_binary(target_dir, &["debug"], "flui-live-smoke");
    let demo = host_binary(target_dir, &["debug", "examples"], "sliver_demo");
    let (harness, demo) = (harness.display().to_string(), demo.display().to_string());
    let run = if wayland {
        Cmd::new(harness).args([demo, "wayland".to_owned()])
    } else {
        Cmd::new("xvfb-run")
            .args(["-a", "-s", "-screen 0 1200x800x24"])
            .args([harness, demo])
    };
    vec![
        Cmd::cargo(["build", "-p", "flui", "--features", "material"])
            .args(["--example", "sliver_demo", "--locked"])
            .into(),
        Cmd::cargo(["build", "-p", "flui-live-smoke", "--locked"]).into(),
        run.into(),
    ]
}

/// The nested-cargo tests' build caches, relative to the target directory:
/// separate target roots that `cargo sweep` cannot see.
const NESTED_CACHES: [&[&str]; 3] = [
    &["cli-template-check"],
    &["facade-consumer-check"],
    &["tests", "trybuild"],
];

/// `bytes` the way `du -sh` prints a size: one decimal below 10 of a unit.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else if value < 10.0 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

/// The total size of the files under `dir`.
fn tree_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len())
        .sum()
}

// Stages: a task's body, an error at the first failure, shared by the
// composite tasks.

fn gate_stage(runner: Runner) -> anyhow::Result<()> {
    checks::run(runner, false)?;
    runner.steps(&lint_plan())?;
    runner.in_process("doc-strict", || doc_strict::doc_strict(&parsed(NO_ARGS)?))
}

fn ci_stage(runner: Runner) -> anyhow::Result<()> {
    gate_stage(runner)?;
    runner.steps(&test_plan(Host::current(), false))?;
    // nextest executes no doctests; flui-platform's need neither
    // `--all-features` nor a display server, so it is not carved out
    runner.run(&Cmd::cargo(["test", "--workspace", "--locked", "--doc"]))
}

/// Runs every step of `slice` even after one fails, so one run reports each
/// broken configuration.
fn feature_matrix_stage(runner: Runner, slice: Slice) -> anyhow::Result<()> {
    let combos = match slice {
        Slice::Shard { .. } => Ok(()),
        Slice::All | Slice::Combinations => facade::run(runner),
    };
    let passes = runner.every(&feature_matrix_plan(slice));
    combos.and(passes)
}

fn live_smoke_stage(runner: Runner, wayland: bool) -> anyhow::Result<()> {
    if Host::current() != Host::Linux {
        println!(
            "live-smoke: skipped on this host: it needs xvfb-run (X11) and weston (Wayland), Linux only; CI runs it"
        );
        return Ok(());
    }
    runner.steps(&live_smoke_plan(wayland, &target_dir()?))
}

/// `Ok(ExitCode::SUCCESS)` once `stage` succeeded.
fn done(stage: anyhow::Result<()>) -> anyhow::Result<ExitCode> {
    stage.map(|()| ExitCode::SUCCESS)
}

/// Arguments for `cargo xtask checks`.
#[derive(Debug, clap::Args)]
pub(crate) struct ChecksArgs {
    /// Fail when typos, taplo or lychee is not installed instead of skipping it (CI).
    #[arg(long)]
    strict: bool,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask checks`: run the source checks that need no workspace build (the CI `checks` job).
///
/// `cargo fmt --check`, typos, taplo, and this crate's own checks in-process:
/// `docs-links` (lychee), `workspace` (its self-test, then the workspace),
/// `toolchain`, `wgsl` (its self-test, then the shaders), `paths-filter` and
/// `font-assets --package-list`. All of them run; exit 1 if any failed.
pub(crate) fn checks(args: &ChecksArgs) -> anyhow::Result<ExitCode> {
    done(checks::run(args.run.runner(), args.strict))
}

/// Arguments for `cargo xtask lint`.
#[derive(Debug, clap::Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask lint`: run clippy the way CI does.
pub(crate) fn lint(args: &LintArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&lint_plan()))
}

/// Arguments for `cargo xtask gate`.
#[derive(Debug, clap::Args)]
pub(crate) struct GateArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask gate`: `checks`, `lint` and `doc-strict`.
pub(crate) fn gate(args: &GateArgs) -> anyhow::Result<ExitCode> {
    done(gate_stage(args.run.runner()))
}

/// Arguments for `cargo xtask test`.
#[derive(Debug, clap::Args)]
pub(crate) struct TestArgs {
    /// Leave out the nested-cargo group (trybuild, generated projects, facade
    /// consumers): the quick local loop.
    #[arg(long)]
    fast: bool,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask test`: run the workspace test suite the way CI does.
pub(crate) fn test(args: &TestArgs) -> anyhow::Result<ExitCode> {
    done(
        args.run
            .runner()
            .steps(&test_plan(Host::current(), args.fast)),
    )
}

/// Arguments for `cargo xtask ci`.
#[derive(Debug, clap::Args)]
pub(crate) struct CiArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask ci`: `gate`, `test` and the doctests: the local mirror of CI's required checks.
pub(crate) fn ci(args: &CiArgs) -> anyhow::Result<ExitCode> {
    done(ci_stage(args.run.runner()))
}

/// Arguments for `cargo xtask ci-full`.
#[derive(Debug, clap::Args)]
pub(crate) struct CiFullArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask ci-full`: `ci` plus every heavy CI job this host can run.
///
/// Then: the all-targets build (the only step that links the examples),
/// test-features, feature-matrix, wasm-check, wasm-link, wasm-test,
/// cross-typecheck, bench-compile, `deps` (each tool when installed), the
/// workflow linters (each when installed), miri, gpu-test, and on Linux
/// live-smoke under X11 and Wayland. Slow on purpose:
/// cargo-hack's per-feature matrix dominates; `cargo xtask doctor full` names
/// any tool it needs first. What stays CI-only is the table in docs/testing.md.
pub(crate) fn ci_full(args: &CiFullArgs) -> anyhow::Result<ExitCode> {
    let runner = args.run.runner();
    ci_stage(runner)?;
    runner.run(&Cmd::cargo([
        "build",
        "--workspace",
        "--all-targets",
        "--locked",
    ]))?;
    runner.steps(&test_features_plan())?;
    feature_matrix_stage(runner, Slice::All)?;
    web::check(runner)?;
    web::link(runner)?;
    web::test(runner)?;
    runner.steps(&cross_typecheck_plan(Host::current()))?;
    runner.steps(&bench_compile_plan())?;
    deps::run(runner, None, false)?;
    runner.steps(&workflow_lint_plan(|program| {
        runner.dry_run || installed(program, &["--version"])
    }))?;
    runner.steps(&miri_plan())?;
    runner.steps(&gpu_test_plan())?;
    if Host::current() == Host::Linux {
        live_smoke_stage(runner, false)?;
        live_smoke_stage(runner, true)?;
    } else {
        println!(
            "ci-full: live-smoke (X11 and Wayland) skipped on this host: it needs xvfb-run and weston (Linux); CI runs it"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Arguments for `cargo xtask check-changed`.
#[derive(Debug, clap::Args)]
pub(crate) struct CheckChangedArgs {
    /// Diff against the merge base with this revision.
    #[arg(long, default_value = "origin/main")]
    base: String,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask check-changed`: fmt, clippy and tests over the crates a change touches (CI's fast lane).
///
/// The crates this branch changes against the base, uncommitted and untracked
/// work included, and every workspace crate depending on them. Refuses (exit
/// 2) a `CARGO_TARGET_DIR` outside the checkout: a target shared between
/// worktrees links stale code (docs/testing.md).
pub(crate) fn check_changed(args: &CheckChangedArgs) -> anyhow::Result<ExitCode> {
    check_changed::run(args.run.runner(), &args.base)
}

/// Arguments for `cargo xtask deps`.
#[derive(Debug, clap::Args)]
pub(crate) struct DepsArgs {
    /// Run one part: `policy` (cargo-deny's bans, licenses and sources, and
    /// cargo-shear) or `advisories` (cargo-deny's, against today's RustSec
    /// database). Both by default.
    #[arg(long, value_enum)]
    only: Option<deps::Part>,
    /// Fail when cargo-deny or cargo-shear is not installed instead of
    /// skipping it (CI).
    #[arg(long)]
    strict: bool,
    #[command(flatten)]
    run: RunOpts,
}

/// The cargo subcommands `cargo xtask deps` runs, each with the crate that
/// installs it (for `cargo xtask doctor`).
pub(crate) fn deps_tools() -> impl Iterator<Item = (&'static str, &'static str)> {
    deps::tools()
        .into_iter()
        .map(|tool| (tool.sub, tool.install))
}

/// `cargo xtask deps`: check the dependency graph and the manifests (cargo-deny, cargo-shear).
///
/// `cargo deny --workspace --locked check` split into its policy checks and
/// its advisories, and `cargo shear --locked` (`--format github` under GitHub
/// Actions). Every step runs; exit 1 if any failed. CI's `deps` job runs
/// `--only policy` and `--only advisories` as two steps, the second blocking
/// only in the `wide`, `full` and `extended` lanes.
pub(crate) fn deps(args: &DepsArgs) -> anyhow::Result<ExitCode> {
    done(deps::run(args.run.runner(), args.only, args.strict))
}

/// Arguments for `cargo xtask feature-matrix`.
#[derive(Debug, clap::Args)]
pub(crate) struct FeatureMatrixArgs {
    /// `all`, `combinations` (the facade feature combinations) or `k/N`
    /// (shard k of N of the per-feature passes).
    #[arg(long, default_value = "all", value_parser = Slice::parse)]
    slice: Slice,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask feature-matrix`: clippy every feature on its own (cargo-hack).
///
/// `facade-combos`, then the per-feature passes; `--slice` picks the part one
/// CI runner does. Needs cargo-hack.
pub(crate) fn feature_matrix(args: &FeatureMatrixArgs) -> anyhow::Result<ExitCode> {
    done(feature_matrix_stage(args.run.runner(), args.slice))
}

/// Arguments for `cargo xtask facade-combos`.
#[derive(Debug, clap::Args)]
pub(crate) struct FacadeCombosArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask facade-combos`: build each supported facade feature combination on its own.
pub(crate) fn facade_combos(args: &FacadeCombosArgs) -> anyhow::Result<ExitCode> {
    done(facade::run(args.run.runner()))
}

/// Arguments for `cargo xtask cross-typecheck`.
#[derive(Debug, clap::Args)]
pub(crate) struct CrossTypecheckArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask cross-typecheck`: clippy the Win32, AppKit, Android and iOS backends without linking.
///
/// Needs `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
/// aarch64-linux-android aarch64-apple-ios`.
pub(crate) fn cross_typecheck(args: &CrossTypecheckArgs) -> anyhow::Result<ExitCode> {
    done(
        args.run
            .runner()
            .steps(&cross_typecheck_plan(Host::current())),
    )
}

/// Arguments for `cargo xtask wasm-check`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmCheckArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask wasm-check`: check and clippy the workspace for wasm32.
pub(crate) fn wasm_check(args: &WasmCheckArgs) -> anyhow::Result<ExitCode> {
    done(web::check(args.run.runner()))
}

/// Arguments for `cargo xtask wasm-link`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmLinkArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask wasm-link`: link the wasm demos and check their imports.
pub(crate) fn wasm_link(args: &WasmLinkArgs) -> anyhow::Result<ExitCode> {
    done(web::link(args.run.runner()))
}

/// Arguments for `cargo xtask wasm-test`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmTestArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask wasm-test`: run the wasm32 tests under wasm-bindgen-test-runner.
///
/// Installs the wasm-bindgen-cli matching Cargo.lock when it is missing.
pub(crate) fn wasm_test(args: &WasmTestArgs) -> anyhow::Result<ExitCode> {
    done(web::test(args.run.runner()))
}

/// Arguments for `cargo xtask live-smoke`.
#[derive(Debug, clap::Args)]
pub(crate) struct LiveSmokeArgs {
    /// Run the Wayland variant (headless weston) instead of X11.
    #[arg(long)]
    wayland: bool,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask live-smoke`: real-window smoke test under X11 (or Wayland).
pub(crate) fn live_smoke(args: &LiveSmokeArgs) -> anyhow::Result<ExitCode> {
    done(live_smoke_stage(args.run.runner(), args.wayland))
}

/// Arguments for `cargo xtask gpu-test`.
#[derive(Debug, clap::Args)]
pub(crate) struct GpuTestArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask gpu-test`: the GPU readback suites.
///
/// CI renders on Windows' WARP software rasterizer; locally the host adapter
/// renders, so a pixel mismatch CI does not show is a host difference to
/// investigate, not a CI result.
pub(crate) fn gpu_test(args: &GpuTestArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&gpu_test_plan()))
}

/// Arguments for `cargo xtask miri`.
#[derive(Debug, clap::Args)]
pub(crate) struct MiriArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask miri`: the unsafe-code paths under Miri.
///
/// Needs the nightly toolchain with the miri component.
pub(crate) fn miri(args: &MiriArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&miri_plan()))
}

/// Arguments for `cargo xtask bench-compile`.
#[derive(Debug, clap::Args)]
pub(crate) struct BenchCompileArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask bench-compile`: compile the benchmarks without running them.
pub(crate) fn bench_compile(args: &BenchCompileArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&bench_compile_plan()))
}

/// Arguments for `cargo xtask demo-snapshots`.
#[derive(Debug, clap::Args)]
pub(crate) struct DemoSnapshotsArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask demo-snapshots`: the demo layer snapshot suite.
///
/// Structural snapshots of the demo trees' painted layer trees, no GPU. With
/// `cupertino` on: the suite snapshots the Material and the Cupertino demo,
/// and Material is the facade default. Review changed snapshots with
/// `cargo insta review`, one diff at a time: a snapshot diff is the regression
/// report, so it is read before it is accepted.
pub(crate) fn demo_snapshots(args: &DemoSnapshotsArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().run(&Cmd::cargo([
        "nextest",
        "run",
        "-p",
        "flui",
        "--locked",
        "--features",
        "cupertino",
        "--test",
        "demo_layer_snapshots",
    ])))
}

/// Arguments for `cargo xtask clean-nested`.
#[derive(Debug, clap::Args)]
pub(crate) struct CleanNestedArgs {
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask clean-nested`: remove the nested-cargo test caches under the target directory.
///
/// `cli-template-check/`, `facade-consumer-check/` and `tests/trybuild/`; the
/// next full `cargo xtask test` rebuilds them (~9 min cold). Run it only while
/// no test run is using them.
pub(crate) fn clean_nested(args: &CleanNestedArgs) -> anyhow::Result<ExitCode> {
    let target = target_dir()?;
    let mut found = 0;
    for rel in NESTED_CACHES {
        let mut dir = target.clone();
        dir.extend(rel);
        if !dir.is_dir() {
            continue;
        }
        found += 1;
        let size = human_size(tree_size(&dir));
        if args.run.dry_run {
            println!("would remove {size} {}", dir.display());
            continue;
        }
        println!("removing {size} {}", dir.display());
        std::fs::remove_dir_all(&dir)
            .map_err(|error| anyhow::anyhow!("removing {}: {error}", dir.display()))?;
    }
    if found == 0 {
        println!(
            "clean-nested: no nested-cargo cache under {}",
            target.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(ToString::to_string).collect()
    }

    const SCOPE: &str = "--workspace --exclude flui-platform --locked --no-fail-fast --lib --bins --tests --features flui/cupertino,flui/localizations";

    #[test]
    fn workflow_lint_runs_each_installed_linter_and_skips_the_rest() {
        assert_eq!(
            lines(&workflow_lint_plan(|_| true)),
            ["$ actionlint", "$ zizmor ."]
        );
        let only_zizmor = lines(&workflow_lint_plan(|program| program == "zizmor"));
        assert!(
            only_zizmor[0].starts_with("actionlint: not installed, skipped"),
            "{only_zizmor:?}"
        );
        assert_eq!(only_zizmor[1], "$ zizmor .");
    }

    #[test]
    fn test_runs_both_group_stages_and_the_platform_suite_per_host() {
        assert_eq!(
            lines(&test_plan(Host::Linux, false)),
            [
                format!("$ cargo nextest run {SCOPE} -E 'not group(nested-cargo)'"),
                "$ FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast".to_owned(),
                format!("$ cargo nextest run {SCOPE} -E 'group(nested-cargo)'"),
            ]
        );
        assert_eq!(
            lines(&test_plan(Host::Windows, false))[1],
            "$ cargo nextest run -p flui-platform --locked --all-features --no-fail-fast"
        );
        assert!(lines(&test_plan(Host::MacOs, false))[1].starts_with("Skipping flui-platform"));
        let fast = lines(&test_plan(Host::Linux, true));
        assert_eq!(fast.len(), 3);
        assert!(fast[2].starts_with("test --fast: SKIPPED the nested-cargo group"));
        assert!(
            fast[2].ends_with(&format!(
                "cargo nextest run {SCOPE} -E 'group(nested-cargo)'"
            )),
            "{}",
            fast[2]
        );
    }

    #[test]
    fn lint_is_the_clippy_job() {
        assert_eq!(
            lines(&lint_plan()),
            [
                "$ cargo clippy --workspace --all-targets --locked -- -D warnings",
                "$ cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings",
            ]
        );
    }

    #[test]
    fn cross_typecheck_is_the_ci_job_plus_ios_on_macos() {
        let ci_job = [
            "$ cargo clippy -p flui-platform --locked --all-targets --features a11y --target x86_64-pc-windows-msvc -- -D warnings",
            "$ cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-darwin -- -D warnings",
            "$ cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-linux-android -- -D warnings",
            "$ cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-ios -- -D warnings",
            "$ CC_aarch64_linux_android=clang CFLAGS_aarch64_linux_android=--target=aarch64-linux-android21 AR_aarch64_linux_android=ar cargo clippy -p flui-app -p flui --locked --target aarch64-linux-android -- -D warnings",
            "$ cargo clippy -p flui-cli --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings",
            "$ cargo clippy -p flui-desktop-mcp --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings",
            "$ cargo clippy -p flui-desktop-mcp --locked --all-targets --target aarch64-apple-darwin -- -D warnings",
        ];
        let linux = lines(&cross_typecheck_plan(Host::Linux));
        let commands: Vec<&String> = linux.iter().filter(|l| l.starts_with('$')).collect();
        assert_eq!(commands, ci_job);
        assert!(linux[4].starts_with("cross-typecheck: skipped the iOS runner"));
        assert_eq!(
            lines(&cross_typecheck_plan(Host::MacOs))[4],
            "$ cargo clippy -p flui-app -p flui --locked --target aarch64-apple-ios -- -D warnings"
        );
    }

    #[test]
    fn feature_matrix_slices_parse() {
        assert_eq!(Slice::parse("all"), Ok(Slice::All));
        assert_eq!(Slice::parse("combinations"), Ok(Slice::Combinations));
        assert_eq!(Slice::parse("3/3"), Ok(Slice::Shard { index: 3, count: 3 }));
        for bad in ["0/3", "4/3", "1/0", "3", "a/b", "1/3/2"] {
            assert!(Slice::parse(bad).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn the_heavy_job_plans_match_ci() {
        assert_eq!(
            lines(&test_features_plan()),
            [
                "$ cargo nextest run -p flui-assets -p flui-widgets --locked --no-fail-fast --features flui-assets/full,flui-widgets/images,flui-widgets/asset-images,flui-widgets/network-images",
                "$ cargo nextest run -p flui-widgets --locked --no-fail-fast --features images --test image",
                "$ cargo nextest run -p flui-view -p flui-testing -p flui-widgets -p flui-app --features flui-view/signals,flui-testing/signals,flui-widgets/signals,flui-app/signals --locked --no-fail-fast",
            ]
        );
        assert_eq!(
            lines(&feature_matrix_plan(Slice::All)),
            [
                "$ cargo hack clippy --workspace --locked --each-feature --keep-going -- -D warnings",
                "$ cargo hack clippy --workspace --locked --each-feature --keep-going --tests --benches --examples -- -D warnings",
            ]
        );
        assert_eq!(
            lines(&feature_matrix_plan(Slice::Shard { index: 2, count: 3 })),
            [
                "$ cargo hack clippy --workspace --partition 2/3 --locked --each-feature --keep-going -- -D warnings",
                "$ cargo hack clippy --workspace --partition 2/3 --locked --each-feature --keep-going --tests --benches --examples -- -D warnings",
            ]
        );
        assert!(feature_matrix_plan(Slice::Combinations).is_empty());
        assert_eq!(
            lines(&gpu_test_plan()),
            [
                "$ cargo nextest run -p flui-engine --features testing --lib --locked --no-fail-fast --test-threads 1",
                "$ cargo nextest run -p flui --no-default-features --features gpu-readback-tests --test composited_layer_update_readback --locked --no-fail-fast --test-threads 1",
            ]
        );
        assert_eq!(
            lines(&miri_plan()),
            [
                "$ CARGO_BUILD_WARNINGS=warn cargo +nightly miri test -p flui-rendering --lib --locked pipeline::owner",
                "$ CARGO_BUILD_WARNINGS=warn cargo +nightly miri test -p flui-view --lib --locked owner::global_key",
                "$ CARGO_BUILD_WARNINGS=warn cargo +nightly miri test -p flui-engine --lib --locked wgpu::surface_lease",
                "$ CARGO_BUILD_WARNINGS=warn cargo +nightly miri test -p flui-engine --lib --locked cancelling_renderer_new",
            ]
        );
        assert_eq!(
            lines(&bench_compile_plan()),
            ["$ cargo bench -p flui-rendering --no-run --locked"]
        );
    }

    #[test]
    fn live_smoke_runs_the_built_binaries_from_the_target_dir() {
        let target = Path::new("/work/target");
        let x11 = live_smoke_plan(false, target);
        assert_eq!(
            lines(&x11[..2]),
            [
                "$ cargo build -p flui --features material --example sliver_demo --locked",
                "$ cargo build -p flui-live-smoke --locked",
            ]
        );
        let bin = |dirs: &[&str], name| host_binary(target, dirs, name).display().to_string();
        let (harness, demo) = (
            bin(&["debug"], "flui-live-smoke"),
            bin(&["debug", "examples"], "sliver_demo"),
        );
        assert_eq!(
            x11[2],
            Step::Run(Cmd::new("xvfb-run").args([
                "-a",
                "-s",
                "-screen 0 1200x800x24",
                &harness,
                &demo
            ]))
        );
        let wayland = live_smoke_plan(true, target);
        assert_eq!(
            wayland[2],
            Step::Run(Cmd::new(harness.clone()).args([demo.as_str(), "wayland"]))
        );
        if cfg!(unix) {
            assert_eq!(
                wayland[2].to_string(),
                "$ /work/target/debug/flui-live-smoke /work/target/debug/examples/sliver_demo wayland"
            );
        }
    }

    #[test]
    fn sizes_print_like_du() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(1023), "1023B");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(10 * 1024 * 1024), "10M");
        assert_eq!(human_size(7 * 1024 * 1024 * 1024 / 2), "3.5G");
    }

    #[test]
    fn tree_size_counts_every_file() {
        let dir = std::env::temp_dir().join(format!("xtask-tree-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a").join("b")).expect("mkdir");
        std::fs::write(dir.join("one"), [0_u8; 10]).expect("write");
        std::fs::write(dir.join("a").join("b").join("two"), [0_u8; 32]).expect("write");
        assert_eq!(tree_size(&dir), 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
