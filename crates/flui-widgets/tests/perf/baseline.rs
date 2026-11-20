//! The counts each perf scenario is held to: one file per scenario in
//! `crates/flui-widgets/perf/baseline/`, read and rewritten by the scenarios
//! themselves when `FLUI_PERF_BASELINE` asks for it.
//!
//! - Unset, a scenario holds only its own budget assertions, which hold on
//!   every host and with any feature set.
//! - `check`: a scenario panics, before it asserts its budget, when its counts
//!   differ from its file. CI's `perf-gates` job runs it, on Linux and with
//!   flui-widgets' default features, which is where the files were blessed: the
//!   heap counters move with the allocator's platform and with the features
//!   that change what a frame builds.
//! - `bless`: a scenario rewrites its file from this run.
//!
//! The comparison depends on the kind of counter:
//!
//! - a **work** counter (elements rebuilt, nodes laid out, layers produced, …)
//!   describes what the frame did, so any difference fails: a rise is a
//!   regression, and a fall is work that went missing until a bless says it was
//!   meant;
//! - a **cost** counter (a name with `alloc` or `bytes` in it) only has to stay
//!   at or under its file. A fall prints and passes, so a pull request that
//!   makes a scenario cheaper needs no baseline edit; a bless on a fresh `main`
//!   lowers the file, so a later rise back to the old value fails.
//!
//! A counter on one side only fails either way.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::PathBuf;

/// The environment variable that turns the comparison on.
const MODE_VARIABLE: &str = "FLUI_PERF_BASELINE";

/// The command that rewrites every file.
const BLESS_COMMAND: &str = "FLUI_PERF_BASELINE=bless cargo nextest run --locked -p flui-widgets --test perf -E 'test(/^perf_/)'";

/// Every scenario that holds a file, so a file whose scenario was renamed or
/// removed fails `every_baseline_file_is_a_scenario` instead of lingering.
/// Kept sorted, which that test checks.
pub(crate) const SCENARIOS: [&str; 12] = [
    "caret_blink_in_10k_list",
    "full_reassemble",
    "idle_frame_retained_scene",
    "label_change_large",
    "label_change_small",
    "list_10k_scroll_one_screen",
    "list_10k_scroll_warm",
    "not_shown_retry",
    "raw_button_over_text",
    "semantics_action_over_text",
    "slide_transition_tick",
    "texture_frame",
];

/// Counter name → value, sorted as the file writes them.
type Counts = BTreeMap<String, u64>;

/// What a run does with a scenario's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Fail on any difference that fails.
    Check,
    /// Rewrite the file from this run.
    Bless,
}

impl Mode {
    /// The mode `FLUI_PERF_BASELINE` names, or `None` when it is unset.
    fn from_env() -> Option<Self> {
        let value = std::env::var(MODE_VARIABLE).ok()?;
        match value.as_str() {
            "check" => Some(Self::Check),
            "bless" => Some(Self::Bless),
            other => panic!("{MODE_VARIABLE}={other:?} names no mode; use `check` or `bless`"),
        }
    }
}

/// Holds `scenario`'s `counts` to its file, or rewrites the file, as
/// `FLUI_PERF_BASELINE` says; does nothing when it is unset. Called before a
/// scenario asserts its budget, so a check that fails reports every counter.
pub(crate) fn hold(scenario: &str, counts: &[(&str, u64)]) {
    assert!(
        SCENARIOS.contains(&scenario),
        "scenario {scenario} holds a baseline file but is not in baseline::SCENARIOS"
    );
    let Some(mode) = Mode::from_env() else {
        return;
    };
    let current: Counts = counts
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect();
    assert_eq!(
        current.len(),
        counts.len(),
        "{scenario} reports a counter twice: {counts:?}"
    );
    let path = file(scenario);
    match mode {
        Mode::Bless => {
            std::fs::write(&path, render(scenario, &current))
                .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
        }
        Mode::Check => {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "reading {}: {error}; a new scenario's file comes from `{BLESS_COMMAND}`",
                    path.display()
                )
            });
            let baseline = parse(scenario, &text)
                .unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()));
            let findings = compare(&baseline, &current);
            let mut report = String::new();
            for finding in &findings {
                writeln!(report, "{scenario}.{finding}").expect("BUG: writing to a String");
            }
            print!("{report}");
            assert!(
                findings.iter().all(|finding| !finding.fails()),
                "{scenario} differs from {}:\n{report}a change that was meant is blessed with `{BLESS_COMMAND}`",
                path.display()
            );
        }
    }
}

/// The directory that holds one `<scenario>.toml` per scenario.
pub(crate) fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("perf")
        .join("baseline")
}

fn file(scenario: &str) -> PathBuf {
    directory().join(format!("{scenario}.toml"))
}

/// The file's text: a header, then the scenario's table with its counters sorted.
fn render(scenario: &str, counts: &Counts) -> String {
    let mut text = format!(
        "# Written by `{BLESS_COMMAND}`;\n\
         # counts, not timings. One scenario of crates/flui-widgets/tests/perf.rs.\n\n[{scenario}]\n"
    );
    for (name, value) in counts {
        writeln!(text, "{name} = {value}").expect("BUG: writing to a String");
    }
    text
}

/// The counters of a file that holds exactly the table `[scenario]` of
/// `name = value` lines, after any comments and blank lines.
fn parse(scenario: &str, text: &str) -> Result<Counts, String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let header = format!("[{scenario}]");
    match lines.next() {
        Some(line) if line == header => {}
        other => return Err(format!("expected the table {header}, found {other:?}")),
    }
    let mut counts = Counts::new();
    for line in lines {
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("`{line}` is not `name = value`"))?;
        let name = name.trim();
        let value = value
            .trim()
            .parse::<u64>()
            .map_err(|error| format!("`{line}`: {error}"))?;
        if counts.insert(name.to_owned(), value).is_some() {
            return Err(format!("`{name}` is set twice"));
        }
    }
    Ok(counts)
}

/// Whether `counter` measures cost (heap traffic), which only has to stay at
/// or under its file, rather than work, which has to match it.
fn is_cost(counter: &str) -> bool {
    counter.contains("alloc") || counter.contains("bytes")
}

/// One difference between a scenario's file and a run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    /// A counter rose.
    Rose {
        counter: String,
        base: u64,
        current: u64,
    },
    /// A work counter fell: the frame does less than the file says.
    Fell {
        counter: String,
        base: u64,
        current: u64,
    },
    /// A cost counter fell. It passes; a bless on `main` lowers the file.
    Cheaper {
        counter: String,
        base: u64,
        current: u64,
    },
    /// A counter of the file the run did not report.
    Missing { counter: String },
    /// A counter the run reported that the file does not have.
    New { counter: String },
}

impl Finding {
    /// Whether the check fails on it: everything but a cost that fell.
    fn fails(&self) -> bool {
        !matches!(self, Self::Cheaper { .. })
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rose {
                counter,
                base,
                current,
            } => {
                write!(f, "{counter}: rose {base} -> {current}")
            }
            Self::Fell {
                counter,
                base,
                current,
            } => {
                write!(
                    f,
                    "{counter}: fell {base} -> {current} (work went missing, or bless it)"
                )
            }
            Self::Cheaper {
                counter,
                base,
                current,
            } => write!(
                f,
                "{counter}: fell {base} -> {current} (passes; a bless on main lowers the file)"
            ),
            Self::Missing { counter } => write!(f, "{counter}: in the file, not in this run"),
            Self::New { counter } => write!(f, "{counter}: not in the file"),
        }
    }
}

/// Every difference between `baseline` and `current`, in counter order.
fn compare(baseline: &Counts, current: &Counts) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (counter, &base) in baseline {
        let Some(&value) = current.get(counter) else {
            findings.push(Finding::Missing {
                counter: counter.clone(),
            });
            continue;
        };
        let counter = counter.clone();
        if value > base {
            findings.push(Finding::Rose {
                counter,
                base,
                current: value,
            });
        } else if value < base && is_cost(&counter) {
            findings.push(Finding::Cheaper {
                counter,
                base,
                current: value,
            });
        } else if value < base {
            findings.push(Finding::Fell {
                counter,
                base,
                current: value,
            });
        }
    }
    for counter in current.keys() {
        if !baseline.contains_key(counter) {
            findings.push(Finding::New {
                counter: counter.clone(),
            });
        }
    }
    findings
}

fn counts(pairs: &[(&str, u64)]) -> Counts {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect()
}

/// A rise of either kind fails, a fall fails for work and passes for cost, and
/// a counter on one side only fails.
#[test]
fn the_comparison_passes_only_an_equal_counter_or_a_cost_that_fell() {
    let base = counts(&[("allocations", 12), ("nodes_laid_out", 40)]);
    let rose = |counter: &str, base, current| Finding::Rose {
        counter: counter.to_owned(),
        base,
        current,
    };
    for (case, current, expected) in [
        (
            "equal",
            counts(&[("allocations", 12), ("nodes_laid_out", 40)]),
            vec![],
        ),
        (
            "work rose",
            counts(&[("allocations", 12), ("nodes_laid_out", 41)]),
            vec![rose("nodes_laid_out", 40, 41)],
        ),
        (
            "cost rose",
            counts(&[("allocations", 13), ("nodes_laid_out", 40)]),
            vec![rose("allocations", 12, 13)],
        ),
        (
            "work fell",
            counts(&[("allocations", 12), ("nodes_laid_out", 39)]),
            vec![Finding::Fell {
                counter: "nodes_laid_out".to_owned(),
                base: 40,
                current: 39,
            }],
        ),
        (
            "cost fell",
            counts(&[("allocations", 11), ("nodes_laid_out", 40)]),
            vec![Finding::Cheaper {
                counter: "allocations".to_owned(),
                base: 12,
                current: 11,
            }],
        ),
        (
            "a counter on one side only",
            counts(&[("nodes_laid_out", 40), ("layers_reused", 0)]),
            vec![
                Finding::Missing {
                    counter: "allocations".to_owned(),
                },
                Finding::New {
                    counter: "layers_reused".to_owned(),
                },
            ],
        ),
    ] {
        let found = compare(&base, &current);
        assert_eq!(found, expected, "{case}");
        let passes = case == "equal" || case == "cost fell";
        assert_eq!(
            found.iter().all(|finding| !finding.fails()),
            passes,
            "{case}"
        );
    }
    assert!(is_cost("allocated_bytes") && is_cost("reallocations") && !is_cost("nodes_painted"));
}

/// A blessed file parses back to the counts it was written from, and a file
/// that holds another scenario's table, a line that is not a count or a
/// counter set twice does not parse.
#[test]
fn a_blessed_file_parses_back_and_a_malformed_one_does_not() {
    let written = counts(&[("allocations", 196), ("reallocations", 4)]);
    assert_eq!(parse("scroll", &render("scroll", &written)), Ok(written));
    for (case, text) in [
        ("another scenario's table", "[idle]\nframes_produced = 0\n"),
        ("no table", "frames_produced = 0\n"),
        ("not a count", "[scroll]\nframes_produced = many\n"),
        ("set twice", "[scroll]\nframes = 1\nframes = 2\n"),
    ] {
        assert!(parse("scroll", text).is_err(), "{case}");
    }
}

/// The directory holds a file for each scenario of [`SCENARIOS`] and no other,
/// and each one parses.
#[test]
fn every_baseline_file_is_a_scenario() {
    let mut sorted = SCENARIOS;
    sorted.sort_unstable();
    assert_eq!(sorted, SCENARIOS, "baseline::SCENARIOS is kept sorted");
    let mut files: Vec<String> = std::fs::read_dir(directory())
        .expect("BUG: the baseline directory is checked in")
        .map(|entry| {
            let path = entry.expect("BUG: a readable directory entry").path();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            let scenario = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("BUG: a baseline file has a UTF-8 name")
                .to_owned();
            assert_eq!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("toml"),
                "{}",
                path.display()
            );
            if let Err(error) = parse(&scenario, &text) {
                panic!("{}: {error}", path.display());
            }
            scenario
        })
        .collect();
    files.sort_unstable();
    assert_eq!(
        files, SCENARIOS,
        "the baseline files and baseline::SCENARIOS"
    );
}
