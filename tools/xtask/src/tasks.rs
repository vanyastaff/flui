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
use crate::util::{human_size, tree_size};

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
/// - `--features flui/material,flui/cupertino`: the facade turns no catalog on
///   by default, so both are named here rather than left to whichever
///   workspace member happens to enable one through feature unification. They
///   join the workspace run instead of a second `-p flui --features ...` run
///   that re-resolved features for flui's graph alone and so rebuilt every
///   shared crate under a second hash. The facade with no feature is then not
///   tested here, CI's `test` job included (it runs this scope);
///   `cargo xtask facade-combos` and `feature-matrix` lint it.
/// - `flui-devtools/agent`: the development agent server, off by default
///   because it opens an endpoint; on here so its `agent_endpoint` test runs
///   in the same jobs.
/// - `flui/persist`: the storage seam the runner hands realms, off by default
///   because it links the file store; on here so its runner tests and the
///   Notes example, which needs it, are covered.
/// - `--lib --bins --tests`: build and run what has tests without LINKING the
///   ~60 examples, which `cargo nextest run` otherwise links on every run.
///   Examples still compile in `lint` (`--all-targets`);
///   [`build_all_targets`] links them with the same features, so after a test
///   run it rebuilds nothing but the example and bench targets.
/// - flui-platform is excluded: it runs on its own (see [`platform_suite`]).
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
    TEST_FEATURES,
];

/// The features [`TEST_SCOPE`] turns on (see there for each one's reason).
const TEST_FEATURES: &str = "flui/material,flui/cupertino,flui/persist,flui-devtools/agent";

/// The build that links the examples and benches: the workspace's
/// `--all-targets` with [`TEST_SCOPE`]'s features, so it reuses what a test
/// run built instead of resolving features a second time. Cargo skips a
/// target whose `required-features` those features leave off (devtools'
/// `profiler_demo`, the engine's `testing` benches): `feature-matrix`
/// compiles them per feature, and nothing links them.
/// The driver itself is already built by the cargo alias. Exclude it here:
/// Windows cannot replace its running executable when workspace feature
/// unification changes the driver's dependency artifacts. Its test binary
/// remains covered by the test task (separately on Windows), and lint checks
/// all its targets.
fn build_all_targets() -> Cmd {
    Cmd::cargo([
        "build",
        "--workspace",
        "--exclude",
        "xtask",
        "--all-targets",
        "--locked",
        "--features",
        TEST_FEATURES,
    ])
}

/// Options every task takes.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct RunOpts {
    /// Print the commands instead of running them.
    #[arg(long)]
    pub(crate) dry_run: bool,
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

/// Portable external compiler checks; no native window or event loop runs.
fn platform_compiler_tests() -> Cmd {
    Cmd::cargo([
        "nextest",
        "run",
        "-p",
        "flui-platform",
        "--test",
        "compiler_guards",
    ])
    .args(["--locked", "--no-fail-fast"])
}

/// flui-platform's suite on `host`: under Xvfb on Linux, directly on Windows
/// (native Windows needs neither), skipped elsewhere. Compiler checks run
/// separately in the nested stage.
fn platform_suite(host: Host) -> Step {
    match host {
        Host::Linux => platform_suite_linux()
            .args(["-E", "not (group(trybuild))"])
            .into(),
        Host::Windows => Cmd::cargo(["nextest", "run", "-p", "flui-platform"])
            .args(["--locked", "--all-features", "--no-fail-fast", "-E", "not (group(trybuild))"])
            .into(),
        Host::MacOs | Host::Other => Step::Note(
            "Skipping native flui-platform tests on this host: the CI-mirroring invocation needs xvfb-run (Linux-only) for the winit backend X11-dependent tests; see docs/testing.md."
                .to_owned(),
        ),
    }
}

fn native_platform_plan(host: Host) -> Vec<Step> {
    match host {
        Host::Windows | Host::MacOs => [false, true]
            .into_iter()
            .map(|all_features| {
                let mut command = Cmd::cargo([
                    "nextest",
                    "run",
                    "-p",
                    "flui-platform",
                    "--locked",
                    "--no-fail-fast",
                ]);
                if all_features {
                    command = command.args(["--all-features"]);
                }
                if host == Host::MacOs {
                    // A bare test process cannot own an AppKit run loop.
                    // Direct backend unit tests still execute; window-loop
                    // tests retain their existing macOS ignore annotations.
                    command = command.env("FLUI_HEADLESS", "1");
                }
                command.args(["-E", "not (group(trybuild))"]).into()
            })
            .collect(),
        Host::Linux => vec![
            platform_suite_linux()
                .args(["-E", "not (group(trybuild))"])
                .into(),
        ],
        Host::Other => vec![Step::Note("platform-test: unsupported host".to_owned())],
    }
}

fn cli_test_plan() -> Vec<Step> {
    vec![
        Cmd::cargo([
            "nextest",
            "run",
            "-p",
            "flui-cli",
            "--locked",
            "--no-fail-fast",
        ])
        .into(),
    ]
}

/// The tests that run a `cargo` or `rustc` of their own (.config/nextest.toml):
/// the trybuild suites (group `trybuild`) and the generated projects and
/// facade consumers (group `nested-cargo`).
const NESTED: &str = "group(nested-cargo) | group(trybuild)";

/// `cargo nextest run` over [`TEST_SCOPE`] with `filterset`, the only `-E`: a
/// second one would be ORed with it, not intersected.
fn scoped_nextest(host: Host, filterset: &str) -> Cmd {
    let command = Cmd::cargo(["nextest", "run"])
        .args(TEST_SCOPE)
        .args(["-E", filterset]);
    if host == Host::Windows {
        command.args(["--exclude", "xtask"])
    } else {
        command
    }
}

/// Test the driver before workspace feature unification can replace its
/// running Windows executable. Keep both unit and integration targets.
fn driver_tests() -> Cmd {
    Cmd::cargo([
        "nextest",
        "run",
        "-p",
        "xtask",
        "--bins",
        "--tests",
        "--locked",
        "--no-fail-fast",
        "--no-tests=pass",
    ])
}

/// Which part of the suite `cargo xtask test` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stages {
    /// Everything: the default.
    All,
    /// Everything but the nested tests (`--fast`).
    Fast,
    /// The nested tests alone (`--nested`); CI can select each group separately.
    Nested,
    /// One nested group, for independent CI jobs.
    NestedGroup(NestedGroup),
    /// Everything but the trybuild suites (`--no-trybuild`): what CI's
    /// Windows job runs, since compiler diagnostics do not depend on the host
    /// and Linux checks them.
    NoTrybuild,
}

/// The test suite: everything outside the nested tests, flui-platform on its
/// own, then the nested tests (they run a `cargo` or `rustc` of their own and
/// dominate the wall-clock, so they run last). Same scope in every stage, so
/// nothing is rebuilt, and the filtersets are complements, so together they
/// are the whole suite. Windows tests the driver separately before the
/// workspace's features can force replacement of its running executable.
fn test_plan(host: Host, stages: Stages) -> Vec<Step> {
    let base = || scoped_nextest(host, &format!("not ({NESTED})"));
    let nested = scoped_nextest(host, NESTED);
    let mut steps = match stages {
        Stages::All => vec![
            base().into(),
            platform_suite(host),
            nested.into(),
            platform_compiler_tests().into(),
        ],
        Stages::Fast => vec![
            base().into(),
            platform_suite(host),
            Step::Note(
                "test --fast: SKIPPED the nested tests (trybuild compile_fail suites, flui-cli cli_create::generated_*, flui::facade_consumer; groups in .config/nextest.toml). Run them with: cargo xtask test --nested".to_owned(),
            ),
        ],
        Stages::Nested => vec![nested.into(), platform_compiler_tests().into()],
        Stages::NestedGroup(group) => {
            let mut steps = vec![scoped_nextest(host, group.filterset()).into()];
            if group == NestedGroup::Trybuild {
                steps.push(platform_compiler_tests().into());
            }
            steps
        }
        Stages::NoTrybuild => vec![
            base().into(),
            platform_suite(host),
            scoped_nextest(host, "group(nested-cargo)").into(),
            Step::Note(
                "test --no-trybuild: SKIPPED the trybuild suites. Run them with: cargo xtask test --nested --nested-group trybuild".to_owned(),
            ),
        ],
    };
    if host == Host::Windows {
        let filter = match stages {
            Stages::All => None,
            Stages::Fast => Some(format!("not ({NESTED})")),
            Stages::Nested => Some(NESTED.to_owned()),
            Stages::NestedGroup(group) => Some(group.filterset().to_owned()),
            Stages::NoTrybuild => Some("not (group(trybuild))".to_owned()),
        };
        let mut driver = driver_tests();
        if let Some(filter) = filter {
            driver = driver.args(["-E", &filter]);
        }
        steps.insert(0, driver.into());
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
/// The iOS runner needs a local macOS host, so it is skipped
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
            "cross-typecheck: skipped the iOS runner (its C shim needs xcrun: macOS only; run it locally on macOS)"
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
/// facade's catalogs, neither on by default, are in [`TEST_SCOPE`]. The last step names
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
/// supported targets in local checks and the wasm-check CI job.
fn feature_matrix_plan(slice: Slice) -> Vec<Step> {
    let packages = match slice {
        Slice::All => "--workspace".to_owned(),
        Slice::Shard { index, count } => format!("--workspace --partition {index}/{count}"),
        Slice::Combinations => return Vec::new(),
    };
    hack_passes(&packages).map(Step::from).into()
}

/// Local `gpu-test` gate: engine readbacks and path-recording allocations, then the facade's
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
            "--test",
            "raster_backpressure_allocation",
        ])
        .args(["--locked", "--no-fail-fast", "--test-threads", "1"])
        .env("FLUI_REQUIRE_GPU", "1")
        .into(),
        Cmd::cargo(["nextest", "run", "-p", "flui", "--no-default-features"])
            .args(["--features", "gpu-readback-tests"])
            .args(["--test", "composited_layer_update_readback"])
            .args(["--locked", "--no-fail-fast", "--test-threads", "1"])
            .env("FLUI_REQUIRE_GPU", "1")
            .into(),
    ]
}

/// CI's `miri` job: selected ownership and containment contracts under Miri.
/// - flui-rendering `pipeline::owner`: owner failure and semantics publishing
///   families, including retirement of caught text panic payloads.
/// - flui-view `view_it::global_key_contract_matrix`: public GlobalKey migration
///   and duplicate-parent behavior (ADR-0050).
/// - flui-engine `surface_lease` and `cancelling_renderer_new`: the
///   wgpu-free surface-lease protocol and `Renderer::new`'s cancellation,
///   GPU-free by construction.
///
/// `CARGO_BUILD_WARNINGS=warn`, as the CI job sets it: nightly adds lints and
/// deprecations ahead of stable, and this run exists for undefined behavior,
/// not lints.
fn miri_plan() -> Vec<Step> {
    [
        ("flui-rendering", "--lib", "pipeline::owner"),
        ("flui-view", "--test view_it", "global_key_contract_matrix"),
        ("flui-engine", "--lib", "surface_lease"),
        ("flui-engine", "--lib", "cancelling_renderer_new"),
    ]
    .into_iter()
    .map(|(package, target, filter)| {
        Cmd::cargo(["+nightly", "miri", "test", "-p", package])
            .split(target)
            .args(["--locked", filter])
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

// Stages: a task's body, an error at the first failure, shared by the
// composite tasks.

fn gate_stage(runner: Runner) -> anyhow::Result<()> {
    checks::run(runner, false)?;
    runner.steps(&lint_plan())?;
    runner.in_process("doc-strict", || doc_strict::doc_strict(&parsed(NO_ARGS)?))
}

fn ci_stage(runner: Runner) -> anyhow::Result<()> {
    gate_stage(runner)?;
    runner.steps(&test_plan(Host::current(), Stages::All))?;
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask checks`: run the source checks that need no workspace build (the CI `checks` job).
///
/// `cargo fmt --check`, typos, taplo, and this crate's own checks in-process:
/// `docs-links` (lychee), `docs-paths`, `workspace` (its self-test, then the workspace),
/// `toolchain`, `wgsl` (its self-test, then the shaders), `paths-filter` and
/// `font-assets --package-list`. All of them run; exit 1 if any failed.
pub(crate) fn checks(args: &ChecksArgs) -> anyhow::Result<ExitCode> {
    done(checks::run(args.run.runner(), args.strict))
}

/// Arguments for `cargo xtask lint`.
#[derive(Debug, clap::Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask lint`: run clippy the way CI does.
pub(crate) fn lint(args: &LintArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&lint_plan()))
}

/// Arguments for `cargo xtask gate`.
#[derive(Debug, clap::Args)]
pub(crate) struct GateArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask gate`: `checks`, `lint` and `doc-strict`.
pub(crate) fn gate(args: &GateArgs) -> anyhow::Result<ExitCode> {
    done(gate_stage(args.run.runner()))
}

/// Nested suites that can run independently while retaining the same build scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum NestedGroup {
    /// Generated projects and native facade consumers.
    Native,
    /// Compiler diagnostics checked by trybuild.
    Trybuild,
}

impl NestedGroup {
    const fn filterset(self) -> &'static str {
        match self {
            Self::Native => "group(nested-cargo)",
            Self::Trybuild => "group(trybuild)",
        }
    }
}

/// Arguments for `cargo xtask test`.
#[derive(Debug, clap::Args)]
pub(crate) struct TestArgs {
    /// Leave out the nested tests (trybuild, generated projects, facade
    /// consumers): the quick local loop.
    #[arg(long)]
    fast: bool,
    /// Run only the nested tests.
    #[arg(long, conflicts_with = "fast")]
    nested: bool,
    /// Select one nested suite; without this option --nested runs both.
    #[arg(long, value_enum, requires = "nested")]
    nested_group: Option<NestedGroup>,
    /// Leave out only the trybuild suites (host-independent compiler output).
    #[arg(long, conflicts_with_all = ["fast", "nested"])]
    no_trybuild: bool,
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask test`: run the workspace test suite the way CI does.
pub(crate) fn test(args: &TestArgs) -> anyhow::Result<ExitCode> {
    let stages = if args.fast {
        Stages::Fast
    } else if args.nested {
        args.nested_group
            .map_or(Stages::Nested, Stages::NestedGroup)
    } else if args.no_trybuild {
        Stages::NoTrybuild
    } else {
        Stages::All
    };
    done(args.run.runner().steps(&test_plan(Host::current(), stages)))
}

/// Arguments for the native platform suite.
#[derive(Debug, clap::Args)]
pub(crate) struct PlatformTestArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// Run the platform suites without compiling unrelated framework crates.
pub(crate) fn platform_test(args: &PlatformTestArgs) -> anyhow::Result<ExitCode> {
    done(
        args.run
            .runner()
            .steps(&native_platform_plan(Host::current())),
    )
}

/// Arguments for the native CLI suite.
#[derive(Debug, clap::Args)]
pub(crate) struct CliTestArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// Run the same CLI suite in either native CI job or locally.
pub(crate) fn cli_test(args: &CliTestArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&cli_test_plan()))
}

/// Arguments for `cargo xtask build-all-targets`.
#[derive(Debug, clap::Args)]
pub(crate) struct BuildAllTargetsArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask build-all-targets`: link the examples and benches the test features reach (see [`build_all_targets`]).
pub(crate) fn build_all_targets_task(args: &BuildAllTargetsArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().run(&build_all_targets()))
}

/// Arguments for `cargo xtask ci`.
#[derive(Debug, clap::Args)]
pub(crate) struct CiArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask ci`: `gate`, `test` and the doctests: the local mirror of CI's required checks.
pub(crate) fn ci(args: &CiArgs) -> anyhow::Result<ExitCode> {
    done(ci_stage(args.run.runner()))
}

/// Arguments for `cargo xtask ci-full`.
#[derive(Debug, clap::Args)]
pub(crate) struct CiFullArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
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
    runner.run(&build_all_targets())?;
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask check-changed`: fmt, clippy and tests over the crates a change touches.
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
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask facade-combos`: build each supported facade feature combination on its own.
pub(crate) fn facade_combos(args: &FacadeCombosArgs) -> anyhow::Result<ExitCode> {
    done(facade::run(args.run.runner()))
}

/// Arguments for `cargo xtask cross-typecheck`.
#[derive(Debug, clap::Args)]
pub(crate) struct CrossTypecheckArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask wasm-check`: check and clippy the workspace for wasm32.
pub(crate) fn wasm_check(args: &WasmCheckArgs) -> anyhow::Result<ExitCode> {
    done(web::check(args.run.runner()))
}

/// Arguments for `cargo xtask wasm-link`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmLinkArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask wasm-link`: link the wasm demos and check their imports.
pub(crate) fn wasm_link(args: &WasmLinkArgs) -> anyhow::Result<ExitCode> {
    done(web::link(args.run.runner()))
}

/// Arguments for `cargo xtask wasm-test`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmTestArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask live-smoke`: real-window smoke test under X11 (or Wayland).
pub(crate) fn live_smoke(args: &LiveSmokeArgs) -> anyhow::Result<ExitCode> {
    done(live_smoke_stage(args.run.runner(), args.wayland))
}

/// Arguments for `cargo xtask gpu-test`.
#[derive(Debug, clap::Args)]
pub(crate) struct GpuTestArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
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
    pub(crate) run: RunOpts,
}

/// `cargo xtask bench-compile`: compile the benchmarks without running them.
pub(crate) fn bench_compile(args: &BenchCompileArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&bench_compile_plan()))
}

/// Arguments for `cargo xtask demo-snapshots`.
#[derive(Debug, clap::Args)]
pub(crate) struct DemoSnapshotsArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask demo-snapshots`: the demo layer snapshot suite.
///
/// Structural snapshots of the demo trees' painted layer trees, no GPU. With
/// `material` and `cupertino` on: the suite snapshots the Material and the
/// Cupertino demo, and the facade turns neither on by default. Review changed snapshots with
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
        "material,cupertino",
        "--test",
        "demo_layer_snapshots",
    ])))
}

/// Arguments for `cargo xtask clean-nested`.
#[derive(Debug, clap::Args)]
pub(crate) struct CleanNestedArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
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

    const SCOPE: &str = "--workspace --exclude flui-platform --locked --no-fail-fast --lib --bins --tests --features flui/material,flui/cupertino,flui/persist,flui-devtools/agent";

    fn native_host_suites_only_build_their_packages() {
        assert_eq!(
            lines(&native_platform_plan(Host::Windows)),
            [
                "$ cargo nextest run -p flui-platform --locked --no-fail-fast -E 'not (group(trybuild))'",
                "$ cargo nextest run -p flui-platform --locked --no-fail-fast --all-features -E 'not (group(trybuild))'",
            ]
        );
        assert_eq!(
            lines(&native_platform_plan(Host::MacOs)),
            [
                "$ FLUI_HEADLESS=1 cargo nextest run -p flui-platform --locked --no-fail-fast -E 'not (group(trybuild))'",
                "$ FLUI_HEADLESS=1 cargo nextest run -p flui-platform --locked --no-fail-fast --all-features -E 'not (group(trybuild))'",
            ]
        );
        assert_eq!(
            lines(&native_platform_plan(Host::Linux)),
            [
                "$ FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast -E 'not (group(trybuild))'"
            ]
        );
        assert_eq!(
            lines(&cli_test_plan()),
            ["$ cargo nextest run -p flui-cli --locked --no-fail-fast"]
        );
    }

    fn gpu_gate_requires_an_adapter_for_both_suites() {
        assert_eq!(
            lines(&gpu_test_plan()),
            [
                "$ FLUI_REQUIRE_GPU=1 cargo nextest run -p flui-engine --features testing --lib --test raster_backpressure_allocation --locked --no-fail-fast --test-threads 1",
                "$ FLUI_REQUIRE_GPU=1 cargo nextest run -p flui --no-default-features --features gpu-readback-tests --test composited_layer_update_readback --locked --no-fail-fast --test-threads 1",
            ]
        );
    }

    fn platform_compiler_checks_follow_each_host_and_stage() {
        for host in [Host::Linux, Host::Windows, Host::MacOs, Host::Other] {
            for (stage, expected) in [
                (Stages::All, 1),
                (Stages::Fast, 0),
                (Stages::Nested, 1),
                (Stages::NestedGroup(NestedGroup::Trybuild), 1),
                (Stages::NestedGroup(NestedGroup::Native), 0),
                (Stages::NoTrybuild, 0),
            ] {
                let commands = lines(&test_plan(host, stage));
                assert_eq!(
                    commands
                        .iter()
                        .filter(|line| line.contains("-p flui-platform --test compiler_guards"))
                        .count(),
                    expected,
                    "{host:?} {stage:?}: {commands:?}"
                );
            }
        }
    }

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

    fn test_runs_both_group_stages_and_the_platform_suite_per_host() {
        assert_eq!(
            lines(&test_plan(Host::Linux, Stages::All)),
            [
                format!("$ cargo nextest run {SCOPE} -E 'not (group(nested-cargo) | group(trybuild))'"),
                "$ FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast -E 'not (group(trybuild))'".to_owned(),
                format!("$ cargo nextest run {SCOPE} -E 'group(nested-cargo) | group(trybuild)'"),
                "$ cargo nextest run -p flui-platform --test compiler_guards --locked --no-fail-fast".to_owned(),
            ]
        );
        assert_eq!(
            lines(&test_plan(Host::Windows, Stages::All))[2],
            "$ cargo nextest run -p flui-platform --locked --all-features --no-fail-fast -E 'not (group(trybuild))'"
        );
        assert!(
            lines(&test_plan(Host::MacOs, Stages::All))[1]
                .starts_with("Skipping native flui-platform")
        );
        let fast = lines(&test_plan(Host::Linux, Stages::Fast));
        assert_eq!(fast.len(), 3);
        assert!(fast[2].starts_with("test --fast: SKIPPED the nested tests"));
        assert!(
            fast[2].ends_with("cargo xtask test --nested"),
            "{}",
            fast[2]
        );
        // Nested stages include the portable platform compiler leg.
        assert_eq!(
            lines(&test_plan(Host::Linux, Stages::Nested)),
            [
                format!("$ cargo nextest run {SCOPE} -E 'group(nested-cargo) | group(trybuild)'"),
                "$ cargo nextest run -p flui-platform --test compiler_guards --locked --no-fail-fast".to_owned(),
            ]
        );
        for (group, filter) in [
            (NestedGroup::Native, "group(nested-cargo)"),
            (NestedGroup::Trybuild, "group(trybuild)"),
        ] {
            let mut expected = vec![format!("$ cargo nextest run {SCOPE} -E '{filter}'")];
            if group == NestedGroup::Trybuild {
                expected.push("$ cargo nextest run -p flui-platform --test compiler_guards --locked --no-fail-fast".to_owned());
            }
            assert_eq!(
                lines(&test_plan(Host::Linux, Stages::NestedGroup(group))),
                expected
            );
        }
        // CI's Windows job: the host-specific nested group, not trybuild
        let windows = lines(&test_plan(Host::Windows, Stages::NoTrybuild));
        assert_eq!(windows.len(), 5);
        assert_eq!(
            windows[3],
            format!("$ cargo nextest run {SCOPE} -E 'group(nested-cargo)' --exclude xtask")
        );
        assert!(
            windows[4].starts_with("test --no-trybuild: SKIPPED the trybuild suites")
                && windows[4].ends_with("cargo xtask test --nested --nested-group trybuild"),
            "{}",
            windows[4]
        );
        // the example link reuses the test build: the same features
        assert_eq!(
            lines(&[build_all_targets().into()]),
            [format!(
                "$ cargo build --workspace --exclude xtask --all-targets --locked --features {TEST_FEATURES}"
            )]
        );
        assert!(SCOPE.ends_with(&format!("--features {TEST_FEATURES}")));
    }

    fn nested_group_requires_nested_and_preserves_existing_modes() {
        #[derive(clap::Parser)]
        struct TestCli {
            #[command(flatten)]
            args: TestArgs,
        }
        use clap::Parser as _;

        for group in ["native", "trybuild"] {
            assert!(TestCli::try_parse_from(["test", "--nested-group", group]).is_err());
            let parsed = TestCli::try_parse_from(["test", "--nested", "--nested-group", group])
                .expect("BUG: nested group must parse with --nested");
            assert!(parsed.args.nested);
            assert!(parsed.args.nested_group.is_some());
            for incompatible in ["--fast", "--no-trybuild"] {
                assert!(
                    TestCli::try_parse_from([
                        "test",
                        "--nested",
                        "--nested-group",
                        group,
                        incompatible,
                    ])
                    .is_err()
                );
            }
        }
        assert!(
            TestCli::try_parse_from(["test", "--nested", "--nested-group", "unknown"]).is_err()
        );
        for mode in ["--nested", "--fast", "--no-trybuild"] {
            let parsed = TestCli::try_parse_from(["test", mode])
                .expect("BUG: existing test mode must parse");
            assert_eq!(parsed.args.nested_group, None);
        }
    }

    fn feature_matrix_slices_parse() {
        assert_eq!(Slice::parse("all"), Ok(Slice::All));
        assert_eq!(Slice::parse("combinations"), Ok(Slice::Combinations));
        assert_eq!(Slice::parse("3/3"), Ok(Slice::Shard { index: 3, count: 3 }));
        for bad in ["0/3", "4/3", "1/0", "3", "a/b", "1/3/2"] {
            assert!(Slice::parse(bad).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn task_plans_contract() {
        crate::table_test::run_table(
            "task_plans_contract",
            &[
                (
                    "gpu_gate_requires_an_adapter_for_both_suites",
                    gpu_gate_requires_an_adapter_for_both_suites as fn(),
                ),
                (
                    "native_host_suites_only_build_their_packages",
                    native_host_suites_only_build_their_packages as fn(),
                ),
                (
                    "workflow_lint_runs_each_installed_linter_and_skips_the_rest",
                    workflow_lint_runs_each_installed_linter_and_skips_the_rest as fn(),
                ),
                (
                    "test_runs_both_group_stages_and_the_platform_suite_per_host",
                    test_runs_both_group_stages_and_the_platform_suite_per_host as fn(),
                ),
                (
                    "platform_compiler_checks_follow_each_host_and_stage",
                    platform_compiler_checks_follow_each_host_and_stage as fn(),
                ),
                (
                    "nested_group_requires_nested_and_preserves_existing_modes",
                    nested_group_requires_nested_and_preserves_existing_modes as fn(),
                ),
                (
                    "feature_matrix_slices_parse",
                    feature_matrix_slices_parse as fn(),
                ),
            ],
        );
    }
}
