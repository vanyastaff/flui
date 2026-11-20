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

mod adr_numbers;
mod android_dex;
mod bench;
mod changelog;
mod doctor;
mod fonts;
mod icon_import;
mod key_vocabulary;
mod platform_floor;
mod recording_vocabulary;
mod toolchain;
mod unmounted_tests;
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
    /// Run the fuzz targets under libFuzzer.
    Fuzz(tasks::FuzzArgs),
    /// Remove the nested-cargo test caches under the target directory.
    CleanNested(tasks::CleanNestedArgs),
    /// Check that every declared MSRV matches rust-toolchain.toml.
    Toolchain(toolchain::ToolchainArgs),
    /// Check that every `tests/*.rs` file of an `autotests = false` crate is compiled.
    UnmountedTests(unmounted_tests::UnmountedTestsArgs),
    /// Check that no two records in `docs/adr/` share a number.
    AdrNumbers(adr_numbers::AdrNumbersArgs),
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
    /// Validate changelog.d fragments; --write merges them into CHANGELOG.md and deletes them.
    Changelog(changelog::ChangelogArgs),
    /// List missing tools for the local checks.
    Doctor(doctor::DoctorArgs),
    /// Collect benchmark results.
    BenchCollect(bench::BenchCollectArgs),
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
        Command::Fuzz(args) => tasks::fuzz(&args),
        Command::CleanNested(args) => tasks::clean_nested(&args),
        Command::Toolchain(args) => toolchain::toolchain(&args),
        Command::UnmountedTests(args) => unmounted_tests::unmounted_tests(&args),
        Command::AdrNumbers(args) => adr_numbers::adr_numbers(&args),
        Command::FontAssets(args) => fonts::font_assets(&args),
        Command::IconImport(args) => icon_import::icon_import(&args),
        Command::KeyVocabulary(args) => key_vocabulary::key_vocabulary(&args),
        Command::RecordingVocabulary(args) => recording_vocabulary::recording_vocabulary(&args),
        Command::AndroidDex(args) => android_dex::android_dex(&args),
        Command::PlatformFloor(args) => platform_floor::platform_floor(&args),
        Command::Changelog(args) => changelog::changelog(&args),
        Command::Doctor(args) => doctor::doctor(&args),
        Command::BenchCollect(args) => bench::bench_collect(&args),
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
