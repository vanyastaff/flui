//! Repository automation for FLUI, run as `cargo xtask <command>`.
//!
//! Everything local gates and CI need beyond cargo itself lives here, in Rust,
//! so it runs the same on every host and is checked by the same compiler and
//! lints as the framework. Each module owns one command family.

mod util;

#[cfg(test)]
mod table_test;

mod device;
mod tasks;

mod bench;
mod change_scope;
mod changelog;
mod doc_strict;
mod docs_links;
mod docs_paths;
mod doctor;
mod file_length;
mod fonts;
mod globals;
mod host_lock;
mod markers;
mod module_dag;
mod perf;
mod ratchet;
mod toolchain;
mod wasm;
mod wgsl;
mod workspace;
mod worktree;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "FLUI repository automation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the source checks that need no workspace build (the CI `checks` job).
    Checks(tasks::ChecksArgs),
    /// Run clippy the way CI does.
    Lint(tasks::LintArgs),
    /// `checks`, `lint` and `doc-strict`.
    Gate(tasks::GateArgs),
    /// Run the workspace test suite the way CI does.
    Test(tasks::TestArgs),
    /// Run the native platform suites on this host.
    PlatformTest(tasks::PlatformTestArgs),
    /// Run the CLI suite on this host.
    CliTest(tasks::CliTestArgs),
    /// Link the examples and benches with the test suite's features.
    BuildAllTargets(tasks::BuildAllTargetsArgs),
    /// `gate`, `test` and the doctests: the local mirror of CI's required checks.
    Ci(tasks::CiArgs),
    /// `ci` plus every heavy CI job this host can run.
    CiFull(tasks::CiFullArgs),
    /// fmt, clippy and tests over the crates a change touches and their dependents.
    CheckChanged(tasks::CheckChangedArgs),
    /// Check the dependency graph and the manifests (cargo-deny, cargo-shear).
    Deps(tasks::DepsArgs),
    /// Clippy every feature on its own (cargo-hack).
    FeatureMatrix(tasks::FeatureMatrixArgs),
    /// Build each supported facade feature combination on its own.
    FacadeCombos(tasks::FacadeCombosArgs),
    /// Clippy the Win32, AppKit, Android and iOS backends without linking.
    CrossTypecheck(tasks::CrossTypecheckArgs),
    /// Check and clippy the workspace for wasm32.
    WasmCheck(tasks::WasmCheckArgs),
    /// Link the wasm demos and check their imports.
    WasmLink(tasks::WasmLinkArgs),
    /// Run the wasm32 tests under wasm-bindgen-test-runner.
    WasmTest(tasks::WasmTestArgs),
    /// Real-window smoke test under X11 (or Wayland).
    LiveSmoke(tasks::LiveSmokeArgs),
    /// The GPU readback suites.
    GpuTest(tasks::GpuTestArgs),
    /// The unsafe-code paths under Miri.
    Miri(tasks::MiriArgs),
    /// Compile the benchmarks without running them.
    BenchCompile(tasks::BenchCompileArgs),
    /// The demo layer snapshot suite.
    DemoSnapshots(tasks::DemoSnapshotsArgs),
    /// Remove the nested-cargo test caches under the target directory.
    CleanNested(tasks::CleanNestedArgs),
    /// Run a macOS, iOS or Windows device check (drivers in tools/device-checks).
    Device(device::DeviceArgs),
    /// Check crate layers, manifests, test reachability and ADR numbers.
    Workspace(workspace::WorkspaceArgs),
    /// Check that no crate reaches what its tier forbids (ADR-0081 §2).
    Reach(workspace::ReachArgs),
    /// Check the declared import direction between a crate's top-level modules.
    ModuleDag(module_dag::ModuleDagArgs),
    /// Print the lane and the packages a change touches (CI's `plan`, `check-changed`).
    Affected(change_scope::AffectedArgs),
    /// Check that no include_str! target is classified as docs-only.
    PathsFilter(change_scope::PathsFilterArgs),
    /// The `ci` aggregator job: every job is gated and exactly the planned ones skipped.
    CiVerify(change_scope::CiVerifyArgs),
    /// Check that every declared MSRV matches rust-toolchain.toml.
    Toolchain(toolchain::ToolchainArgs),
    /// Check WGSL derivative uniformity.
    Wgsl(wgsl::WgslArgs),
    /// Check that every process-global has a reviewed entry (ADR-0097).
    Globals(globals::GlobalsArgs),
    /// Check a linked wasm module's imports against the allowlist.
    WasmImports(wasm::WasmImportsArgs),
    /// Print the crates whose tests run on wasm32.
    WasmTestCrates(wasm::WasmTestCratesArgs),
    /// Print a package's version from Cargo.lock.
    LockedVersion(wasm::LockedVersionArgs),
    /// Build the workspace docs with warnings denied.
    DocStrict(doc_strict::DocStrictArgs),
    /// Check the links in the repository's markdown (lychee, offline).
    DocsLinks(docs_links::DocsLinksArgs),
    /// Check the repository paths, packages and llms.txt links the docs name.
    DocsPaths(docs_paths::DocsPathsArgs),
    /// Check or list the bundled font assets.
    FontAssets(fonts::FontAssetsArgs),
    /// Check that no .rs file exceeds 3000 production lines (ADR-0081 §5).
    FileLength(file_length::FileLengthArgs),
    /// Check for process markers outside the archival roots (ADR-0078 §4).
    Markers(markers::MarkersArgs),
    /// Validate changelog.d fragments; --write merges them into CHANGELOG.md and deletes them.
    Changelog(changelog::ChangelogArgs),
    /// List missing tools for `ci` / `ci-full`.
    Doctor(doctor::DoctorArgs),
    /// Collect benchmark results.
    BenchCollect(bench::BenchCollectArgs),
    /// Run the counted perf scenarios and compare them with the baseline.
    Perf(perf::PerfArgs),
    /// Create, list and prune task worktrees under `.worktrees/`.
    Worktree(worktree::WorktreeArgs),
}

impl Command {
    /// Whether this command builds or tests the workspace, or deletes what
    /// such a run uses, and so waits for any other such run on the host
    /// ([`host_lock`]). A dry run builds and deletes nothing.
    ///
    /// Every command is classified here, with no catch-all, so a new one
    /// must be placed on one side or the other.
    fn is_heavy(&self) -> bool {
        let run = match self {
            // Builds, tests, docs or benches of the workspace.
            Self::Lint(args) => args.run,
            Self::Gate(args) => args.run,
            Self::Test(args) => args.run,
            Self::PlatformTest(args) => args.run,
            Self::CliTest(args) => args.run,
            Self::BuildAllTargets(args) => args.run,
            Self::Ci(args) => args.run,
            Self::CiFull(args) => args.run,
            Self::CheckChanged(args) => args.run,
            Self::FeatureMatrix(args) => args.run,
            Self::FacadeCombos(args) => args.run,
            Self::CrossTypecheck(args) => args.run,
            Self::WasmCheck(args) => args.run,
            Self::WasmLink(args) => args.run,
            Self::WasmTest(args) => args.run,
            Self::LiveSmoke(args) => args.run,
            Self::GpuTest(args) => args.run,
            Self::Miri(args) => args.run,
            Self::BenchCompile(args) => args.run,
            Self::DemoSnapshots(args) => args.run,
            // Deletes the nested-test caches a running test would use.
            Self::CleanNested(args) => args.run,
            Self::DocStrict(_) | Self::Device(_) | Self::BenchCollect(_) => return true,
            // The planted-fixture comparison builds nothing.
            Self::Perf(args) => return !args.self_test,
            // `prune` deletes worktrees and their target directories.
            Self::Worktree(args) => return args.deletes(),
            // Source checks, metadata queries and bookkeeping: they build at
            // most xtask itself.
            Self::Checks(_)
            | Self::Deps(_)
            | Self::Workspace(_)
            | Self::Reach(_)
            | Self::ModuleDag(_)
            | Self::Affected(_)
            | Self::PathsFilter(_)
            | Self::CiVerify(_)
            | Self::Toolchain(_)
            | Self::Wgsl(_)
            | Self::Globals(_)
            | Self::WasmImports(_)
            | Self::WasmTestCrates(_)
            | Self::LockedVersion(_)
            | Self::DocsLinks(_)
            | Self::DocsPaths(_)
            | Self::FontAssets(_)
            | Self::FileLength(_)
            | Self::Markers(_)
            | Self::Changelog(_)
            | Self::Doctor(_) => return false,
        };
        !run.dry_run
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(error) = util::built_from_this_checkout() {
        eprintln!("xtask: {error:#}");
        return ExitCode::from(2);
    }
    let _heavy_run = cli.command.is_heavy().then(|| {
        host_lock::HeavyRunLock::acquire(
            &host_lock::LockSettings::from_env(),
            &host_lock::Holder::this_run(),
            &mut std::io::stderr(),
        )
    });
    let result = match cli.command {
        Command::Checks(args) => tasks::checks(&args),
        Command::Lint(args) => tasks::lint(&args),
        Command::Gate(args) => tasks::gate(&args),
        Command::Test(args) => tasks::test(&args),
        Command::PlatformTest(args) => tasks::platform_test(&args),
        Command::CliTest(args) => tasks::cli_test(&args),
        Command::BuildAllTargets(args) => tasks::build_all_targets_task(&args),
        Command::Ci(args) => tasks::ci(&args),
        Command::CiFull(args) => tasks::ci_full(&args),
        Command::CheckChanged(args) => tasks::check_changed(&args),
        Command::Deps(args) => tasks::deps(&args),
        Command::FeatureMatrix(args) => tasks::feature_matrix(&args),
        Command::FacadeCombos(args) => tasks::facade_combos(&args),
        Command::CrossTypecheck(args) => tasks::cross_typecheck(&args),
        Command::WasmCheck(args) => tasks::wasm_check(&args),
        Command::WasmLink(args) => tasks::wasm_link(&args),
        Command::WasmTest(args) => tasks::wasm_test(&args),
        Command::LiveSmoke(args) => tasks::live_smoke(&args),
        Command::GpuTest(args) => tasks::gpu_test(&args),
        Command::Miri(args) => tasks::miri(&args),
        Command::BenchCompile(args) => tasks::bench_compile(&args),
        Command::DemoSnapshots(args) => tasks::demo_snapshots(&args),
        Command::CleanNested(args) => tasks::clean_nested(&args),
        Command::Device(args) => device::device(&args),
        Command::Workspace(args) => workspace::workspace(&args),
        Command::Reach(args) => workspace::reach(&args),
        Command::ModuleDag(args) => module_dag::module_dag(&args),
        Command::Affected(args) => change_scope::affected(&args),
        Command::PathsFilter(args) => change_scope::paths_filter(&args),
        Command::CiVerify(args) => change_scope::ci_verify(&args),
        Command::Toolchain(args) => toolchain::toolchain(&args),
        Command::Wgsl(args) => wgsl::wgsl(&args),
        Command::Globals(args) => globals::globals(&args),
        Command::WasmImports(args) => wasm::wasm_imports(&args),
        Command::WasmTestCrates(args) => wasm::wasm_test_crates(&args),
        Command::LockedVersion(args) => wasm::locked_version(&args),
        Command::DocStrict(args) => doc_strict::doc_strict(&args),
        Command::DocsLinks(args) => docs_links::docs_links(&args),
        Command::DocsPaths(args) => docs_paths::docs_paths(&args),
        Command::FontAssets(args) => fonts::font_assets(&args),
        Command::FileLength(args) => file_length::file_length(&args),
        Command::Markers(args) => markers::markers(&args),
        Command::Changelog(args) => changelog::changelog(&args),
        Command::Doctor(args) => doctor::doctor(&args),
        Command::BenchCollect(args) => bench::bench_collect(&args),
        Command::Perf(args) => perf::perf(&args),
        Command::Worktree(args) => worktree::worktree(&args),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("xtask: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::Cli;

    /// Every command, as typed, and whether it queues behind the run lock.
    const COMMANDS: &[(&str, bool)] = &[
        ("lint", true),
        ("gate", true),
        ("test", true),
        ("test --fast", true),
        ("platform-test", true),
        ("cli-test", true),
        ("build-all-targets", true),
        ("ci", true),
        ("ci-full", true),
        ("check-changed", true),
        ("feature-matrix", true),
        ("facade-combos", true),
        ("cross-typecheck", true),
        ("wasm-check", true),
        ("wasm-link", true),
        ("wasm-test", true),
        ("live-smoke", true),
        ("gpu-test", true),
        ("miri", true),
        ("bench-compile", true),
        ("demo-snapshots", true),
        ("doc-strict", true),
        ("device macos-close-path", true),
        ("bench-collect baseline", true),
        ("perf", true),
        ("perf --self-test", false),
        ("gate --dry-run", false),
        ("check-changed --dry-run", false),
        ("ci --dry-run", false),
        ("lint --dry-run", false),
        ("checks", false),
        ("deps", false),
        ("clean-nested", true),
        ("clean-nested --dry-run", false),
        ("workspace", false),
        ("reach", false),
        ("module-dag", false),
        ("affected", false),
        ("paths-filter", false),
        ("ci-verify", false),
        ("toolchain", false),
        ("wgsl", false),
        ("globals", false),
        ("wasm-imports module.wasm", false),
        ("wasm-test-crates", false),
        ("locked-version wgpu", false),
        ("docs-links", false),
        ("docs-paths", false),
        ("font-assets", false),
        ("file-length", false),
        ("markers", false),
        ("changelog --check", false),
        ("doctor", false),
        ("worktree list", false),
        ("worktree new area/slug", false),
        ("worktree prune", true),
        ("worktree prune --dry-run", false),
    ];

    #[test]
    fn heavy_commands_take_the_run_lock() {
        let wrong: Vec<String> = COMMANDS
            .iter()
            .filter_map(|&(line, heavy)| {
                let argv = std::iter::once("xtask").chain(line.split_whitespace());
                match Cli::try_parse_from(argv) {
                    Ok(cli) if cli.command.is_heavy() == heavy => None,
                    Ok(_) => Some(format!("`{line}`: expected heavy = {heavy}")),
                    Err(error) => Some(format!("`{line}`: {error}")),
                }
            })
            .collect();
        assert!(wrong.is_empty(), "misclassified: {wrong:#?}");
        let covered: std::collections::BTreeSet<&str> = COMMANDS
            .iter()
            .filter_map(|(line, _)| line.split_whitespace().next())
            .collect();
        let missing: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_owned())
            .filter(|name| !covered.contains(name.as_str()))
            .collect();
        assert!(missing.is_empty(), "commands with no row: {missing:?}");
    }
}
