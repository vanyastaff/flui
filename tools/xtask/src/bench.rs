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
    /// Also run the bench targets that declare `required-features`, with
    /// those features enabled. Those are the GPU benches, so this needs a
    /// host with a usable adapter.
    #[arg(long)]
    with_features: bool,
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
/// Targets with `required-features` (the GPU readback benches) are skipped
/// unless `--with-features` asks for them: explicitly selecting such a target
/// without its features is a hard cargo error, and the default run is meant
/// for GPU-less environments. With the flag, each runs with its own required
/// features enabled.
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
        .partition(|(_, _, features)| args.with_features || features.is_empty());

    if runnable.is_empty() {
        eprintln!("error: no bench targets found");
        return Ok(ExitCode::FAILURE);
    }
    if !skipped.is_empty() {
        println!("skipping feature-gated bench targets:");
        for (package, bench, _) in &skipped {
            println!("  {package} {bench} (required-features; --with-features runs it)");
        }
    }
    for target in &runnable {
        let (package, bench, _) = target;
        println!("== {package} / {bench}");
        let status = cargo()
            .args(bench_args(target, &args.baseline))
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

/// The `cargo` arguments that run one bench target and save its baseline,
/// enabling the target's required features when it has any.
fn bench_args((package, bench, features): &BenchTarget, baseline: &str) -> Vec<String> {
    let mut args: Vec<String> = ["bench", "-p", package, "--bench", bench, "--locked"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    if !features.is_empty() {
        args.push("--features".to_owned());
        args.push(features.join(","));
    }
    args.extend(
        ["--", "--noplot", "--save-baseline", baseline]
            .into_iter()
            .map(str::to_owned),
    );
    args
}

#[cfg(test)]
mod tests {
    use super::bench_args;

    #[test]
    fn a_feature_gated_target_runs_with_its_features() {
        let target = (
            "flui-engine".to_owned(),
            "render_throughput".to_owned(),
            vec!["testing".to_owned(), "extra".to_owned()],
        );
        assert_eq!(
            bench_args(&target, "base"),
            [
                "bench",
                "-p",
                "flui-engine",
                "--bench",
                "render_throughput",
                "--locked",
                "--features",
                "testing,extra",
                "--",
                "--noplot",
                "--save-baseline",
                "base"
            ]
        );
        let plain = ("flui-layer".to_owned(), "damage_diff".to_owned(), Vec::new());
        assert!(!bench_args(&plain, "base").contains(&"--features".to_owned()));
    }
}
