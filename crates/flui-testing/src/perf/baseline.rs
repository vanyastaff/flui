//! The counts a perf scenario is held to: one `<scenario>.toml` per scenario in
//! a directory its target names, read and rewritten by the scenarios
//! themselves as `FLUI_PERF_BASELINE` says (ADR-0396 §2).
//!
//! A target describes its files once with a [`Baselines`] (the directory and
//! the scenarios that may hold a file) and holds each scenario with
//! [`Baselines::hold_measured`], which takes the [`Cost`] of the scenario's
//! [`measure`](super::measure) window, or with [`Baselines::hold`] for counts
//! no window measured:
//!
//! - `FLUI_PERF_BASELINE` unset: nothing is compared. A scenario holds only
//!   its own budget assertions, which hold on every host and with any feature
//!   set; the heap counters move with the platform's allocator and with the
//!   features that change what a frame builds.
//! - `check`: a scenario panics, before it asserts its budget, when its counts
//!   differ from its file in a way that fails (below).
//! - `bless`: a scenario rewrites its file from this run once its body has run
//!   to its end. A hold returns a [`Held`] the scenario keeps to the end of
//!   its body, and a panic before then (a broken budget) writes nothing.
//!
//! A file holds the lines of two runs, which build the scenarios two ways
//! (ADR-0396 §3). A run without `FLUI_PERF_INSTRUCTIONS` owns the work and
//! heap counters, at the opt-level [`BLESS_COMMAND`] builds with, and CI's
//! `test` job checks them on every pull request. A run with
//! `FLUI_PERF_INSTRUCTIONS=callgrind` owns the `instructions` line of each
//! window, built in release, and CI's `perf-gates` job checks it on `main`
//! and blesses it. Each run compares only its own lines and writes the other
//! run's back as the file has them, so neither undoes the other and no line
//! is compared across the two builds.
//!
//! When `FLUI_PERF_OUT` names a directory, a `check` or `bless` run first
//! writes each scenario's file there as `<scenario>.toml`, as a bless of this
//! run would write it. CI's `test` job compares that directory with the
//! baselines afterwards, so a scenario that held nothing (it stopped running,
//! or no longer reaches the hold) fails there, and a cost that fell, which
//! passes, shows in the job's log.
//!
//! The comparison depends on the kind of counter:
//!
//! - a **work** counter (elements rebuilt, nodes laid out, layers produced, …)
//!   describes what the frame did, so any difference fails: a rise is a
//!   regression, and a fall is work that went missing until a bless says it was
//!   meant;
//! - a **cost** counter (a name with `alloc` or `bytes` in it, and
//!   `instructions`) only has to stay at or under its file, and
//!   `instructions` within 3% over it: one build counts the same on every
//!   run, and the allowance is for what moves with no change of the code
//!   under test (the toolchain, a dependency). A fall passes, so a pull
//!   request that makes a scenario cheaper needs no baseline edit; a bless on
//!   a fresh `main` lowers the file, so a later rise back to the old value
//!   fails.
//!
//! A counter on one side only fails either way.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use super::instructions;
use super::window::Cost;
use flui_foundation::testing::heap::HeapCounts;

/// The environment variable that names the mode.
const MODE_VARIABLE: &str = "FLUI_PERF_BASELINE";

/// The environment variable that names a directory for the run's counts.
const RECORD_VARIABLE: &str = "FLUI_PERF_OUT";

/// The counter a run with `FLUI_PERF_INSTRUCTIONS` owns.
const INSTRUCTIONS: &str = "instructions";

/// How far over its file a window's instructions may rise and pass, in
/// percent.
const INSTRUCTIONS_ALLOWANCE_PERCENT: u64 = 3;

/// How far over the small tree's window the large tree's may cost in
/// instructions under [`Baselines::hold_scaling`], in percent.
const SCALING_ALLOWANCE_PERCENT: u64 = 5;

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

/// Which lines of a file a run owns: the one it compares and rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The work and heap counters, at the opt-level of [`BLESS_COMMAND`].
    Counts,
    /// The `instructions` line, in release under callgrind.
    Instructions,
}

impl Kind {
    /// Whether a run of this kind owns `counter`.
    fn owns(self, counter: &str) -> bool {
        (counter == INSTRUCTIONS) == (self == Self::Instructions)
    }

    /// The counters of `counts` a run of this kind owns.
    fn owned(self, counts: &Counts) -> Counts {
        counts
            .iter()
            .filter(|(counter, _)| self.owns(counter))
            .map(|(counter, value)| (counter.clone(), *value))
            .collect()
    }
}

/// The command that rewrites the work and heap counters of every baseline
/// file of the workspace: each perf target, with the features of CI's `test`
/// job, which checks them. The `instructions` lines are blessed by CI's
/// `perf-gates` job alone, since callgrind's counts follow the host's
/// valgrind and processor.
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

    /// Holds `scenario`'s `counts`, which no [`measure`](super::measure)
    /// window counted (a mount's per-row figures), to its file as
    /// `FLUI_PERF_BASELINE` says, and writes the file this run would bless to
    /// `FLUI_PERF_OUT` when that is set and the run compares or blesses.
    /// Called before a scenario asserts its budget, so a check that fails
    /// reports every counter; the returned [`Held`] is kept to the end of the
    /// test, so a bless waits for it.
    ///
    /// # Panics
    ///
    /// When `scenario` is not one of this target's scenarios, a counter is
    /// reported twice or is named `instructions`, the mode variable names no
    /// mode, or a check fails.
    pub fn hold(&self, scenario: &str, counts: &[(&str, u64)]) -> Held {
        self.hold_counted(scenario, counts, None)
    }

    /// Holds `scenario`'s `work` counters and the `cost` of its window to its
    /// file, as [`hold`](Self::hold) holds counts: the window's heap counts
    /// as `allocations`, `reallocations` and `allocated_bytes`, and, when
    /// `FLUI_PERF_INSTRUCTIONS=callgrind` is set, its `instructions`, counted
    /// by running this test again under callgrind.
    ///
    /// # Panics
    ///
    /// As [`hold`](Self::hold), and when instructions are counted and the run
    /// under callgrind does not start, fails, or collects nothing.
    pub fn hold_measured(&self, scenario: &str, work: &[(&str, u64)], cost: &Cost) -> Held {
        let heap = cost.heap();
        let mut counts = work.to_vec();
        counts.extend([
            ("allocations", heap.allocations),
            ("reallocations", heap.reallocations),
            ("allocated_bytes", heap.allocated_bytes),
        ]);
        self.hold_counted(scenario, &counts, Some(cost))
    }

    /// Holds one change measured on a small tree and on a large one, each as
    /// [`hold_measured`](Self::hold_measured) holds a window with no work
    /// counters, then asserts that the large tree's window costs what the
    /// small one's does: the same heap counts, and, when instructions are
    /// counted, at most 5% more of them. A frame whose cost grows with nodes
    /// it does not touch fails here though no count of its work moves.
    ///
    /// Both windows are held before either is compared with the other, so a
    /// check reports both files first.
    ///
    /// # Panics
    ///
    /// As [`hold_measured`](Self::hold_measured), and when the large tree's
    /// window costs more than the small one's.
    pub fn hold_scaling(&self, small: (&str, &Cost), large: (&str, &Cost)) -> [Held; 2] {
        let held = [
            self.hold_measured(small.0, &[], small.1),
            self.hold_measured(large.0, &[], large.1),
        ];
        assert_flat(
            Scaled::new(small.0, small.1, &held[0]),
            Scaled::new(large.0, large.1, &held[1]),
        );
        held
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

    /// [`hold`](Self::hold) of `counts` and, when the run counts
    /// instructions, of `window`'s.
    fn hold_counted(&self, scenario: &str, counts: &[(&str, u64)], window: Option<&Cost>) -> Held {
        assert!(
            self.scenarios.contains(&scenario),
            "scenario {scenario} holds a baseline file but is not one of {:?}",
            self.scenarios
        );
        // The run under callgrind only counts; its parent holds the count.
        if instructions::is_child() {
            return Held::inert();
        }
        let counted: Counts = counts
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect();
        assert_eq!(
            counted.len(),
            counts.len(),
            "{scenario} reports a counter twice: {counts:?}"
        );
        assert!(
            !counted.contains_key(INSTRUCTIONS),
            "{scenario} reports `{INSTRUCTIONS}`, which only a measured window counts"
        );
        let mode = Mode::parse(std::env::var(MODE_VARIABLE).ok().as_deref())
            .unwrap_or_else(|error| panic!("{error}"));
        let records = std::env::var_os(RECORD_VARIABLE).map(PathBuf::from);
        let (kind, run) = if instructions::counting() {
            let count = window.map(|cost| instructions::count(cost.window()));
            let run = count
                .map(|count| (INSTRUCTIONS.to_owned(), count))
                .into_iter()
                .collect();
            (Kind::Instructions, run)
        } else {
            (Kind::Counts, counted)
        };
        let mut held = self.hold_in(mode, kind, records.as_deref(), scenario, &run);
        held.instructions = run.get(INSTRUCTIONS).copied();
        held
    }

    /// Holds what a run of `kind` counted of `scenario` (`run`, only
    /// counters that kind owns) in `mode`, recording into `records` when
    /// given.
    fn hold_in(
        &self,
        mode: Option<Mode>,
        kind: Kind,
        records: Option<&Path>,
        scenario: &str,
        run: &Counts,
    ) -> Held {
        let Some(mode) = mode else {
            return Held::inert();
        };
        let path = self.dir.join(format!("{scenario}.toml"));
        let file = read_file(&path, scenario);
        // The file this run blesses: its own counters, and the other run's
        // as the file has them.
        let written = || {
            let mut counts = match &file {
                Ok(counts) => counts.clone(),
                Err(FileError::Absent(_)) => Counts::new(),
                Err(error) => panic!("{error}"),
            };
            counts.retain(|counter, _| !kind.owns(counter));
            counts.extend(run.iter().map(|(counter, value)| (counter.clone(), *value)));
            render(scenario, &counts)
        };
        // Only a run with a mode records, so a step whose mode variable is
        // misspelled finds no records rather than a run that compared nothing.
        if let Some(records) = records {
            record(records, scenario, &written());
        }
        match mode {
            Mode::Bless => Held {
                bless: Some((path, written())),
                instructions: None,
            },
            Mode::Check => {
                let file = file.unwrap_or_else(|error| panic!("{error}"));
                let findings = compare(&kind.owned(&file), run);
                let mut report = String::new();
                for finding in &findings {
                    writeln!(report, "{scenario}.{finding}").expect("BUG: writing to a String");
                }
                let bless = match kind {
                    Kind::Counts => format!("`{BLESS_COMMAND}`"),
                    Kind::Instructions => {
                        "CI's `perf-gates` job, whose log and artifact hold the files it blessed"
                            .to_owned()
                    }
                };
                assert!(
                    findings.iter().all(|finding| !finding.fails()),
                    "{scenario} differs from {}:\n{report}a change that was meant is blessed by {bless}",
                    path.display(),
                );
                Held::inert()
            }
        }
    }
}

/// One tree's side of a [`Baselines::hold_scaling`]: what its window cost.
#[derive(Debug, Clone, Copy)]
struct Scaled<'a> {
    scenario: &'a str,
    heap: HeapCounts,
    instructions: Option<u64>,
}

impl<'a> Scaled<'a> {
    fn new(scenario: &'a str, cost: &Cost, held: &Held) -> Self {
        Self {
            scenario,
            heap: cost.heap(),
            instructions: held.instructions,
        }
    }
}

/// Asserts that the large tree's window costs what the small one's does: the
/// same heap counts, and, when both counted instructions, at most
/// [`SCALING_ALLOWANCE_PERCENT`] more of them.
fn assert_flat(small: Scaled<'_>, large: Scaled<'_>) {
    assert_eq!(
        small.heap, large.heap,
        "{} asked {:?} of the heap and {} {:?}",
        small.scenario, small.heap, large.scenario, large.heap
    );
    if let (Some(small_count), Some(large_count)) = (small.instructions, large.instructions) {
        assert!(
            !exceeds(large_count, small_count, SCALING_ALLOWANCE_PERCENT),
            "{} costs {large_count} instructions, more than \
             {SCALING_ALLOWANCE_PERCENT}% over {}'s {small_count}",
            large.scenario,
            small.scenario
        );
    }
}

/// Why a scenario's file gave no counts.
#[derive(Debug)]
enum FileError {
    /// The file does not exist: a scenario a bless has not written yet.
    Absent(String),
    /// The file is unreadable or does not parse.
    Broken(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent(message) | Self::Broken(message) => f.write_str(message),
        }
    }
}

/// The counters of `scenario`'s file at `path`.
fn read_file(path: &Path, scenario: &str) -> Result<Counts, FileError> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        let message = format!(
            "reading {}: {error}; a new scenario's file comes from `{BLESS_COMMAND}`",
            path.display(),
        );
        if error.kind() == std::io::ErrorKind::NotFound {
            FileError::Absent(message)
        } else {
            FileError::Broken(message)
        }
    })?;
    parse(scenario, &text)
        .map_err(|error| FileError::Broken(format!("parsing {}: {error}", path.display())))
}

/// The file's text: a header, then the scenario's table with its counters
/// sorted.
fn render(scenario: &str, counts: &Counts) -> String {
    let mut text = format!("# Written by `{BLESS_COMMAND}`");
    if counts.contains_key(INSTRUCTIONS) {
        text.push_str(",\n# `instructions` by CI's perf-gates job");
    }
    write!(text, ";\n# counts, not timings.\n\n[{scenario}]\n").expect("BUG: writing to a String");
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
    /// The instructions this hold counted of its window, when it counted.
    instructions: Option<u64>,
}

impl Held {
    /// A hold that writes nothing.
    fn inert() -> Self {
        Self {
            bless: None,
            instructions: None,
        }
    }
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

/// Whether `counter` measures cost (heap traffic, instructions), which only
/// has to stay at or under its file, rather than work, which has to match it.
fn is_cost(counter: &str) -> bool {
    counter == INSTRUCTIONS || counter.contains("alloc") || counter.contains("bytes")
}

/// How far, in percent, `counter` may rise over its file and pass.
fn allowance_percent(counter: &str) -> u64 {
    if counter == INSTRUCTIONS {
        INSTRUCTIONS_ALLOWANCE_PERCENT
    } else {
        0
    }
}

/// Whether `value` is more than `percent`% over `base`, in arithmetic that
/// cannot overflow.
fn exceeds(value: u64, base: u64, percent: u64) -> bool {
    u128::from(value) * 100 > u128::from(base) * u128::from(100 + percent)
}

/// One difference between a scenario's file and a run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    /// A counter rose past what it may.
    Rose {
        counter: String,
        base: u64,
        current: u64,
    },
    /// A cost counter rose within its allowance. It passes.
    Within {
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
    /// Whether the check fails on it: everything but a cost that fell or
    /// rose within its allowance.
    fn fails(&self) -> bool {
        !matches!(self, Self::Cheaper { .. } | Self::Within { .. })
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
            Self::Within {
                counter,
                base,
                current,
            } => write!(
                f,
                "{counter}: rose {base} -> {current} (passes: within {}%)",
                allowance_percent(counter)
            ),
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
        if exceeds(value, base, allowance_percent(&counter)) {
            findings.push(Finding::Rose {
                counter,
                base,
                current: value,
            });
        } else if value > base {
            findings.push(Finding::Within {
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
    /// instructions pass a rise of up to 3%, and a counter on one side only
    /// fails.
    #[test]
    fn the_comparison_passes_only_an_equal_counter_or_a_cost_within_its_allowance() {
        let base = counts(&[
            ("allocations", 12),
            ("instructions", 1_000),
            ("nodes_laid_out", 40),
        ]);
        let with = |pairs: &[(&str, u64)]| {
            let mut run = base.clone();
            run.extend(counts(pairs));
            run
        };
        let rose = |counter: &str, base, current| Finding::Rose {
            counter: counter.to_owned(),
            base,
            current,
        };
        for (case, current, expected, passes) in [
            ("equal", base.clone(), vec![], true),
            (
                "work rose",
                with(&[("nodes_laid_out", 41)]),
                vec![rose("nodes_laid_out", 40, 41)],
                false,
            ),
            (
                "cost rose",
                with(&[("allocations", 13)]),
                vec![rose("allocations", 12, 13)],
                false,
            ),
            (
                "instructions rose 3%",
                with(&[("instructions", 1_030)]),
                vec![Finding::Within {
                    counter: "instructions".to_owned(),
                    base: 1_000,
                    current: 1_030,
                }],
                true,
            ),
            (
                "instructions rose past 3%",
                with(&[("instructions", 1_031)]),
                vec![rose("instructions", 1_000, 1_031)],
                false,
            ),
            (
                "work fell",
                with(&[("nodes_laid_out", 39)]),
                vec![Finding::Fell {
                    counter: "nodes_laid_out".to_owned(),
                    base: 40,
                    current: 39,
                }],
                false,
            ),
            (
                "cost fell",
                with(&[("allocations", 11)]),
                vec![Finding::Cheaper {
                    counter: "allocations".to_owned(),
                    base: 12,
                    current: 11,
                }],
                true,
            ),
            (
                "a counter on one side only",
                counts(&[
                    ("instructions", 1_000),
                    ("nodes_laid_out", 40),
                    ("layers_reused", 0),
                ]),
                vec![
                    Finding::Missing {
                        counter: "allocations".to_owned(),
                    },
                    Finding::New {
                        counter: "layers_reused".to_owned(),
                    },
                ],
                false,
            ),
        ] {
            let found = compare(&base, &current);
            assert_eq!(found, expected, "{case}");
            assert_eq!(
                found.iter().all(|finding| !finding.fails()),
                passes,
                "{case}"
            );
        }
        assert!(
            is_cost("allocated_bytes")
                && is_cost("reallocations")
                && is_cost("instructions")
                && !is_cost("nodes_painted")
        );
        assert!(
            !exceeds(u64::MAX, u64::MAX, 0) && exceeds(u64::MAX, u64::MAX / 2, 5),
            "the allowance's arithmetic does not overflow"
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

    /// A scaling hold passes equal heap counts with instructions within 5%,
    /// and fails a heap count that differs or instructions past 5%; with no
    /// instructions counted it compares the heap alone.
    #[test]
    fn a_scaling_hold_fails_a_large_tree_that_costs_more() {
        let heap = |allocations| HeapCounts {
            allocations,
            ..HeapCounts::default()
        };
        let side = |scenario, allocations, instructions| Scaled {
            scenario,
            heap: heap(allocations),
            instructions,
        };
        for (case, large, fails) in [
            ("the same", side("large", 4, Some(1_000)), false),
            ("5% more instructions", side("large", 4, Some(1_050)), false),
            ("past 5%", side("large", 4, Some(1_051)), true),
            ("more allocations", side("large", 5, Some(1_000)), true),
            ("no instructions counted", side("large", 4, None), false),
        ] {
            let small = side("small", 4, Some(1_000));
            let panicked = panics(move || assert_flat(small, large));
            assert_eq!(panicked, fails, "{case}");
        }
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
                drop(files.hold_in(Some(Mode::Check), Kind::Counts, None, scenario, &current));
            });
            assert_eq!(panicked, fails, "{case}");
        }
        let unset = files;
        assert!(
            !panics(move || drop(unset.hold_in(
                None,
                Kind::Counts,
                None,
                "idle",
                &BTreeMap::new()
            ))),
            "no mode reads no file"
        );
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    /// Each kind of run compares only its own lines: a counts run passes
    /// whatever the file's `instructions` line holds, and an instructions run
    /// passes whatever its counts lines hold but fails on an instruction
    /// count past its allowance, one the file lacks, and a window that no
    /// longer counts one.
    #[test]
    fn each_kind_of_run_compares_only_its_own_lines() {
        let dir = scratch("kinds");
        let files = baselines(&dir);
        let file = counts(&[("allocations", 12), ("instructions", 1_000)]);
        std::fs::write(dir.join("scroll.toml"), render("scroll", &file)).expect("plant a file");
        std::fs::write(
            dir.join("idle.toml"),
            render("idle", &counts(&[("allocations", 0)])),
        )
        .expect("plant a file");
        for (case, kind, scenario, run, fails) in [
            (
                "counts, the file's instructions aside",
                Kind::Counts,
                "scroll",
                counts(&[("allocations", 12)]),
                false,
            ),
            (
                "instructions, the file's counts aside",
                Kind::Instructions,
                "scroll",
                counts(&[("instructions", 1_000)]),
                false,
            ),
            (
                "instructions past the allowance",
                Kind::Instructions,
                "scroll",
                counts(&[("instructions", 1_031)]),
                true,
            ),
            (
                "instructions the file lacks",
                Kind::Instructions,
                "idle",
                counts(&[("instructions", 1_000)]),
                true,
            ),
            (
                "a window that counts none",
                Kind::Instructions,
                "scroll",
                Counts::new(),
                true,
            ),
            (
                "a hold of no window, with none in the file",
                Kind::Instructions,
                "idle",
                Counts::new(),
                false,
            ),
        ] {
            let files = files.clone();
            let panicked = panics(move || {
                drop(files.hold_in(Some(Mode::Check), kind, None, scenario, &run));
            });
            assert_eq!(panicked, fails, "{case}");
        }
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
            let _held = passing.hold_in(
                Some(Mode::Bless),
                Kind::Counts,
                None,
                "scroll",
                &passed_counts,
            );
        }));
        let written =
            std::fs::read_to_string(dir.join("scroll.toml")).expect("a passing bless writes");
        assert_eq!(parse("scroll", &written), Ok(current.clone()));

        let failing = baselines(&dir);
        assert!(panics(move || {
            let _held = failing.hold_in(Some(Mode::Bless), Kind::Counts, None, "idle", &current);
            panic!("the scenario's budget broke after it held its counts");
        }));
        assert!(
            !dir.join("idle.toml").exists(),
            "a failed test blesses nothing"
        );
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    /// A bless of either kind rewrites its own lines and writes the other
    /// kind's back as the file had them.
    #[test]
    fn a_bless_keeps_the_lines_of_the_other_kind() {
        let dir = scratch("bless-kinds");
        let files = baselines(&dir);
        let file = counts(&[("allocations", 12), ("instructions", 1_000)]);
        for (kind, run, blessed) in [
            (
                Kind::Counts,
                counts(&[("allocations", 9), ("nodes_laid_out", 3)]),
                counts(&[
                    ("allocations", 9),
                    ("instructions", 1_000),
                    ("nodes_laid_out", 3),
                ]),
            ),
            (
                Kind::Instructions,
                counts(&[("instructions", 900)]),
                counts(&[("allocations", 12), ("instructions", 900)]),
            ),
        ] {
            std::fs::write(dir.join("scroll.toml"), render("scroll", &file)).expect("plant a file");
            drop(files.hold_in(Some(Mode::Bless), kind, None, "scroll", &run));
            let written = std::fs::read_to_string(dir.join("scroll.toml")).expect("a bless writes");
            assert_eq!(parse("scroll", &written), Ok(blessed), "{kind:?}");
        }
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    /// A run with a mode writes each scenario's file to the records directory
    /// as its bless would write it, the other kind's lines included; a run
    /// with no mode writes nothing there, so a misspelled mode variable leaves
    /// the records step with missing files.
    #[test]
    fn only_a_run_with_a_mode_records_its_counts() {
        let dir = scratch("records-baseline");
        let files = baselines(&dir);
        let current = counts(&[("allocations", 7), ("nodes_laid_out", 3)]);
        let mut file = current.clone();
        file.insert("instructions".to_owned(), 1_000);
        std::fs::write(dir.join("scroll.toml"), render("scroll", &file)).expect("plant a file");
        for (case, mode, records_written) in [
            ("no mode", None, false),
            ("check", Some(Mode::Check), true),
            ("bless", Some(Mode::Bless), true),
        ] {
            let records = scratch(&format!("records-{}", u8::from(records_written)));
            std::fs::remove_dir_all(&records).expect("start with no records directory");
            drop(files.hold_in(mode, Kind::Counts, Some(&records), "scroll", &current));
            let record = records.join("scroll.toml");
            assert_eq!(record.exists(), records_written, "{case}");
            if records_written {
                let text = std::fs::read_to_string(&record).expect("the record reads");
                assert_eq!(text, render("scroll", &file), "{case}");
                std::fs::remove_dir_all(&records).expect("remove the records directory");
            }
        }
        std::fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }
}
