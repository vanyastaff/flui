//! The growth one merge brings: a `main` push's binary sizes against the
//! commit before it.
//!
//! The baseline's tolerance is cumulative: merges that each add a little pass
//! until the change that crosses it, which is usually someone else's. So each
//! `main` push records a [`Measurement`] of its commit (CI caches it under
//! the commit's SHA and hands the parent's back through `--previous`), and a
//! push whose binary grew by more than [`STEP_TOLERANCE_PER_MILLE`] over its
//! parent fails, naming both commits and the delta, so the growth is answered
//! at its own merge. A push that restates the baseline between the two
//! measurements has answered it already: the counter it blessed is not
//! compared.

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// How far one push may grow a byte counter over the push before it, in
/// thousandths: about 110 KB of the counter's 22 MB host binary.
pub(super) const STEP_TOLERANCE_PER_MILLE: u64 = 5;

/// The byte sizes measured at one commit: what `--record` writes and
/// `--previous` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Measurement {
    /// The commit the sizes were measured at.
    pub(super) commit: String,
    /// Each byte counter, by its baseline name.
    pub(super) bytes: BTreeMap<String, u64>,
    /// The baseline's byte counters at that commit: a counter whose baseline
    /// differs between two measurements was blessed in between.
    pub(super) baseline: BTreeMap<String, u64>,
}

impl Measurement {
    /// Reads a recorded measurement; `None` when there is none yet (the first
    /// push after the cache expired).
    pub(super) fn read(path: &Path) -> anyhow::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let measurement =
            toml::from_str::<Self>(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(measurement))
    }

    /// Writes this measurement for the next push to read.
    pub(super) fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(path, toml::to_string(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }
}

/// A byte counter that grew past [`STEP_TOLERANCE_PER_MILLE`] in one push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepGrowth {
    pub(super) counter: String,
    pub(super) previous_commit: String,
    pub(super) previous: u64,
    pub(super) commit: String,
    pub(super) current: u64,
}

impl fmt::Display for StepGrowth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GREW       size.{}: {} -> {} ({}) at {} over {}; one merge may add {}.{}%: \
             make it smaller, or justify it in a follow-up pull request",
            self.counter,
            self.previous,
            self.current,
            Delta::new(self.previous, self.current),
            self.commit,
            self.previous_commit,
            STEP_TOLERANCE_PER_MILLE / 10,
            STEP_TOLERANCE_PER_MILLE % 10,
        )
    }
}

/// Every byte counter of `current` that grew past the step tolerance over
/// `previous`. A counter on one side only is not compared (the baseline
/// comparison reports it), and neither is one whose baseline was restated
/// between the two: that bless said in its pull request why the counter grew.
pub(super) fn grown(previous: &Measurement, current: &Measurement) -> Vec<StepGrowth> {
    current
        .bytes
        .iter()
        .filter_map(|(counter, &value)| {
            let &before = previous.bytes.get(counter)?;
            if previous.baseline.get(counter) != current.baseline.get(counter) {
                return None;
            }
            past_step(before, value).then(|| StepGrowth {
                counter: counter.clone(),
                previous_commit: previous.commit.clone(),
                previous: before,
                commit: current.commit.clone(),
                current: value,
            })
        })
        .collect()
}

/// Whether `current` exceeds `previous` by more than the step tolerance.
/// Widened before multiplying, so no size overflows the comparison.
fn past_step(previous: u64, current: u64) -> bool {
    u128::from(current) * 1000 > u128::from(previous) * u128::from(1000 + STEP_TOLERANCE_PER_MILLE)
}

/// The Markdown table a run appends to the job summary: each byte counter at
/// the baseline, at the previous push and at this commit, with this commit's
/// delta over each. It is the record a later `--bless` cites to attribute the
/// growth it restates.
pub(super) fn summary(
    baseline: &BTreeMap<String, u64>,
    previous: Option<&Measurement>,
    current: &Measurement,
) -> String {
    let mut table = String::from("### Release sizes\n\n");
    let previous_header = previous.map_or_else(
        || "previous push (none recorded)".to_owned(),
        |previous| format!("previous push `{}`", previous.commit),
    );
    let _ = writeln!(
        table,
        "| counter | baseline | {previous_header} | `{}` | over the baseline | over the previous push |",
        current.commit
    );
    table.push_str("|---|---:|---:|---:|---:|---:|\n");
    for (counter, &value) in &current.bytes {
        let base = baseline.get(counter).copied();
        let before = previous.and_then(|previous| previous.bytes.get(counter).copied());
        let cell = |side: Option<u64>| side.map_or_else(|| "–".to_owned(), |v| v.to_string());
        let delta = |side: Option<u64>| {
            side.map_or_else(|| "–".to_owned(), |v| Delta::new(v, value).to_string())
        };
        let _ = writeln!(
            table,
            "| `{counter}` | {} | {} | {value} | {} | {} |",
            cell(base),
            cell(before),
            delta(base),
            delta(before),
        );
    }
    table
}

/// A signed byte delta with its share of the earlier size, in hundredths of a
/// percent, computed in integers so no float rounding moves a boundary.
struct Delta {
    bytes: i128,
    basis_points: i128,
}

impl Delta {
    fn new(before: u64, after: u64) -> Self {
        let bytes = i128::from(after) - i128::from(before);
        let basis_points = if before == 0 {
            0
        } else {
            bytes * 10_000 / i128::from(before)
        };
        Self {
            bytes,
            basis_points,
        }
    }
}

impl fmt::Display for Delta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.bytes < 0 { "-" } else { "+" };
        let points = self.basis_points.unsigned_abs();
        write!(
            f,
            "{sign}{} B, {sign}{}.{:02}%",
            self.bytes.unsigned_abs(),
            points / 100,
            points % 100
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(commit: &str, bytes: &[(&str, u64)]) -> Measurement {
        Measurement {
            commit: commit.to_owned(),
            bytes: bytes
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect(),
            baseline: BTreeMap::new(),
        }
    }

    #[test]
    fn a_counter_blessed_between_two_pushes_is_not_a_step() {
        let previous = at("aaa", &[("counter_release_bytes", 1000)]);
        let mut current = at("bbb", &[("counter_release_bytes", 1100)]);
        assert_eq!(
            grown(&previous, &current).len(),
            1,
            "unblessed growth fails"
        );
        current
            .baseline
            .insert("counter_release_bytes".to_owned(), 1100);
        assert_eq!(grown(&previous, &current), Vec::new(), "a bless answers it");
    }

    #[test]
    fn step_growth_table() {
        let previous = at("aaa", &[("counter_release_bytes", 1000)]);
        for (case, current, expected) in [
            ("equal holds", 1000, false),
            ("within the step holds", 1005, false),
            ("past the step fails", 1006, true),
            ("a fall holds", 900, false),
        ] {
            let current = at("bbb", &[("counter_release_bytes", current)]);
            let found = grown(&previous, &current);
            assert_eq!(!found.is_empty(), expected, "{case}");
        }
        assert!(!past_step(u64::MAX, u64::MAX));
    }

    #[test]
    fn a_counter_on_one_side_only_is_not_a_step() {
        let previous = at("aaa", &[("counter_release_bytes", 1000)]);
        let current = at("bbb", &[("counter_wasm_bytes", 5000)]);
        assert_eq!(grown(&previous, &current), Vec::new());
    }

    #[test]
    fn a_step_failure_names_both_commits_and_the_delta() {
        let previous = at("aaa111", &[("counter_release_bytes", 20_000_000)]);
        let current = at("bbb222", &[("counter_release_bytes", 20_150_000)]);
        let [growth] = grown(&previous, &current)
            .try_into()
            .expect("one counter grew");
        let message = growth.to_string();
        for part in [
            "size.counter_release_bytes: 20000000 -> 20150000",
            "+150000 B, +0.75%",
            "at bbb222 over aaa111",
            "0.5%",
        ] {
            assert!(message.contains(part), "{part:?} in {message}");
        }
    }

    #[test]
    fn a_measurement_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("xtask-size-step-{}", std::process::id()));
        let path = dir.join("measurement.toml");
        let measurement = at(
            "abc",
            &[("counter_release_bytes", 7), ("counter_wasm_bytes", 9)],
        );
        measurement.write(&path).expect("written");
        // CI saves the file only when it holds this line for its own commit.
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(
            text.lines().any(|line| line == "commit = \"abc\""),
            "{text}"
        );
        let read = Measurement::read(&path).expect("read");
        std::fs::remove_dir_all(&dir).expect("removed");
        assert_eq!(read, Some(measurement));
        assert_eq!(Measurement::read(&path).expect("read"), None);
    }

    #[test]
    fn the_summary_shows_each_delta() {
        let baseline: BTreeMap<String, u64> = [("counter_release_bytes".to_owned(), 1000)].into();
        let previous = at("aaa", &[("counter_release_bytes", 1010)]);
        let current = at("bbb", &[("counter_release_bytes", 1020)]);
        let table = summary(&baseline, Some(&previous), &current);
        assert!(
            table.contains(
                "| `counter_release_bytes` | 1000 | 1010 | 1020 | +20 B, +2.00% | +10 B, +0.99% |"
            ),
            "{table}"
        );
        let first = summary(&baseline, None, &current);
        assert!(first.contains("previous push (none recorded)"), "{first}");
        assert!(
            first.contains("| 1000 | – | 1020 | +20 B, +2.00% | – |"),
            "{first}"
        );
    }
}
