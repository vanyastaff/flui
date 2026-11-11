//! `cargo xtask perf --instructions`: a frame's instructions on a small and a
//! large tree, counted by callgrind.
//!
//! Each `perf_scaling_` scenario of the perf target runs one change on two
//! tree sizes, `small` and `large`, and selects one of them from
//! `FLUI_PERF_SIZE`. This runs every such scenario once per size under
//! `valgrind --tool=callgrind`, collecting instructions only while the
//! target's `perf_measured` runs, and compares the two counts. The count is a
//! deterministic function of the code and the tree, close enough across runs
//! that a fixed tolerance separates a flat frame from one whose cost follows
//! the tree: a frame that walks or clears something sized by the whole tree
//! costs a multiple of the small count on the large one.
//!
//! Advisory by default; `--check` fails on a scenario whose large count
//! exceeds the small one by more than [`TOLERANCE_PERCENT`].

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, bail};

use super::PACKAGE;

/// How far the large tree's count may exceed the small tree's, in percent.
pub(super) const TOLERANCE_PERCENT: u64 = 5;

/// The prefix that marks a scenario as comparing two tree sizes.
const SCALING_PREFIX: &str = "perf_scaling_";

/// The size names a scaling scenario reads from `FLUI_PERF_SIZE`.
const SIZES: [&str; 2] = ["small", "large"];

/// The function whose run is the measured window.
const MEASURED: &str = "*perf_measured*";

/// One scaling scenario's two counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Scaling {
    pub(super) small: u64,
    pub(super) large: u64,
}

impl Scaling {
    /// Whether the large count stays within the tolerance of the small one.
    /// Widened before multiplying, so no count overflows the comparison.
    pub(super) fn is_flat(self) -> bool {
        u128::from(self.large) * 100 <= u128::from(self.small) * u128::from(100 + TOLERANCE_PERCENT)
    }
}

/// `cargo xtask perf --instructions [--check]`.
pub(super) fn run(root: &Path, out: &Path, check: bool) -> anyhow::Result<ExitCode> {
    which_valgrind()?;
    let binary = build(root)?;
    let scenarios = scaling_scenarios(&binary)?;
    if scenarios.is_empty() {
        bail!("the perf target has no `{SCALING_PREFIX}` scenario");
    }
    let mut failed = 0usize;
    println!(
        "{:<64} {:>12} {:>12} {:>8}",
        "scenario", "small", "large", "ratio"
    );
    for scenario in &scenarios {
        let [small, large] = SIZES.map(|size| collect(&binary, out, scenario, size));
        let scaling = Scaling {
            small: small?,
            large: large?,
        };
        let ratio = scaling.large as f64 / scaling.small.max(1) as f64;
        let verdict = if scaling.is_flat() { "" } else { "  GROWS" };
        println!(
            "{scenario:<64} {:>12} {:>12} {ratio:>8.3}{verdict}",
            scaling.small, scaling.large
        );
        if !scaling.is_flat() {
            failed += 1;
        }
    }
    if failed == 0 {
        println!("perf --instructions: every scaling scenario is flat within {TOLERANCE_PERCENT}%");
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "perf --instructions: {failed} scenario(s) cost more on the large tree than \
         {TOLERANCE_PERCENT}% over the small one"
    );
    Ok(if check {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn which_valgrind() -> anyhow::Result<()> {
    let found = Command::new("valgrind")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if found {
        Ok(())
    } else {
        bail!("`cargo xtask perf --instructions` needs valgrind on PATH")
    }
}

/// Builds the perf target and returns its test executable.
fn build(root: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args([
            "test",
            "--locked",
            "-p",
            PACKAGE,
            "--test",
            "perf",
            "--no-run",
            "--message-format=json",
        ])
        .stderr(Stdio::inherit())
        .output()
        .context("building the perf target")?;
    if !output.status.success() {
        bail!("building the perf target failed ({})", output.status);
    }
    let stdout = String::from_utf8(output.stdout).context("cargo's JSON is UTF-8")?;
    executable_from_messages(&stdout).context("cargo reported no executable for the perf target")
}

/// The `perf` test executable among cargo's `--message-format=json` lines.
pub(super) fn executable_from_messages(messages: &str) -> Option<PathBuf> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == "perf")
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
}

/// The scaling scenarios the target lists.
fn scaling_scenarios(binary: &Path) -> anyhow::Result<Vec<String>> {
    let output = Command::new(binary)
        .args(["--list", "--format", "terse"])
        .output()
        .with_context(|| format!("listing {}", binary.display()))?;
    let listing = String::from_utf8(output.stdout).context("the test listing is UTF-8")?;
    Ok(scaling_from_listing(&listing))
}

/// The `perf_scaling_` names in a libtest `--list --format terse` listing.
pub(super) fn scaling_from_listing(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .filter(|name| name.starts_with(SCALING_PREFIX))
        .map(str::to_owned)
        .collect()
}

/// Runs `scenario` at `size` under callgrind and returns the instructions it
/// collected inside the measured window.
fn collect(binary: &Path, out: &Path, scenario: &str, size: &str) -> anyhow::Result<u64> {
    let profile = out.join(format!("{scenario}.{size}.callgrind"));
    let output = Command::new("valgrind")
        .arg("--tool=callgrind")
        .arg(format!("--callgrind-out-file={}", profile.display()))
        .arg(format!("--toggle-collect={MEASURED}"))
        .arg(binary)
        .args(["--exact", scenario, "--test-threads=1"])
        .env("FLUI_PERF_SIZE", size)
        .output()
        .with_context(|| format!("running {scenario} ({size}) under callgrind"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!(
            "{scenario} ({size}) failed under callgrind ({}):\n{stderr}",
            output.status
        );
    }
    collected_from_log(&stderr)
        .with_context(|| format!("callgrind printed no `Collected` line for {scenario} ({size})"))
}

/// The count of callgrind's `==PID== Collected : N` line.
pub(super) fn collected_from_log(log: &str) -> Option<u64> {
    log.lines()
        .filter_map(|line| line.split_once("Collected :"))
        .find_map(|(_, count)| count.trim().parse::<u64>().ok())
}
