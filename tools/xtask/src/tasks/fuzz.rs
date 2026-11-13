//! `cargo xtask fuzz`: the fuzz targets under libFuzzer.
//!
//! A fuzz target is an example of the crate it tests (`crates/<crate>/fuzz/<name>.rs`)
//! that links libFuzzer when it is built under `--cfg fuzzing`. An example
//! shares its crate's lockfile and dev-dependencies, so a run parses with the
//! versions an application ships and needs no package of its own. The flags are
//! the ones `cargo fuzz` passes; this task is their one place, so the nightly
//! workflow and a developer who replays its finding build the same binary.
//!
//! `--cfg fuzzing` reaches every crate of the build, and the decoders read it:
//! `png` and `fdeflate` skip their checksums under it, so that a mutated image
//! gets past the CRC to the code behind it. A finding is therefore replayed in
//! the same build (`--replay`), not in a test binary, which would refuse the
//! input at its checksum; the test case its fix adds restates the checksums.
//!
//! libFuzzer keeps the inputs that reached new code in
//! `<target dir>/fuzz/<name>/corpus/`, which the nightly workflow carries from
//! one run to the next, and writes the input that ended a run to
//! `<target dir>/fuzz/<name>/findings/`, named by what it did (`crash-<hash>`,
//! `timeout-<hash>`, `oom-<hash>`, `leak-<hash>`).

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
    /// A cap on a single allocation, in MiB, for a target whose entry
    /// promises none of its own: libFuzzer reports a block of this size or
    /// larger as a finding. `None` keeps libFuzzer's default, its RSS limit:
    /// a target whose entry promises a bound holds it in its property, from
    /// the entry's own constant.
    malloc_limit_mb: Option<u32>,
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
        // The property bounds every block a decode asks for by
        // `MAX_DECODED_IMAGE_BYTES`, which a decode may reach exactly.
        malloc_limit_mb: None,
    },
    Target {
        name: "agent_wire",
        package: "flui-runtime",
        seeds: &["crates/flui-runtime/fuzz/seeds/agent_wire"],
        max_len: 2048,
        // A conversation of 2 KiB asks a session for nothing near this.
        malloc_limit_mb: Some(64),
    },
];

/// The one host the sanitizer runtime and this task's flags are written for.
const FUZZ_TARGET: &str = "x86_64-unknown-linux-gnu";

/// What `rustc` adds to a fuzz build: the `fuzzing` name the targets, their
/// manifests and some decoders test, the edge counters and comparison hooks
/// libFuzzer steers by (`-sanitizer-coverage-*`, as `cargo fuzz` sets them),
/// and AddressSanitizer, so a read or write out of bounds in `unsafe` code
/// ends the run where it happens. With `--target` named they reach the
/// target's crates only, never a build script or a procedural macro.
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
    /// Seconds each target runs, at least one: libFuzzer reads zero as no
    /// limit.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..))]
    seconds: u32,
    /// Run the one named target on this file alone and stop: the replay of
    /// a finding in the build that found it.
    #[arg(long, value_name = "FILE", conflicts_with = "seconds")]
    replay: Option<PathBuf>,
    #[command(flatten)]
    run: RunOpts,
}

/// `cargo xtask fuzz`: runs each named target for `--seconds`, every one of
/// them even after one fails, and fails when an input panics, aborts, trips
/// the sanitizer, hangs, leaks or allocates past its target's limit. With
/// `--replay` it runs one target on one input.
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
        // libFuzzer reads a directory as a corpus and fuzzes from it without end.
        if !input.is_file() {
            bail!("`--replay` takes a file: {} is not one", input.display());
        }
        // Every command runs from the repository root, wherever this one was typed.
        let input =
            std::path::absolute(input).with_context(|| format!("resolving {}", input.display()))?;
        runner.steps(&[replay_step(target, &input)])?;
        return Ok(ExitCode::SUCCESS);
    }
    let work = target_dir()?.join("fuzz");
    let mut steps = Vec::with_capacity(targets.len());
    for target in targets {
        let dir = work.join(target.name);
        if !runner.dry_run {
            for made in [dir.join("corpus"), dir.join("findings")] {
                std::fs::create_dir_all(&made)
                    .with_context(|| format!("creating {}", made.display()))?;
            }
        }
        steps.push(run_step(target, &dir, args.seconds));
    }
    runner.every(&steps)?;
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
        // Nightly adds lints ahead of stable; this run is for findings.
        .env("CARGO_BUILD_WARNINGS", "warn")
}

/// The limits an input runs under, the same in a run and in its replay.
fn limits(target: Target) -> Vec<String> {
    let mut limits = vec![format!("-timeout={UNIT_TIMEOUT_SECS}")];
    limits.extend(
        target
            .malloc_limit_mb
            .map(|mb| format!("-malloc_limit_mb={mb}")),
    );
    limits
}

/// One target's run: libFuzzer over the corpus in `dir` and the target's
/// seeds for `seconds`.
fn run_step(target: Target, dir: &Path, seconds: u32) -> Step {
    let dir = dir.display();
    started(target)
        .args([format!("{dir}/corpus")])
        .args(target.seeds.iter().copied())
        .args([
            format!("-artifact_prefix={dir}/findings/"),
            format!("-max_total_time={seconds}"),
            format!("-max_len={}", target.max_len),
        ])
        .args(limits(target))
        .args(["-print_final_stats=1"])
        .into()
}

/// One target on `input` alone: libFuzzer runs a file it is given once.
fn replay_step(target: Target, input: &Path) -> Step {
    started(target)
        .args([input.display().to_string()])
        .args(limits(target))
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
                 -artifact_prefix=target/fuzz/image_decode/findings/ -max_total_time=600 \
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
                "-p flui-runtime --example fuzz_agent_wire -- /tmp/crash-0a1b -timeout=20 \
                 -malloc_limit_mb=64"
            ),
            "{line}"
        );
    }

    /// `--seconds 0` would be `-max_total_time=0`, which libFuzzer reads as no limit.
    fn a_run_of_no_seconds_is_refused() {
        let parsed = |argv: &[&str]| super::super::exec::parsed::<FuzzArgs>(argv.iter().copied());
        let refused = parsed(&["--seconds", "0"]).expect_err("zero seconds are refused");
        assert!(refused.to_string().contains("0 is not in 1.."), "{refused}");
        let taken = parsed(&["agent_wire", "--seconds", "1"]).expect("one second is a run");
        assert_eq!((taken.seconds, taken.targets.len()), (1, 1));
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
                    "a_run_of_no_seconds_is_refused",
                    a_run_of_no_seconds_is_refused as fn(),
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
