//! `cargo xtask fuzz`: the fuzz targets under libFuzzer.
//!
//! A fuzz target is an example of the crate it tests (`crates/<crate>/fuzz/<name>.rs`)
//! that links libFuzzer when it is built under `--cfg fuzzing`. An example
//! shares its crate's lockfile and dev-dependencies, so a run decodes with the
//! versions an application ships and needs no package of its own. The flags are
//! the ones `cargo fuzz` passes; this task is their one place, so the nightly
//! workflow and a developer who replays its crash build the same binary.
//!
//! libFuzzer keeps the inputs that reached new code in
//! `<target dir>/fuzz/<name>/corpus/`, which the nightly workflow carries from
//! one run to the next, and writes the input that ended a run beside it as
//! `crash-<hash>`, `timeout-<hash>` or `oom-<hash>`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use super::RunOpts;
use super::exec::{Cmd, Host, Step, target_dir};

/// A fuzz target and what its run starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    /// The name `cargo xtask fuzz` takes; the example is `fuzz_<name>`.
    name: &'static str,
    /// The package whose example it is.
    package: &'static str,
    /// Directories of inputs the run starts from, read and never written.
    seeds: &'static [&'static str],
    /// The longest input libFuzzer writes, in bytes.
    max_len: u32,
}

/// Every fuzz target. Each parses bytes nobody vouches for: an encoded image
/// an application loads from anywhere, and the lines a client writes to the
/// development agent's socket.
const TARGETS: [Target; 2] = [
    Target {
        name: "image_decode",
        package: "flui-painting",
        seeds: &["crates/flui-painting/tests/fixtures"],
        max_len: 4096,
    },
    Target {
        name: "agent_wire",
        package: "flui-runtime",
        seeds: &["crates/flui-runtime/fuzz/seeds/agent_wire"],
        max_len: 2048,
    },
];

/// The one host the sanitizer runtime and this task's flags are written for.
const FUZZ_TARGET: &str = "x86_64-unknown-linux-gnu";

/// What `rustc` adds to a fuzz build: the `fuzzing` name the targets and
/// their manifests test, the edge counters and comparison hooks libFuzzer
/// steers by (`-sanitizer-coverage-*`, as `cargo fuzz` sets them), and
/// AddressSanitizer, so a read or write out of bounds in `unsafe` code ends
/// the run where it happens. With `--target` named they reach the target's
/// crates only, never a build script or a procedural macro.
const RUSTFLAGS: &str = "--cfg fuzzing -Cpasses=sancov-module \
    -Cllvm-args=-sanitizer-coverage-level=4 \
    -Cllvm-args=-sanitizer-coverage-inline-8bit-counters \
    -Cllvm-args=-sanitizer-coverage-pc-table \
    -Cllvm-args=-sanitizer-coverage-trace-compares \
    -Zsanitizer=address";

/// Seconds one input may take before libFuzzer reports it as a hang.
const UNIT_TIMEOUT_SECS: u32 = 20;

/// Arguments for `cargo xtask fuzz`.
#[derive(Debug, clap::Args)]
pub(crate) struct FuzzArgs {
    /// The targets to run (`image_decode`, `agent_wire`); every target when
    /// none is named.
    targets: Vec<String>,
    /// Seconds each target runs.
    #[arg(long, default_value_t = 60)]
    seconds: u32,
    /// Run the one named target on this input alone and stop: the replay of
    /// a `crash-`, `timeout-` or `oom-` file under the sanitizer.
    #[arg(long, value_name = "FILE", conflicts_with = "seconds")]
    replay: Option<PathBuf>,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask fuzz`: runs each named target for `--seconds`, and fails on
/// the first input that panics, aborts, trips the sanitizer, hangs or runs
/// out of memory. With `--replay` it runs one target on one input.
///
/// Needs the nightly toolchain, on x86_64 Linux.
pub(crate) fn fuzz(args: &FuzzArgs) -> anyhow::Result<ExitCode> {
    let targets = selected(&args.targets)?;
    if Host::current() != Host::Linux || std::env::consts::ARCH != "x86_64" {
        bail!("`cargo xtask fuzz` runs on x86_64 Linux: its sanitizer flags name {FUZZ_TARGET}");
    }
    let runner = args.run.runner();
    if let Some(input) = &args.replay {
        let [target] = targets[..] else {
            bail!("`--replay` takes the one target the input belongs to");
        };
        // Every command runs from the repository root, wherever this one was typed.
        let input =
            std::path::absolute(input).with_context(|| format!("resolving {}", input.display()))?;
        runner.steps(&[replay_step(target, &input)])?;
        return Ok(ExitCode::SUCCESS);
    }
    let work = target_dir()?.join("fuzz");
    for target in targets {
        let dir = work.join(target.name);
        if !runner.dry_run {
            let corpus = dir.join("corpus");
            std::fs::create_dir_all(&corpus)
                .with_context(|| format!("creating {}", corpus.display()))?;
        }
        runner.steps(&[run_step(target, &dir, args.seconds)])?;
    }
    Ok(ExitCode::SUCCESS)
}

/// The targets `names` asks for, in the order of [`TARGETS`]; all of them
/// for no name.
fn selected(names: &[String]) -> anyhow::Result<Vec<Target>> {
    if let Some(unknown) = names
        .iter()
        .find(|name| TARGETS.iter().all(|target| target.name != name.as_str()))
    {
        let known: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
        bail!(
            "no fuzz target `{unknown}`; the targets are {}",
            known.join(", ")
        );
    }
    Ok(TARGETS
        .into_iter()
        .filter(|target| names.is_empty() || names.iter().any(|name| name == target.name))
        .collect())
}

/// `target` built under [`RUSTFLAGS`] and started, up to the `--` its own
/// arguments follow.
fn started(target: Target) -> Cmd {
    Cmd::cargo(["+nightly", "run", "--locked", "--target", FUZZ_TARGET])
        .args(["-p", target.package, "--example"])
        .args([format!("fuzz_{}", target.name)])
        .args(["--"])
        .env("RUSTFLAGS", RUSTFLAGS)
        // Nightly adds lints ahead of stable; this run is for crashes.
        .env("CARGO_BUILD_WARNINGS", "warn")
}

/// One target's run: libFuzzer over the corpus in `dir` and the target's
/// seeds for `seconds`.
fn run_step(target: Target, dir: &Path, seconds: u32) -> Step {
    let dir = dir.display();
    started(target)
        .args([format!("{dir}/corpus")])
        .args(target.seeds.iter().copied())
        .args([
            format!("-artifact_prefix={dir}/"),
            format!("-max_total_time={seconds}"),
            format!("-max_len={}", target.max_len),
            format!("-timeout={UNIT_TIMEOUT_SECS}"),
            "-print_final_stats=1".to_owned(),
        ])
        .into()
}

/// One target on `input` alone: libFuzzer runs a file it is given once.
fn replay_step(target: Target, input: &Path) -> Step {
    started(target)
        .args([
            input.display().to_string(),
            format!("-timeout={UNIT_TIMEOUT_SECS}"),
        ])
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_run_builds_the_example_under_the_fuzzing_flags() {
        let step = run_step(TARGETS[0], Path::new("target/fuzz/image_decode"), 600);
        let line = step.to_string();
        assert!(
            line.starts_with("$ RUSTFLAGS='--cfg fuzzing -Cpasses=sancov-module "),
            "{line}"
        );
        assert!(
            line.ends_with(
                "cargo +nightly run --locked --target x86_64-unknown-linux-gnu \
                 -p flui-painting --example fuzz_image_decode -- \
                 target/fuzz/image_decode/corpus crates/flui-painting/tests/fixtures \
                 -artifact_prefix=target/fuzz/image_decode/ -max_total_time=600 \
                 -max_len=4096 -timeout=20 -print_final_stats=1"
            ),
            "{line}"
        );
    }

    fn a_replay_runs_the_target_on_the_one_input() {
        let step = replay_step(TARGETS[1], Path::new("/tmp/crash-0a1b"));
        let line = step.to_string();
        assert!(
            line.ends_with(
                "-p flui-runtime --example fuzz_agent_wire -- /tmp/crash-0a1b -timeout=20"
            ),
            "{line}"
        );
    }

    fn targets_are_selected_by_name_and_an_unknown_one_is_refused() {
        let names = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        };
        let all = selected(&[]).expect("no name selects every target");
        assert_eq!(all, TARGETS);
        let one = selected(&names(&["agent_wire"])).expect("a target's name selects it");
        assert_eq!(one, [TARGETS[1]]);
        let refused =
            selected(&names(&["image_decode", "nothing"])).expect_err("an unknown name is refused");
        assert!(
            refused
                .to_string()
                .contains("no fuzz target `nothing`; the targets are image_decode, agent_wire"),
            "{refused}"
        );
    }

    /// Every target is an example its package declares, with seeds that exist:
    /// a target renamed in one place fails here, not in the nightly run.
    fn every_target_is_a_declared_example_with_seeds() {
        let root = crate::util::repo_root();
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(&root)
            .no_deps()
            .exec()
            .expect("cargo metadata");
        for target in TARGETS {
            let package = metadata
                .packages
                .iter()
                .find(|package| package.name.as_str() == target.package)
                .unwrap_or_else(|| panic!("no package {}", target.package));
            let example = format!("fuzz_{}", target.name);
            assert!(
                package
                    .targets
                    .iter()
                    .any(|built| built.is_example() && built.name == example),
                "{} declares no example {example}",
                target.package
            );
            for seeds in target.seeds {
                let entries = std::fs::read_dir(root.join(seeds))
                    .unwrap_or_else(|error| panic!("{seeds}: {error}"));
                assert!(entries.count() > 0, "{seeds} holds no seed");
            }
        }
    }

    #[test]
    fn fuzz_plan_contract() {
        crate::table_test::run_table(
            "fuzz_plan_contract",
            &[
                (
                    "a_run_builds_the_example_under_the_fuzzing_flags",
                    a_run_builds_the_example_under_the_fuzzing_flags as fn(),
                ),
                (
                    "a_replay_runs_the_target_on_the_one_input",
                    a_replay_runs_the_target_on_the_one_input as fn(),
                ),
                (
                    "targets_are_selected_by_name_and_an_unknown_one_is_refused",
                    targets_are_selected_by_name_and_an_unknown_one_is_refused as fn(),
                ),
                (
                    "every_target_is_a_declared_example_with_seeds",
                    every_target_is_a_declared_example_with_seeds as fn(),
                ),
            ],
        );
    }
}
