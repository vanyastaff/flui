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
//! - `bless`: a scenario rewrites its file from this run once its test has
//!   passed. [`hold`] returns a [`Held`] the scenario keeps to its end, and a
//!   panic before then (a broken budget) writes nothing.
//!
//! When `FLUI_PERF_OUT` names a directory, a `check` or `bless` run first
//! writes each scenario's counts there as `<scenario>.toml`, in the baseline
//! file's form. CI's
//! `perf-gates` job compares that directory with the baseline afterwards, so a
//! scenario that held nothing (it stopped running, or no longer reaches
//! [`hold`]) fails there, and a cost that fell, which passes here, shows in the
//! job's log.
//!
//! The comparison depends on the kind of counter:
//!
//! - a **work** counter (elements rebuilt, nodes laid out, layers produced, …)
//!   describes what the frame did, so any difference fails: a rise is a
//!   regression, and a fall is work that went missing until a bless says it was
//!   meant;
//! - a **cost** counter (a name with `alloc` or `bytes` in it) only has to stay
//!   at or under its file. A fall passes, so a pull request that makes a
//!   scenario cheaper needs no baseline edit; a bless on a fresh `main` lowers
//!   the file, so a later rise back to the old value fails.
//!
//! A counter on one side only fails either way.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

/// The environment variable that turns the comparison on.
const MODE_VARIABLE: &str = "FLUI_PERF_BASELINE";

/// The environment variable that names a directory for the run's counts.
const RECORD_VARIABLE: &str = "FLUI_PERF_OUT";

/// The command that rewrites every file.
const BLESS_COMMAND: &str = "FLUI_PERF_BASELINE=bless cargo nextest run --locked --no-fail-fast -p flui-widgets --test perf -E 'test(/^perf_/)'";

/// Every scenario that may hold a file: [`hold`] refuses a name not listed, and
/// `every_baseline_file_is_a_scenario` fails on a file not listed. That each
/// listed scenario still runs and holds its file is the `perf-gates` step's
/// comparison of the `FLUI_PERF_OUT` records with the directory. Kept sorted,
/// which that test checks.
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
    /// The mode a value of `FLUI_PERF_BASELINE` names; `None` when it is unset.
    fn parse(value: Option<&str>) -> Result<Option<Self>, String> {
        match value {
            None => Ok(None),
            Some("check") => Ok(Some(Self::Check)),
            Some("bless") => Ok(Some(Self::Bless)),
            Some(other) => Err(format!(
                "{MODE_VARIABLE}={other:?} names no mode; use `check` or `bless`"
            )),
        }
    }
}

/// A scenario's hold on its file, kept to the end of its test. Under `bless`
/// it rewrites the file when it drops, unless the thread is panicking: a
/// scenario whose budget broke after it held its counts blesses nothing.
#[must_use = "a bless writes the file when this drops without a panic; bind it to the end of the test"]
pub(crate) struct Held {
    /// The file and the text a bless writes to it.
    bless: Option<(PathBuf, String)>,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some((path, text)) = self.bless.take()
            && !std::thread::panicking()
        {
            std::fs::write(&path, text)
                .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
        }
    }
}

/// Holds `scenario`'s `counts` to its file as `FLUI_PERF_BASELINE` says, and
/// writes them to `FLUI_PERF_OUT` when that is set. Called before a scenario
/// asserts its budget, so a check that fails reports every counter; the
/// returned [`Held`] is kept to the end of the test, so a bless waits for it.
pub(crate) fn hold(scenario: &str, counts: &[(&str, u64)]) -> Held {
    assert!(
        SCENARIOS.contains(&scenario),
        "scenario {scenario} holds a baseline file but is not in baseline::SCENARIOS"
    );
    let current: Counts = counts
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect();
    assert_eq!(
        current.len(),
        counts.len(),
        "{scenario} reports a counter twice: {counts:?}"
    );
    let mode = Mode::parse(std::env::var(MODE_VARIABLE).ok().as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    // Only a run with a mode records, so a step whose mode variable is
    // misspelled finds no records rather than a run that compared nothing.
    if mode.is_some()
        && let Some(records) = std::env::var_os(RECORD_VARIABLE)
    {
        record(Path::new(&records), scenario, &current);
    }
    hold_in(&directory(), mode, scenario, &current)
}

/// Writes `current` as `<records>/<scenario>.toml`, in the baseline file's form.
fn record(records: &Path, scenario: &str, current: &Counts) {
    std::fs::create_dir_all(records)
        .unwrap_or_else(|error| panic!("creating {}: {error}", records.display()));
    let path = records.join(format!("{scenario}.toml"));
    std::fs::write(&path, render(scenario, current))
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}

/// [`hold`] against the files of `dir`.
fn hold_in(dir: &Path, mode: Option<Mode>, scenario: &str, current: &Counts) -> Held {
    let path = dir.join(format!("{scenario}.toml"));
    match mode {
        None => Held { bless: None },
        Some(Mode::Bless) => Held {
            bless: Some((path, render(scenario, current))),
        },
        Some(Mode::Check) => {
            let findings =
                check_file(&path, scenario, current).unwrap_or_else(|error| panic!("{error}"));
            let mut report = String::new();
            for finding in &findings {
                writeln!(report, "{scenario}.{finding}").expect("BUG: writing to a String");
            }
            assert!(
                findings.iter().all(|finding| !finding.fails()),
                "{scenario} differs from {}:\n{report}a change that was meant is blessed with `{BLESS_COMMAND}`",
                path.display()
            );
            Held { bless: None }
        }
    }
}

/// Every difference between the file at `path` and `current`.
fn check_file(path: &Path, scenario: &str, current: &Counts) -> Result<Vec<Finding>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "reading {}: {error}; a new scenario's file comes from `{BLESS_COMMAND}`",
            path.display()
        )
    })?;
    let baseline =
        parse(scenario, &text).map_err(|error| format!("parsing {}: {error}", path.display()))?;
    Ok(compare(&baseline, current))
}

/// The directory that holds one `<scenario>.toml` per scenario.
pub(crate) fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("perf")
        .join("baseline")
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

/// A fresh, empty directory under the system temp dir for one test.
fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("flui-perf-baseline-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear the scratch directory");
    }
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

/// Runs `body` on a thread of its own and returns whether it panicked: a
/// [`Held`] it drops during that panic sees `std::thread::panicking()`.
fn panics(body: impl FnOnce() + Send + 'static) -> bool {
    std::thread::spawn(body).join().is_err()
}

/// The variable's values: unset is no mode, `check` and `bless` are theirs,
/// and anything else is refused rather than read as unset.
#[test]
fn the_mode_variable_is_check_bless_or_unset() {
    assert_eq!(Mode::parse(None), Ok(None));
    assert_eq!(Mode::parse(Some("check")), Ok(Some(Mode::Check)));
    assert_eq!(Mode::parse(Some("bless")), Ok(Some(Mode::Bless)));
    for refused in ["", "Check", "nope"] {
        assert!(Mode::parse(Some(refused)).is_err(), "{refused:?}");
    }
}

/// A check passes on the file's counts and panics on a work counter that
/// moved, a cost that rose and a scenario with no file; with no mode nothing
/// is read.
#[test]
fn a_check_fails_on_a_difference_that_fails_and_on_a_missing_file() {
    let dir = scratch("check");
    let held = counts(&[("allocations", 12), ("nodes_laid_out", 40)]);
    std::fs::write(dir.join("scroll.toml"), render("scroll", &held)).expect("plant a file");
    for (case, scenario, current, fails) in [
        ("equal", "scroll", held.clone(), false),
        (
            "a cost that fell",
            "scroll",
            counts(&[("allocations", 11), ("nodes_laid_out", 40)]),
            false,
        ),
        (
            "a cost that rose",
            "scroll",
            counts(&[("allocations", 13), ("nodes_laid_out", 40)]),
            true,
        ),
        (
            "work that fell",
            "scroll",
            counts(&[("allocations", 12), ("nodes_laid_out", 39)]),
            true,
        ),
        ("no file", "idle", held, true),
    ] {
        let dir = dir.clone();
        let panicked = panics(move || drop(hold_in(&dir, Some(Mode::Check), scenario, &current)));
        assert_eq!(panicked, fails, "{case}");
    }
    let dir_for_unset = dir.clone();
    assert!(
        !panics(move || drop(hold_in(&dir_for_unset, None, "idle", &BTreeMap::new()))),
        "no mode reads no file"
    );
    std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
}

/// A bless writes the scenario's file when its hold drops at the end of a test
/// that passed, and writes nothing when the test panics after holding.
#[test]
fn a_bless_writes_the_file_only_when_the_test_ends_without_a_panic() {
    let dir = scratch("bless");
    let current = counts(&[("allocations", 7)]);
    let (passing, passed_counts) = (dir.clone(), current.clone());
    assert!(!panics(move || {
        let _held = hold_in(&passing, Some(Mode::Bless), "scroll", &passed_counts);
    }));
    let written = std::fs::read_to_string(dir.join("scroll.toml")).expect("a passing bless writes");
    assert_eq!(parse("scroll", &written), Ok(current.clone()));

    let failing = dir.clone();
    assert!(panics(move || {
        let _held = hold_in(&failing, Some(Mode::Bless), "idle", &current);
        panic!("the scenario's budget broke after it held its counts");
    }));
    assert!(
        !dir.join("idle.toml").exists(),
        "a failed test blesses nothing"
    );
    std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
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
