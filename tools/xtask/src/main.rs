//! Repository automation for FLUI, run as `cargo xtask <command>`.
//!
//! Everything local gates and CI need beyond cargo itself lives here, in Rust,
//! so it runs the same on every host and is checked by the same compiler and
//! lints as the framework. Each module owns one command family.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
mod util;

#[cfg(test)]
mod table_test;

mod tasks;

mod android_dex;
mod api_closure;
mod bench;
mod change_scope;
mod changelog;
mod doc_strict;
mod docs_links;
mod doctor;
mod fonts;
mod icon_import;
mod key_vocabulary;
mod perf;
mod platform_floor;
mod public_api;
mod recording_vocabulary;
mod send_positions;
mod toolchain;
mod wasm;
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
    /// Run the Wayland cases CI's Weston cannot, against a compositor that offers their subjects.
    WaylandOptional(tasks::WaylandOptionalArgs),
    /// Link the examples and benches with the test suite's features.
    BuildAllTargets(tasks::BuildAllTargetsArgs),
    /// `gate`, `test` and the doctests: the local mirror of CI's required checks.
    Ci(tasks::CiArgs),
    /// `ci` plus every heavy CI job this host can run.
    CiFull(tasks::CiFullArgs),
    /// Check the dependency graph (cargo-deny).
    Deps(tasks::DepsArgs),
    /// Clippy every feature on its own (cargo-hack).
    FeatureMatrix(tasks::FeatureMatrixArgs),
    /// Clippy flui-native once per single feature (the part of feature-matrix pull requests run).
    NativeFeatures(tasks::NativeFeaturesArgs),
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
    /// The GPU readback suites.
    GpuTest(tasks::GpuTestArgs),
    /// The unsafe-code paths under Miri.
    Miri(tasks::MiriArgs),
    /// Run the fuzz targets under libFuzzer.
    Fuzz(tasks::FuzzArgs),
    /// Remove the nested-cargo test caches under the target directory.
    CleanNested(tasks::CleanNestedArgs),
    /// Print the lane and the packages a change touches (CI's `plan`).
    Affected(change_scope::AffectedArgs),
    /// List the expectations a change edits; with `--body`, fail on each the body leaves unnamed (ADR-0331).
    Expectations(change_scope::ExpectationsArgs),
    /// Check that no include_str! target is classified as docs-only.
    PathsFilter(change_scope::PathsFilterArgs),
    /// The `ci` aggregator job: every job is gated and exactly the planned ones skipped.
    CiVerify(change_scope::CiVerifyArgs),
    /// Check that every declared MSRV matches rust-toolchain.toml.
    Toolchain(toolchain::ToolchainArgs),
    /// Check that every place the public API names `Send` or `Sync` is listed under
    /// ADR-0323 §3.
    SendPositions(send_positions::SendPositionsArgs),
    /// Check the facade's public API against its committed snapshot `api/flui.txt` (ADR-0324).
    PublicApi(public_api::PublicApiArgs),
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
    /// Import vector animations as `AnimatedIconData` constants, or check the bundled Material ones.
    IconImport(icon_import::IconImportArgs),
    /// Check (or --write) the W3C key tables generated from keyboard-types (ADR-0089 §4).
    KeyVocabulary(key_vocabulary::KeyVocabularyArgs),
    /// Check (or --write) the table of variants an input recording carries (ADR-0215 §2).
    RecordingVocabulary(recording_vocabulary::RecordingVocabularyArgs),
    /// Check (or --write) the Android `FluiView` dex against its Java sources (ADR-0247).
    AndroidDex(android_dex::AndroidDexArgs),
    /// Check (or --write) the floors generated from platform-support.toml (ADR-0244).
    PlatformFloor(platform_floor::PlatformFloorArgs),
    /// Check what Stable crates' public API names from upstream crates (ADR-0089).
    ApiClosure(api_closure::ApiClosureArgs),
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
        Command::PlatformTest(args) => tasks::platform_test(&args),
        Command::CliTest(args) => tasks::cli_test(&args),
        Command::WaylandOptional(args) => tasks::wayland_optional(&args),
        Command::BuildAllTargets(args) => tasks::build_all_targets_task(&args),
        Command::Ci(args) => tasks::ci(&args),
        Command::CiFull(args) => tasks::ci_full(&args),
        Command::Deps(args) => tasks::deps(&args),
        Command::FeatureMatrix(args) => tasks::feature_matrix(&args),
        Command::NativeFeatures(args) => tasks::native_features(&args),
        Command::FacadeCombos(args) => tasks::facade_combos(&args),
        Command::CrossTypecheck(args) => tasks::cross_typecheck(&args),
        Command::WasmCheck(args) => tasks::wasm_check(&args),
        Command::WasmLink(args) => tasks::wasm_link(&args),
        Command::WasmTest(args) => tasks::wasm_test(&args),
        Command::GpuTest(args) => tasks::gpu_test(&args),
        Command::Miri(args) => tasks::miri(&args),
        Command::Fuzz(args) => tasks::fuzz(&args),
        Command::CleanNested(args) => tasks::clean_nested(&args),
        Command::Affected(args) => change_scope::affected(&args),
        Command::Expectations(args) => change_scope::expectations(&args),
        Command::PathsFilter(args) => change_scope::paths_filter(&args),
        Command::CiVerify(args) => change_scope::ci_verify(&args),
        Command::Toolchain(args) => toolchain::toolchain(&args),
        Command::SendPositions(args) => send_positions::send_positions(&args),
        Command::PublicApi(args) => public_api::public_api(&args),
        Command::WasmImports(args) => wasm::wasm_imports(&args),
        Command::WasmTestCrates(args) => wasm::wasm_test_crates(&args),
        Command::LockedVersion(args) => wasm::locked_version(&args),
        Command::DocStrict(args) => doc_strict::doc_strict(&args),
        Command::DocsLinks(args) => docs_links::docs_links(&args),
        Command::FontAssets(args) => fonts::font_assets(&args),
        Command::IconImport(args) => icon_import::icon_import(&args),
        Command::KeyVocabulary(args) => key_vocabulary::key_vocabulary(&args),
        Command::RecordingVocabulary(args) => recording_vocabulary::recording_vocabulary(&args),
        Command::AndroidDex(args) => android_dex::android_dex(&args),
        Command::PlatformFloor(args) => platform_floor::platform_floor(&args),
        Command::ApiClosure(args) => api_closure::api_closure(&args),
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
