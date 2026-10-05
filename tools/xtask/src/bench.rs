//! `cargo xtask bench-collect`: run every criterion benchmark and save the
//! numbers under a named baseline (the weekly benchmark job).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::Context;
use cargo_metadata::TargetKind;
use serde::Deserialize;

use crate::util::{self, repo_root};

/// The cargo to run: the one driving this process when there is one.
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Arguments for `cargo xtask bench-collect`.
#[derive(Debug, clap::Args)]
pub(crate) struct BenchCollectArgs {
    /// Fresh baseline name to save under the Cargo target directory's `criterion/`.
    baseline: String,
    /// Also run the bench targets that declare `required-features`, with
    /// those features enabled. Those are the GPU benches, so this needs a
    /// host with a usable adapter. Every selected target must produce samples.
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
    let output = metadata
        .target_directory
        .into_std_path_buf()
        .join("criterion");
    collect_baseline(args, &targets, &output, |target, home| {
        cargo()
            .args(bench_args(target, &args.baseline))
            .env("CRITERION_HOME", home)
            .current_dir(repo_root())
            .status()
            .map(|status| status.success())
            .context("spawning `cargo bench`")
    })
}

/// Fresh output is the execution evidence; process success alone is insufficient.
fn collect_baseline(
    args: &BenchCollectArgs,
    targets: &[BenchTarget],
    output: &Path,
    mut run: impl FnMut(&BenchTarget, &Path) -> anyhow::Result<bool>,
) -> anyhow::Result<ExitCode> {
    anyhow::ensure!(
        !args.baseline.is_empty()
            && !matches!(args.baseline.as_str(), "." | ".." | "new" | "change")
            && !args.baseline.contains(['/', '\\', ':']),
        "baseline must be a single directory name other than Criterion's 'new' and 'change'"
    );
    anyhow::ensure!(
        saved_directories(output, &args.baseline)?.is_empty(),
        "baseline '{}' already exists; choose a fresh name to keep old measurements separate",
        args.baseline
    );
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
    let mut measured = 0;
    let mut unavailable = 0;
    for target in &runnable {
        let (package, bench, _) = target;
        println!("== {package} / {bench}");
        let home = util::ScratchDir::new("criterion")?;
        if !run(target, home.path())? {
            eprintln!("failed: {package} / {bench}");
            return Ok(ExitCode::FAILURE);
        }
        let workloads = read_workloads(home.path(), &args.baseline)
            .with_context(|| format!("validating {package} / {bench}"))?;
        if workloads.is_empty() {
            println!(
                "unavailable: {package} / {bench} (process succeeded but produced no samples)"
            );
            unavailable += 1;
            continue;
        }
        // Validate every workload before publishing any of this target's artifacts.
        for workload in workloads {
            publish_workload(home.path(), output, &workload.directory)?;
            println!("measured: {} ({} samples)", workload.id, workload.samples);
            measured += 1;
        }
    }
    println!(
        "baseline '{}': {measured} measured workloads, {unavailable} unavailable targets, {} feature-gated targets skipped; output {}",
        args.baseline,
        skipped.len(),
        output.display()
    );
    if measured == 0 || (args.with_features && unavailable != 0) {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Find only Criterion artifact directories, not similarly named group directories.
fn saved_directories(home: &Path, baseline: &str) -> anyhow::Result<BTreeSet<PathBuf>> {
    if !home.exists() {
        return Ok(BTreeSet::new());
    }
    let mut directories = BTreeSet::new();
    for entry in walkdir::WalkDir::new(home) {
        let entry = entry.context("reading Criterion output")?;
        if !matches!(
            entry.file_name().to_str(),
            Some("sample.json" | "estimates.json" | "benchmark.json")
        ) {
            continue;
        }
        if let Some(parent) = entry.path().parent()
            && parent.file_name().is_some_and(|name| name == baseline)
        {
            anyhow::ensure!(
                entry.file_type().is_file(),
                "Criterion artifact is not a regular file: {}",
                entry.path().display()
            );
            directories.insert(parent.to_path_buf());
        }
    }
    Ok(directories)
}

#[derive(Deserialize)]
struct Samples {
    iters: Vec<f64>,
    times: Vec<f64>,
}

#[derive(Deserialize)]
struct Identity {
    full_id: String,
}

#[derive(Deserialize)]
struct Estimate {
    point_estimate: f64,
}

#[derive(Deserialize)]
struct Estimates {
    mean: Estimate,
    median: Estimate,
}

struct Workload {
    directory: PathBuf,
    id: String,
    samples: usize,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn read_workloads(home: &Path, baseline: &str) -> anyhow::Result<Vec<Workload>> {
    saved_directories(home, baseline)?
        .into_iter()
        .map(|directory| {
            let samples: Samples = read_json(&directory.join("sample.json"))?;
            anyhow::ensure!(
                !samples.iters.is_empty()
                    && samples.iters.len() == samples.times.len()
                    && samples
                        .iters
                        .iter()
                        .chain(&samples.times)
                        .all(|value| value.is_finite() && *value > 0.0),
                "invalid or empty samples in {}",
                directory.display()
            );
            let estimates: Estimates = read_json(&directory.join("estimates.json"))?;
            anyhow::ensure!(
                [
                    estimates.mean.point_estimate,
                    estimates.median.point_estimate
                ]
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
                "invalid estimates in {}",
                directory.display()
            );
            let identity: Identity = read_json(&directory.join("benchmark.json"))?;
            anyhow::ensure!(
                !identity.full_id.is_empty(),
                "missing workload identity in {}",
                directory.display()
            );
            Ok(Workload {
                directory,
                id: identity.full_id,
                samples: samples.iters.len(),
            })
        })
        .collect()
}

fn publish_workload(home: &Path, output: &Path, directory: &Path) -> anyhow::Result<()> {
    let destination = output.join(directory.strip_prefix(home)?);
    // An empty directory holds no measurement: a publication that failed
    // before copying, or an interrupted run, must not block the next attempt.
    let empty = destination.is_dir() && std::fs::read_dir(&destination)?.next().is_none();
    anyhow::ensure!(
        empty || !destination.exists(),
        "baseline destination already exists: {}",
        destination.display()
    );
    std::fs::create_dir_all(&destination)
        .with_context(|| format!("creating {}", destination.display()))?;
    let copied = copy_artifacts(directory, &destination);
    if copied.is_err() {
        // Best effort: a partial copy must not look like a measurement.
        let _ = std::fs::remove_dir_all(&destination);
    }
    copied
}

fn copy_artifacts(directory: &Path, destination: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        anyhow::ensure!(
            entry.file_type()?.is_file(),
            "unexpected Criterion artifact: {}",
            entry.path().display()
        );
        std::fs::copy(entry.path(), destination.join(entry.file_name()))
            .with_context(|| format!("publishing {}", entry.path().display()))?;
    }
    Ok(())
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
    use super::{BenchCollectArgs, BenchTarget, bench_args, collect_baseline};
    use crate::util::ScratchDir;
    use std::path::Path;
    use std::process::ExitCode;

    fn args(with_features: bool) -> BenchCollectArgs {
        BenchCollectArgs {
            baseline: "fixture".to_owned(),
            with_features,
        }
    }

    fn target(name: &str) -> BenchTarget {
        ("fixture-package".to_owned(), name.to_owned(), Vec::new())
    }

    fn artifacts(home: &Path, workload: &str, samples: &str) {
        let directory = home.join(workload).join("fixture");
        std::fs::create_dir_all(&directory).expect("fixture directory");
        std::fs::write(directory.join("sample.json"), samples).expect("fixture samples");
        std::fs::write(
            directory.join("estimates.json"),
            r#"{"mean":{"point_estimate":12.0},"median":{"point_estimate":12.0}}"#,
        )
        .expect("fixture estimates");
        std::fs::write(
            directory.join("benchmark.json"),
            serde_json::to_vec(&serde_json::json!({"full_id": workload}))
                .expect("fixture identity JSON"),
        )
        .expect("fixture identity");
    }

    const VALID_SAMPLES: &str = r#"{"iters":[1.0,2.0],"times":[12.0,24.0]}"#;

    fn fresh_measurement_is_published() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("custom-target/criterion");
        let status = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            assert!(!home.join("scene/fixture/sample.json").exists());
            artifacts(home, "scene", VALID_SAMPLES);
            Ok(true)
        })
        .expect("collect fresh samples");
        assert_eq!(status, ExitCode::SUCCESS);
        assert_eq!(
            std::fs::read_to_string(output.join("scene/fixture/sample.json"))
                .expect("published sample"),
            VALID_SAMPLES
        );
    }

    fn success_without_samples_is_unavailable() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let status = collect_baseline(&args(false), &[target("gpu")], &output, |_, _| Ok(true))
            .expect("unavailable target");
        assert_eq!(
            status,
            ExitCode::FAILURE,
            "process success cannot certify measurements"
        );
        assert!(!output.exists());
    }

    fn stale_baseline_cannot_stand_in_for_a_run() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        artifacts(&output, "scene", VALID_SAMPLES);
        let mut called = false;
        let result = collect_baseline(&args(false), &[target("gpu")], &output, |_, _| {
            called = true;
            Ok(true)
        });
        assert!(
            result.is_err(),
            "old samples must not be accepted as current"
        );
        assert!(!called, "reject reused names before starting any benchmark");
        assert_eq!(
            std::fs::read_to_string(output.join("scene/fixture/sample.json"))
                .expect("old sample preserved"),
            VALID_SAMPLES
        );
    }

    fn empty_destination_left_by_a_failed_run_is_reused() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        std::fs::create_dir_all(output.join("scene/fixture")).expect("empty destination");
        let status = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            artifacts(home, "scene", VALID_SAMPLES);
            Ok(true)
        })
        .expect("an empty destination holds no measurement");
        assert_eq!(status, ExitCode::SUCCESS);
        assert_eq!(
            std::fs::read_to_string(output.join("scene/fixture/sample.json"))
                .expect("published sample"),
            VALID_SAMPLES
        );
    }

    fn criterion_reserved_names_are_refused() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        for reserved in ["new", "change"] {
            let collect = BenchCollectArgs {
                baseline: reserved.to_owned(),
                with_features: false,
            };
            let result = collect_baseline(&collect, &[target("cpu")], &output, |_, _| {
                panic!("a reserved name must be refused before running {reserved}")
            });
            assert!(result.is_err(), "{reserved} is Criterion's own state");
        }
    }

    fn optional_unavailable_target_does_not_erase_measured_work() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let status = collect_baseline(
            &args(false),
            &[target("cpu"), target("gpu")],
            &output,
            |(_, name, _), home| {
                if name == "cpu" {
                    artifacts(home, "scene", VALID_SAMPLES);
                }
                Ok(true)
            },
        )
        .expect("partial collection");
        assert_eq!(status, ExitCode::SUCCESS);
        assert!(output.join("scene/fixture/sample.json").exists());
    }

    fn explicit_complete_run_rejects_an_unavailable_target() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let status = collect_baseline(
            &args(true),
            &[target("cpu"), target("gpu")],
            &output,
            |(_, name, _), home| {
                if name == "cpu" {
                    artifacts(home, "scene", VALID_SAMPLES);
                }
                Ok(true)
            },
        )
        .expect("complete collection verdict");
        assert_eq!(status, ExitCode::FAILURE);
        assert!(
            output.join("scene/fixture/sample.json").exists(),
            "retain real measurements on failure"
        );
    }

    fn invalid_samples_do_not_publish(invalid: &str) {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let result = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            artifacts(home, "valid", VALID_SAMPLES);
            artifacts(home, "invalid", invalid);
            Ok(true)
        });
        assert!(result.is_err(), "invalid fixture admitted: {invalid}");
        assert!(
            !output.exists(),
            "validate all workloads before publishing a target"
        );
    }

    fn empty_samples_do_not_publish() {
        invalid_samples_do_not_publish(r#"{"iters":[],"times":[]}"#);
    }

    fn mismatched_samples_do_not_publish() {
        invalid_samples_do_not_publish(r#"{"iters":[1.0],"times":[12.0,24.0]}"#);
    }

    fn zero_iterations_do_not_publish() {
        invalid_samples_do_not_publish(r#"{"iters":[0.0],"times":[12.0]}"#);
    }

    fn negative_time_does_not_publish() {
        invalid_samples_do_not_publish(r#"{"iters":[1.0],"times":[-12.0]}"#);
    }

    fn non_finite_time_does_not_publish() {
        invalid_samples_do_not_publish(r#"{"iters":[1.0],"times":[1e999]}"#);
    }

    fn incomplete_artifacts_are_rejected() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let result = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            artifacts(home, "scene", VALID_SAMPLES);
            std::fs::remove_file(home.join("scene/fixture/estimates.json"))
                .expect("partial-write fixture");
            Ok(true)
        });
        assert!(result.is_err());
        assert!(!output.exists());
    }

    fn failed_process_cannot_publish_samples() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        let status = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            artifacts(home, "scene", VALID_SAMPLES);
            Ok(false)
        })
        .expect("process failure verdict");
        assert_eq!(status, ExitCode::FAILURE);
        assert!(!output.exists());
    }

    fn publication_failure_is_not_reported_as_saved() {
        let scratch = ScratchDir::new("bench-fixture").expect("fixture");
        let output = scratch.path().join("criterion");
        std::fs::create_dir_all(&output).expect("output directory");
        std::fs::write(output.join("scene"), "blocks destination directory")
            .expect("publication failure fixture");
        let result = collect_baseline(&args(false), &[target("cpu")], &output, |_, home| {
            artifacts(home, "scene", VALID_SAMPLES);
            Ok(true)
        });
        assert!(result.is_err());
    }

    #[test]
    fn baseline_collection_requires_fresh_measurements() {
        crate::table_test::run_table(
            "benchmark evidence",
            &[
                (
                    "fresh_measurement_is_published",
                    fresh_measurement_is_published,
                ),
                (
                    "success_without_samples_is_unavailable",
                    success_without_samples_is_unavailable,
                ),
                (
                    "stale_baseline_cannot_stand_in_for_a_run",
                    stale_baseline_cannot_stand_in_for_a_run,
                ),
                (
                    "empty_destination_left_by_a_failed_run_is_reused",
                    empty_destination_left_by_a_failed_run_is_reused,
                ),
                (
                    "criterion_reserved_names_are_refused",
                    criterion_reserved_names_are_refused,
                ),
                (
                    "optional_unavailable_target_does_not_erase_measured_work",
                    optional_unavailable_target_does_not_erase_measured_work,
                ),
                (
                    "explicit_complete_run_rejects_an_unavailable_target",
                    explicit_complete_run_rejects_an_unavailable_target,
                ),
                ("empty_samples_do_not_publish", empty_samples_do_not_publish),
                (
                    "mismatched_samples_do_not_publish",
                    mismatched_samples_do_not_publish,
                ),
                (
                    "zero_iterations_do_not_publish",
                    zero_iterations_do_not_publish,
                ),
                (
                    "negative_time_does_not_publish",
                    negative_time_does_not_publish,
                ),
                (
                    "non_finite_time_does_not_publish",
                    non_finite_time_does_not_publish,
                ),
                (
                    "incomplete_artifacts_are_rejected",
                    incomplete_artifacts_are_rejected,
                ),
                (
                    "failed_process_cannot_publish_samples",
                    failed_process_cannot_publish_samples,
                ),
                (
                    "publication_failure_is_not_reported_as_saved",
                    publication_failure_is_not_reported_as_saved,
                ),
            ],
        );
    }

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
        let plain = (
            "flui-layer".to_owned(),
            "damage_diff".to_owned(),
            Vec::new(),
        );
        assert!(!bench_args(&plain, "base").contains(&"--features".to_owned()));
    }
}
