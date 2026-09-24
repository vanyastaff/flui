//! `cargo xtask bench-collect`: run every criterion benchmark and save the
//! numbers under a named baseline (the weekly benchmark job).

use std::process::{Command, ExitCode};

use anyhow::Context;
use cargo_metadata::TargetKind;

use crate::util::{self, repo_root};

/// The cargo to run: the one driving this process when there is one.
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Arguments for `cargo xtask bench-collect`.
#[derive(Debug, clap::Args)]
pub(crate) struct BenchCollectArgs {
    /// Baseline name to save under `target/criterion/<bench>/<baseline>`.
    baseline: String,
}

/// A `[[bench]]` target: package, target name, and its required features.
type BenchTarget = (String, String, Vec<String>);

/// `cargo xtask bench-collect`: run every criterion bench target in the
/// workspace and save the numbers under a named baseline.
///
/// Why not plain `cargo bench --workspace -- --save-baseline X`: cargo's bench
/// target selection includes every lib/test target whose default `bench =
/// true` flag is set, and those run under libtest's harness, which rejects
/// criterion CLI flags ("Unrecognized option: 'save-baseline'"). Enumerating
/// the real `[[bench]]` targets (harness = false, criterion) sidesteps that
/// without sprinkling `bench = false` across two dozen manifests.
///
/// Targets with `required-features` (the GPU readback benches) are skipped:
/// explicitly selecting such a target without its features is a hard cargo
/// error, and this runs in GPU-less environments.
pub(crate) fn bench_collect(args: &BenchCollectArgs) -> anyhow::Result<ExitCode> {
    let metadata = util::metadata(&repo_root())?;
    let targets: Vec<BenchTarget> = metadata
        .packages
        .iter()
        .flat_map(|package| {
            package
                .targets
                .iter()
                .filter(|target| target.kind.contains(&TargetKind::Bench))
                .map(|target| {
                    (
                        package.name.to_string(),
                        target.name.clone(),
                        target.required_features.clone(),
                    )
                })
        })
        .collect();
    let (runnable, skipped): (Vec<&BenchTarget>, Vec<&BenchTarget>) = targets
        .iter()
        .partition(|(_, _, features)| features.is_empty());

    if runnable.is_empty() {
        eprintln!("error: no bench targets found");
        return Ok(ExitCode::FAILURE);
    }
    if !skipped.is_empty() {
        println!("skipping feature-gated bench targets:");
        for (package, bench, _) in &skipped {
            println!("  {package} {bench} (required-features)");
        }
    }
    for (package, bench, _) in &runnable {
        println!("== {package} / {bench}");
        let status = cargo()
            .args(["bench", "-p", package, "--bench", bench, "--locked", "--"])
            .args(["--noplot", "--save-baseline", &args.baseline])
            .current_dir(repo_root())
            .status()
            .context("spawning `cargo bench`")?;
        if !status.success() {
            return Ok(status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from));
        }
    }
    println!("baseline '{}' saved under target/criterion/", args.baseline);
    Ok(ExitCode::SUCCESS)
}
