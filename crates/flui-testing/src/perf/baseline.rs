//! The counts a perf scenario is held to: one `<scenario>.toml` per scenario in
//! a directory its target names, read and rewritten by the scenarios
//! themselves as `FLUI_PERF_BASELINE` says (ADR-0396 §2).
//!
//! A target describes its files once with a [`Baselines`] (the directory, the
//! scenarios that may hold a file, the command that blesses them) and holds
//! each scenario's counters with [`Baselines::hold`]:
//!
//! - `FLUI_PERF_BASELINE` unset: nothing is compared. A scenario holds only
//!   its own budget assertions, which hold on every host and with any feature
//!   set; the heap counters move with the platform's allocator and with the
//!   features that change what a frame builds.
//! - `check`: a scenario panics, before it asserts its budget, when its counts
//!   differ from its file in a way that fails (below). CI's `test` job runs
//!   it, on Linux and with the features the files were blessed with.
//! - `bless`: a scenario rewrites its file from this run once its body has run
//!   to its end. [`Baselines::hold`] returns a [`Held`] the scenario keeps to
//!   the end of its body, and a panic before then (a broken budget) writes
//!   nothing.
//!
//! When `FLUI_PERF_OUT` names a directory, a `check` or `bless` run first
//! writes each scenario's counts there as `<scenario>.toml`, in the baseline
//! file's form. CI's `test` job compares that directory with the baselines
//! afterwards, so a scenario that held nothing (it stopped running, or no
//! longer reaches the hold) fails there, and a cost that fell, which passes,
//! shows in the job's log.
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

/// The environment variable that names the mode.
const MODE_VARIABLE: &str = "FLUI_PERF_BASELINE";

/// The environment variable that names a directory for the run's counts.
const RECORD_VARIABLE: &str = "FLUI_PERF_OUT";

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

/// The command that rewrites every baseline file of the workspace: each perf
/// target, with the features of CI's `test` job, which checks them.
pub const BLESS_COMMAND: &str = "FLUI_PERF_BASELINE=bless CARGO_PROFILE_DEV_OPT_LEVEL=0 cargo nextest run --locked --no-fail-fast --workspace --exclude flui-native --features flui/material,flui/cupertino,flui/native --test perf";

/// One target's baseline files: where they live and which scenarios may hold
/// one.
#[derive(Debug, Clone)]
pub struct Baselines {
    dir: PathBuf,
    scenarios: &'static [&'static str],
}

impl Baselines {
    /// The files of `dir`, one for each of `scenarios` (kept sorted).
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, scenarios: &'static [&'static str]) -> Self {
        Self {
            dir: dir.into(),
            scenarios,
        }
    }

    /// Holds `scenario`'s `counts` to its file as `FLUI_PERF_BASELINE` says,
    /// and writes them to `FLUI_PERF_OUT` when that is set and the run
    /// compares or blesses. Called before a scenario asserts its budget, so a
    /// check that fails reports every counter; the returned [`Held`] is kept
    /// to the end of the test, so a bless waits for it.
    ///
    /// # Panics
    ///
    /// When `scenario` is not one of this target's scenarios, a counter is
    /// reported twice, the mode variable names no mode, or a check fails.
    pub fn hold(&self, scenario: &str, counts: &[(&str, u64)]) -> Held {
        assert!(
            self.scenarios.contains(&scenario),
            "scenario {scenario} holds a baseline file but is not one of {:?}",
            self.scenarios
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
        let records = std::env::var_os(RECORD_VARIABLE).map(PathBuf::from);
        self.hold_in(mode, records.as_deref(), scenario, &current)
    }

    /// Asserts that the directory holds a parsing file for each scenario and
    /// no other, and that the scenarios are listed sorted: the test a target
    /// runs so that a renamed scenario cannot leave its old file behind.
    ///
    /// # Panics
    ///
    /// When the list is unsorted, a file is unreadable or does not parse, or
    /// the files and the scenarios differ.
    pub fn assert_files_are_scenarios(&self) {
        let mut sorted = self.scenarios.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, self.scenarios, "the scenarios are kept sorted");
        let mut files: Vec<String> = std::fs::read_dir(&self.dir)
            .unwrap_or_else(|error| panic!("reading {}: {error}", self.dir.display()))
            .map(|entry| {
                let path = entry.expect("BUG: a readable directory entry").path();
                assert_eq!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("toml"),
                    "{}",
                    path.display()
                );
                let scenario = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("BUG: a baseline file has a UTF-8 name")
                    .to_owned();
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
                if let Err(error) = parse(&scenario, &text) {
                    panic!("{}: {error}", path.display());
                }
                scenario
            })
            .collect();
        files.sort_unstable();
        assert_eq!(
            files,
            self.scenarios,
            "the files of {} and the scenarios",
            self.dir.display()
        );
    }

    /// [`hold`](Self::hold) in `mode`, recording into `records` when given.
    fn hold_in(
        &self,
        mode: Option<Mode>,
        records: Option<&Path>,
        scenario: &str,
        current: &Counts,
    ) -> Held {
        // Only a run with a mode records, so a step whose mode variable is
        // misspelled finds no records rather than a run that compared nothing.
        if mode.is_some()
            && let Some(records) = records
        {
            record(records, scenario, &render(scenario, current));
        }
        let path = self.dir.join(format!("{scenario}.toml"));
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
                    path.display(),
                );
                Held { bless: None }
            }
        }
    }
}

/// Every difference between the file at `path` and `current`.
fn check_file(path: &Path, scenario: &str, current: &Counts) -> Result<Vec<Finding>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "reading {}: {error}; a new scenario's file comes from `{BLESS_COMMAND}`",
            path.display(),
        )
    })?;
    let baseline =
        parse(scenario, &text).map_err(|error| format!("parsing {}: {error}", path.display()))?;
    Ok(compare(&baseline, current))
}

/// The file's text: a header, then the scenario's table with its counters
/// sorted.
fn render(scenario: &str, counts: &Counts) -> String {
    let mut text =
        format!("# Written by `{BLESS_COMMAND}`;\n# counts, not timings.\n\n[{scenario}]\n");
    for (name, value) in counts {
        writeln!(text, "{name} = {value}").expect("BUG: writing to a String");
    }
    text
}

/// A scenario's hold on its file, kept to the end of its test body. Under
/// `bless` it rewrites the file when it drops, unless the thread is panicking:
/// a scenario whose budget broke after it held its counts blesses nothing. A
/// value the body declared before the hold drops after it, so a panic in that
/// teardown (a runtime unmounting its tree) fails the test after the file is
/// written.
#[must_use = "a bless writes the file when this drops without a panic; bind it to the end of the test"]
#[derive(Debug)]
pub struct Held {
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

/// Writes `text` as `<records>/<scenario>.toml`.
fn record(records: &Path, scenario: &str, text: &str) {
    std::fs::create_dir_all(records)
        .unwrap_or_else(|error| panic!("creating {}: {error}", records.display()));
    let path = records.join(format!("{scenario}.toml"));
    std::fs::write(&path, text)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
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
            } => write!(f, "{counter}: rose {base} -> {current}"),
            Self::Fell {
                counter,
                base,
                current,
            } => write!(
                f,
                "{counter}: fell {base} -> {current} (work went missing, or bless it)"
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(pairs: &[(&str, u64)]) -> Counts {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect()
    }

    fn baselines(dir: &Path) -> Baselines {
        Baselines::new(dir, &["idle", "scroll"])
    }

    /// A rise of either kind fails, a fall fails for work and passes for cost,
    /// and a counter on one side only fails.
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
        assert!(
            is_cost("allocated_bytes") && is_cost("reallocations") && !is_cost("nodes_painted")
        );
    }

    /// A blessed file parses back to the counts it was written from, and a file
    /// that holds another scenario's table, a line that is not a count or a
    /// counter set twice does not parse.
    #[test]
    fn a_blessed_file_parses_back_and_a_malformed_one_does_not() {
        let written = counts(&[("allocations", 196), ("reallocations", 4)]);
        let text = render("scroll", &written);
        assert_eq!(parse("scroll", &text), Ok(written));
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

    /// The variable's values: unset is no mode, `check` and `bless` are
    /// theirs, and anything else is refused rather than read as unset.
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
    /// moved, a cost that rose and a scenario with no file; with no mode
    /// nothing is read.
    #[test]
    fn a_check_fails_on_a_difference_that_fails_and_on_a_missing_file() {
        let dir = scratch("check");
        let held = counts(&[("allocations", 12), ("nodes_laid_out", 40)]);
        let files = baselines(&dir);
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
            let files = files.clone();
            let panicked = panics(move || {
                drop(files.hold_in(Some(Mode::Check), None, scenario, &current));
            });
            assert_eq!(panicked, fails, "{case}");
        }
        let unset = files;
        assert!(
            !panics(move || drop(unset.hold_in(None, None, "idle", &BTreeMap::new()))),
            "no mode reads no file"
        );
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    /// A bless writes the scenario's file when its hold drops at the end of a
    /// test that passed, and writes nothing when the test panics after holding.
    #[test]
    fn a_bless_writes_the_file_only_when_the_test_ends_without_a_panic() {
        let dir = scratch("bless");
        let current = counts(&[("allocations", 7)]);
        let (passing, passed_counts) = (baselines(&dir), current.clone());
        assert!(!panics(move || {
            let _held = passing.hold_in(Some(Mode::Bless), None, "scroll", &passed_counts);
        }));
        let written =
            std::fs::read_to_string(dir.join("scroll.toml")).expect("a passing bless writes");
        assert_eq!(parse("scroll", &written), Ok(current.clone()));

        let failing = baselines(&dir);
        assert!(panics(move || {
            let _held = failing.hold_in(Some(Mode::Bless), None, "idle", &current);
            panic!("the scenario's budget broke after it held its counts");
        }));
        assert!(
            !dir.join("idle.toml").exists(),
            "a failed test blesses nothing"
        );
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    /// A run with a mode writes each scenario's counts to the records directory
    /// in the baseline file's form; a run with no mode writes nothing there, so
    /// a misspelled mode variable leaves the records step with missing files.
    #[test]
    fn only_a_run_with_a_mode_records_its_counts() {
        let dir = scratch("records-baseline");
        let files = baselines(&dir);
        let current = counts(&[("allocations", 7), ("nodes_laid_out", 3)]);
        std::fs::write(dir.join("scroll.toml"), render("scroll", &current)).expect("plant a file");
        for (case, mode, records_written) in [
            ("no mode", None, false),
            ("check", Some(Mode::Check), true),
            ("bless", Some(Mode::Bless), true),
        ] {
            let records = scratch(&format!("records-{}", u8::from(records_written)));
            std::fs::remove_dir_all(&records).expect("start with no records directory");
            drop(files.hold_in(mode, Some(&records), "scroll", &current));
            let record = records.join("scroll.toml");
            assert_eq!(record.exists(), records_written, "{case}");
            if records_written {
                let text = std::fs::read_to_string(&record).expect("the record reads");
                assert_eq!(text, render("scroll", &current), "{case}");
                std::fs::remove_dir_all(&records).expect("remove the records directory");
            }
        }
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }
}
