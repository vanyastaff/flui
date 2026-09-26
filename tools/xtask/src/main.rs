//! Repository automation for FLUI, run as `cargo xtask <command>`.
//!
//! Everything local gates and CI need beyond cargo itself lives here, in Rust,
//! so it runs the same on every host and is checked by the same compiler and
//! lints as the framework. Each module owns one command family.

mod util;

mod device;
mod tasks;

mod bench;
mod change_scope;
mod changelog;
mod doc_strict;
mod docs_links;
mod doctor;
mod file_length;
mod fonts;
mod globals;
mod markers;
mod module_dag;
mod perf;
mod ratchet;
mod toolchain;
mod wasm;
mod wgsl;
mod workspace;

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
    /// `gate`, `test` and the doctests: the local mirror of CI's required checks.
    Ci(tasks::CiArgs),
    /// `ci` plus every heavy CI job this host can run.
    CiFull(tasks::CiFullArgs),
    /// fmt, clippy and tests over the crates a change touches (CI's fast lane).
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
    /// Run a macOS or iOS device check (tools/device-checks).
    Device(device::DeviceArgs),
    /// Check crate layers, manifests, test reachability and ADR numbers.
    Workspace(workspace::WorkspaceArgs),
    /// Check that no crate reaches what its tier forbids (ADR-0081 §2).
    Reach(workspace::ReachArgs),
    /// Check the declared import direction between a crate's top-level modules.
    ModuleDag(module_dag::ModuleDagArgs),
    /// Print the packages a change touches (CI fast lane, `check-changed`).
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(error) = util::built_from_this_checkout() {
        eprintln!("xtask: {error:#}");
        return ExitCode::from(2);
    }
    let result = match cli.command {
        Command::Checks(args) => tasks::checks(&args),
        Command::Lint(args) => tasks::lint(&args),
        Command::Gate(args) => tasks::gate(&args),
        Command::Test(args) => tasks::test(&args),
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
        Command::FontAssets(args) => fonts::font_assets(&args),
        Command::FileLength(args) => file_length::file_length(&args),
        Command::Markers(args) => markers::markers(&args),
        Command::Changelog(args) => changelog::changelog(&args),
        Command::Doctor(args) => doctor::doctor(&args),
        Command::BenchCollect(args) => bench::bench_collect(&args),
        Command::Perf(args) => perf::perf(&args),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("xtask: {error:#}");
            ExitCode::FAILURE
        }
    }
}
