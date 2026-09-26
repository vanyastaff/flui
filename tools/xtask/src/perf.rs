//! `cargo xtask perf`: run the counted perf scenarios and compare them with
//! the checked-in baseline.
//!
//! The scenarios are `flui-widgets`' `perf` test target. Each one mounts a
//! small app, applies one change, and records the frame's work — elements
//! rebuilt, nodes laid out and painted, layers produced and reused, semantics
//! nodes published, frames produced — as counts, never timings. Counts are a
//! deterministic function of the tree and the change, so the comparison is
//! exact and a difference is a finding: a count that rose is a regression, and
//! one that fell asks for `--bless`, so an improvement is locked in and a later
//! regression back up to the old value cannot pass unnoticed.
//!
//! Advisory by default (findings print, the command exits 0); `--check` makes
//! any finding fatal. `--self-test` compares planted fixtures in memory and
//! builds nothing, which is why it is the half that runs in `cargo xtask
//! checks`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};

use crate::util::{self, repo_root};

/// The package whose `perf` test target holds the scenarios.
const PACKAGE: &str = "flui-widgets";

/// The baseline, inside the scenarios' crate so a change to it is classified
/// as a change to that crate and runs its lane.
const BASELINE: &str = "crates/flui-widgets/perf/baseline.toml";

/// Written above the tables of a blessed baseline.
const BASELINE_HEADER: &str = "# Written by `cargo xtask perf --bless`; counts, not timings.\n\
# One table per scenario of crates/flui-widgets/tests/perf.rs.\n";

/// Scenario name → counter name → value.
type Counts = BTreeMap<String, BTreeMap<String, u64>>;

/// Arguments for `cargo xtask perf`.
#[derive(Debug, clap::Args)]
pub(crate) struct PerfArgs {
    /// Fail when any counter differs from the baseline.
    #[arg(long, conflicts_with_all = ["bless", "self_test"])]
    check: bool,
    /// Rewrite the baseline from this run.
    #[arg(long, conflicts_with = "self_test")]
    bless: bool,
    /// Compare planted fixtures in memory and fail if the comparison misses
    /// one; builds nothing.
    #[arg(long)]
    self_test: bool,
}

/// How a run treats its findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Print them and exit 0.
    Advisory,
    /// Exit 1 on any.
    Check,
    /// Rewrite the baseline and exit 0.
    Bless,
}

/// One difference between the baseline and a run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// A counter rose.
    Regressed {
        scenario: String,
        counter: String,
        base: u64,
        current: u64,
    },
    /// A counter fell: bless it so the ratchet holds the new value.
    Improved {
        scenario: String,
        counter: String,
        base: u64,
        current: u64,
    },
    /// A baseline scenario the run did not report.
    MissingScenario { scenario: String },
    /// A reported scenario the baseline does not have.
    NewScenario { scenario: String },
    /// A baseline counter the run did not report.
    MissingCounter { scenario: String, counter: String },
    /// A reported counter the baseline does not have.
    NewCounter { scenario: String, counter: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Regressed {
                scenario,
                counter,
                base,
                current,
            } => write!(f, "REGRESSED  {scenario}.{counter}: {base} -> {current}"),
            Self::Improved {
                scenario,
                counter,
                base,
                current,
            } => write!(
                f,
                "IMPROVED   {scenario}.{counter}: {base} -> {current} (run `cargo xtask perf --bless`)"
            ),
            Self::MissingScenario { scenario } => {
                write!(
                    f,
                    "MISSING    scenario {scenario}: in the baseline, not in this run"
                )
            }
            Self::NewScenario { scenario } => {
                write!(f, "NEW        scenario {scenario}: not in the baseline")
            }
            Self::MissingCounter { scenario, counter } => write!(
                f,
                "MISSING    {scenario}.{counter}: in the baseline, not in this run"
            ),
            Self::NewCounter { scenario, counter } => {
                write!(f, "NEW        {scenario}.{counter}: not in the baseline")
            }
        }
    }
}

/// `cargo xtask perf`.
pub(crate) fn perf(args: &PerfArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let mode = if args.check {
        Mode::Check
    } else if args.bless {
        Mode::Bless
    } else {
        Mode::Advisory
    };

    let root = repo_root();
    let out = util::metadata(&root)?
        .target_directory
        .into_std_path_buf()
        .join("perf");
    if out.exists() {
        std::fs::remove_dir_all(&out).with_context(|| format!("clearing {}", out.display()))?;
    }
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;

    run_scenarios(&root, &out)?;
    let current = read_records(&out)?;
    if current.is_empty() {
        bail!(
            "the perf scenarios reported nothing under {}; is FLUI_PERF_OUT read?",
            out.display()
        );
    }
    std::fs::write(out.join("current.toml"), render(&current)?)
        .with_context(|| format!("writing {}", out.join("current.toml").display()))?;

    let baseline_path = root.join(BASELINE);
    if mode == Mode::Bless {
        std::fs::write(&baseline_path, render(&current)?)
            .with_context(|| format!("writing {}", baseline_path.display()))?;
        println!("perf: blessed {BASELINE} ({} scenarios)", current.len());
        return Ok(ExitCode::SUCCESS);
    }

    let baseline = if baseline_path.exists() {
        parse(
            &std::fs::read_to_string(&baseline_path)
                .with_context(|| format!("reading {}", baseline_path.display()))?,
        )
        .with_context(|| format!("parsing {BASELINE}"))?
    } else {
        println!("perf: no baseline at {BASELINE}; run `cargo xtask perf --bless`");
        Counts::new()
    };

    print_table(&baseline, &current);
    let findings = compare(&baseline, &current);
    for finding in &findings {
        println!("{finding}");
    }
    Ok(outcome(mode, findings.len()))
}

/// Runs the scenarios with `FLUI_PERF_OUT=out`. A failing test is always
/// fatal: a budget assertion broke, and the ordinary suite fails on it too.
fn run_scenarios(root: &Path, out: &Path) -> anyhow::Result<()> {
    let mut cargo = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cargo
        .current_dir(root)
        .args([
            "nextest",
            "run",
            "--locked",
            "-p",
            PACKAGE,
            "--test",
            "perf",
            "--no-fail-fast",
            "-E",
            "test(/^perf_/)",
        ])
        .env("FLUI_PERF_OUT", out);
    let status = cargo
        .status()
        .context("running the perf scenarios (is cargo-nextest installed?)")?;
    if !status.success() {
        bail!("the perf scenarios failed ({status}); a budget assertion broke");
    }
    Ok(())
}

/// Reads every `<scenario>.toml` the scenarios wrote.
fn read_records(out: &Path) -> anyhow::Result<Counts> {
    let mut counts = Counts::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(out)
        .with_context(|| format!("listing {}", out.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    files.sort();
    for file in files {
        if file.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        for (scenario, counters) in
            parse(&text).with_context(|| format!("parsing {}", file.display()))?
        {
            if counts.insert(scenario.clone(), counters).is_some() {
                bail!("scenario {scenario} was reported twice");
            }
        }
    }
    Ok(counts)
}

fn parse(text: &str) -> anyhow::Result<Counts> {
    Ok(toml::from_str(text)?)
}

/// The baseline file's text: the header, then one sorted table per scenario.
fn render(counts: &Counts) -> anyhow::Result<String> {
    Ok(format!("{BASELINE_HEADER}\n{}", toml::to_string(counts)?))
}

/// Every difference between `baseline` and `current`, in a stable order.
fn compare(baseline: &Counts, current: &Counts) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (scenario, base_counters) in baseline {
        let Some(current_counters) = current.get(scenario) else {
            findings.push(Finding::MissingScenario {
                scenario: scenario.clone(),
            });
            continue;
        };
        for (counter, &base) in base_counters {
            let Some(&value) = current_counters.get(counter) else {
                findings.push(Finding::MissingCounter {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                });
                continue;
            };
            if value > base {
                findings.push(Finding::Regressed {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                    base,
                    current: value,
                });
            } else if value < base {
                findings.push(Finding::Improved {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                    base,
                    current: value,
                });
            }
        }
        for counter in current_counters.keys() {
            if !base_counters.contains_key(counter) {
                findings.push(Finding::NewCounter {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                });
            }
        }
    }
    for scenario in current.keys() {
        if !baseline.contains_key(scenario) {
            findings.push(Finding::NewScenario {
                scenario: scenario.clone(),
            });
        }
    }
    findings
}

/// The exit code for `findings` findings under `mode`.
fn outcome(mode: Mode, findings: usize) -> ExitCode {
    match mode {
        Mode::Bless => ExitCode::SUCCESS,
        Mode::Check if findings > 0 => {
            println!("perf: {findings} findings against {BASELINE}");
            ExitCode::FAILURE
        }
        Mode::Check => {
            println!("perf: matches {BASELINE}");
            ExitCode::SUCCESS
        }
        Mode::Advisory => {
            println!("advisory: {findings} findings");
            ExitCode::SUCCESS
        }
    }
}

fn print_table(baseline: &Counts, current: &Counts) {
    println!(
        "{:<30} {:<26} {:>10} {:>10}",
        "scenario", "counter", "baseline", "current"
    );
    for (scenario, counters) in current {
        for (counter, value) in counters {
            let base = baseline
                .get(scenario)
                .and_then(|counters| counters.get(counter))
                .map_or_else(|| "-".to_owned(), u64::to_string);
            println!("{scenario:<30} {counter:<26} {base:>10} {value:>10}");
        }
    }
}

/// The planted comparison: a baseline and a run that differ by exactly one
/// regression, one improvement, one missing scenario and one new counter.
fn self_test_fixture() -> (Counts, Counts, Vec<Finding>) {
    let table = |pairs: &[(&str, u64)]| -> BTreeMap<String, u64> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect()
    };
    let baseline: Counts = [
        (
            "scroll".to_owned(),
            table(&[("nodes_laid_out", 40), ("nodes_painted", 30)]),
        ),
        ("idle".to_owned(), table(&[("frames_produced", 0)])),
        ("gone".to_owned(), table(&[("frames_produced", 1)])),
    ]
    .into_iter()
    .collect();
    let current: Counts = [
        (
            "scroll".to_owned(),
            table(&[("nodes_laid_out", 41), ("nodes_painted", 29)]),
        ),
        (
            "idle".to_owned(),
            table(&[("frames_produced", 0), ("layers_reused", 0)]),
        ),
    ]
    .into_iter()
    .collect();
    let expected = vec![
        Finding::Regressed {
            scenario: "scroll".to_owned(),
            counter: "nodes_laid_out".to_owned(),
            base: 40,
            current: 41,
        },
        Finding::Improved {
            scenario: "scroll".to_owned(),
            counter: "nodes_painted".to_owned(),
            base: 30,
            current: 29,
        },
        Finding::MissingScenario {
            scenario: "gone".to_owned(),
        },
        Finding::NewCounter {
            scenario: "idle".to_owned(),
            counter: "layers_reused".to_owned(),
        },
    ];
    (baseline, current, expected)
}

/// `--self-test`: fails unless the comparison reports exactly the planted
/// findings, and the planted run survives a bless-and-parse round trip.
fn self_test() -> ExitCode {
    let (baseline, current, expected) = self_test_fixture();
    let mut found = compare(&baseline, &current);
    let mut expected = expected;
    found.sort();
    expected.sort();
    let round_trip = render(&current)
        .and_then(|text| parse(&text))
        .is_ok_and(|parsed| parsed == current);
    if found == expected && round_trip {
        println!("perf self-test: the comparison reports exactly the 4 planted findings");
        return ExitCode::SUCCESS;
    }
    for finding in expected.iter().filter(|finding| !found.contains(finding)) {
        println!("perf self-test: MISSED {finding}");
    }
    for finding in found.iter().filter(|finding| !expected.contains(finding)) {
        println!("perf self-test: UNEXPECTED {finding}");
    }
    if !round_trip {
        println!("perf self-test: a blessed baseline does not parse back to itself");
    }
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(scenario: &str, counter: &str, value: u64) -> Counts {
        [(
            scenario.to_owned(),
            [(counter.to_owned(), value)].into_iter().collect(),
        )]
        .into_iter()
        .collect()
    }

    #[test]
    fn a_counter_above_its_baseline_is_a_regression() {
        assert_eq!(
            compare(&one("s", "c", 3), &one("s", "c", 4)),
            [Finding::Regressed {
                scenario: "s".to_owned(),
                counter: "c".to_owned(),
                base: 3,
                current: 4,
            }]
        );
    }

    #[test]
    fn a_counter_below_its_baseline_asks_for_a_bless() {
        let findings = compare(&one("s", "c", 3), &one("s", "c", 2));
        assert_eq!(
            findings,
            [Finding::Improved {
                scenario: "s".to_owned(),
                counter: "c".to_owned(),
                base: 3,
                current: 2,
            }]
        );
        assert!(
            findings[0].to_string().contains("--bless"),
            "{}",
            findings[0]
        );
    }

    #[test]
    fn an_equal_run_has_no_findings() {
        assert!(compare(&one("s", "c", 3), &one("s", "c", 3)).is_empty());
    }

    #[test]
    fn a_scenario_missing_from_the_run_is_reported() {
        assert_eq!(
            compare(&one("s", "c", 3), &Counts::new()),
            [Finding::MissingScenario {
                scenario: "s".to_owned()
            }]
        );
        assert_eq!(
            compare(&Counts::new(), &one("s", "c", 3)),
            [Finding::NewScenario {
                scenario: "s".to_owned()
            }]
        );
        assert_eq!(
            compare(&one("s", "c", 3), &one("s", "d", 3)),
            [
                Finding::MissingCounter {
                    scenario: "s".to_owned(),
                    counter: "c".to_owned()
                },
                Finding::NewCounter {
                    scenario: "s".to_owned(),
                    counter: "d".to_owned()
                },
            ]
        );
    }

    #[test]
    fn blessed_output_round_trips_through_the_parser() {
        let (_, current, _) = self_test_fixture();
        let text = render(&current).expect("renders");
        assert!(
            text.starts_with("# Written by `cargo xtask perf --bless`"),
            "{text}"
        );
        assert_eq!(parse(&text).expect("parses"), current);
    }

    #[test]
    fn advisory_mode_exits_zero_with_findings() {
        assert_eq!(outcome(Mode::Advisory, 3), ExitCode::SUCCESS);
        assert_eq!(outcome(Mode::Bless, 3), ExitCode::SUCCESS);
    }

    #[test]
    fn check_mode_exits_nonzero_with_findings() {
        assert_eq!(outcome(Mode::Check, 1), ExitCode::FAILURE);
        assert_eq!(outcome(Mode::Check, 0), ExitCode::SUCCESS);
    }

    #[test]
    fn self_test_fixture_yields_exactly_the_expected_findings() {
        let (baseline, current, mut expected) = self_test_fixture();
        let mut found = compare(&baseline, &current);
        found.sort();
        expected.sort();
        assert_eq!(found, expected);
        assert_eq!(self_test(), ExitCode::SUCCESS);
    }
}
