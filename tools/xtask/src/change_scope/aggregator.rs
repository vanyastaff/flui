//! The `ci` aggregator's rule (ci.yml, job `ci`), and a reader for the parts of
//! ci.yml it depends on.
//!
//! Two rules:
//!
//! 1. COMPLETENESS -- every job in the workflow except `ci` itself (and
//!    `notify-main-red`, which runs after it) must appear in `ci`'s `needs`.
//!    Without this, a job added without editing that list ran, passed or
//!    failed, and gated nothing.
//! 2. EXACTLY THE EXPECTED SKIPS -- which jobs may skip is decided by `plan`
//!    alone, and the aggregator recomputes it from `plan`'s outputs instead of
//!    trusting each job's `if:`:
//!    - heavy lane (main, merge queue, nightly, dispatch, `full-ci`): only
//!      fast-lane and fast-lane-ios skip; every heavy job must succeed;
//!    - ordinary PR, mode packages/full: every heavy job skips, fast-lane and
//!      deps must succeed, fast-lane-ios too when plan puts the iOS runner in
//!      scope (`cross_ios`);
//!    - ordinary PR, mode none (repo tooling only): heavy + fast-lane skip;
//!    - ordinary PR, mode docs: heavy + fast-lane + deps skip.
//!
//!    Any other skip (a drifted `if:`, a cancellation) or a job that ran where
//!    it should have skipped fails the aggregator.
//!
//! A regression here either lets a heavy job silently skip (red main that
//! looks green) or fails every PR.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, bail};

/// Jobs the aggregator does not gate: itself, and the job that runs after it.
const UNGATED: [&str; 2] = ["ci", "notify-main-red"];

/// One job of ci.yml, as far as the aggregator reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Job {
    pub(super) name: String,
    /// The job-level `if:`, verbatim.
    pub(super) condition: Option<String>,
    /// The job's `needs`.
    pub(super) needs: Vec<String>,
    /// A `HEAVY_JOBS: >-` folded scalar anywhere in the job, split on whitespace.
    pub(super) heavy_jobs: Option<Vec<String>>,
}

/// Leading spaces of `line`.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// `line` without a trailing YAML comment (` #...`), trimmed.
fn strip_comment(line: &str) -> &str {
    line.split_once(" #")
        .map_or(line, |(value, _)| value)
        .trim()
}

/// Reads the jobs under ci.yml's top-level `jobs:` map.
///
/// Not a general YAML parser: it reads the block style the workflow is written
/// in and refuses anything else at the positions it reads (a job key, `needs`),
/// so an unusual spelling fails loudly instead of hiding a job. Quoted keys and
/// trailing comments, the two valid spellings a naive line match misses, are
/// handled.
pub(super) fn parse_jobs(ci_yml: &str) -> anyhow::Result<Vec<Job>> {
    let lines: Vec<&str> = ci_yml.lines().collect();
    let start = lines
        .iter()
        .position(|l| strip_comment(l) == "jobs:")
        .context("ci.yml has no top-level `jobs:`")?;
    let mut jobs: Vec<Job> = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        let content = strip_comment(line);
        let lineno = i + 1;
        i += 1;
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        match indent(line) {
            0 => break, // the next top-level key: the jobs map ended
            2 => {
                let key = content
                    .strip_suffix(':')
                    .map(|k| k.trim_matches(|c| c == '"' || c == '\''))
                    .filter(|k| {
                        !k.is_empty()
                            && k.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    })
                    .with_context(|| {
                        format!(
                            "ci.yml:{lineno}: cannot read a job key from `{}`",
                            line.trim()
                        )
                    })?;
                jobs.push(Job {
                    name: key.to_owned(),
                    condition: None,
                    needs: Vec::new(),
                    heavy_jobs: None,
                });
            }
            n => {
                let Some(job) = jobs.last_mut() else {
                    bail!("ci.yml:{lineno}: indented line before the first job")
                };
                if n == 4 {
                    if let Some(cond) = content.strip_prefix("if:") {
                        job.condition = Some(cond.trim().to_owned());
                    } else if let Some(needs) = content.strip_prefix("needs:") {
                        let needs = needs.trim();
                        job.needs = match needs.strip_prefix('[').and_then(|n| n.strip_suffix(']'))
                        {
                            Some(list) => list
                                .split(',')
                                .map(|n| n.trim().to_owned())
                                .filter(|n| !n.is_empty())
                                .collect(),
                            None if !needs.is_empty() => vec![needs.to_owned()],
                            None => bail!(
                                "ci.yml:{lineno}: `needs:` of `{}` is a block list; write it as `[a, b]`",
                                job.name
                            ),
                        };
                    }
                }
                if content == "HEAVY_JOBS: >-" {
                    let mut names = Vec::new();
                    while i < lines.len() && (lines[i].trim().is_empty() || indent(lines[i]) > n) {
                        names.extend(lines[i].split_whitespace().map(str::to_owned));
                        i += 1;
                    }
                    job.heavy_jobs = Some(names);
                }
            }
        }
    }
    Ok(jobs)
}

/// What `plan` decided, as the aggregator receives it.
#[derive(Debug, Clone)]
pub(super) struct Plan<'a> {
    /// `plan`'s own result (`success`, `failure`, ...).
    pub(super) result: Option<&'a str>,
    pub(super) heavy: bool,
    pub(super) mode: &'a str,
    pub(super) cross_ios: bool,
}

/// Python-style repr of a string, as the aggregator's messages print it:
/// single-quoted unless it holds a `'` and no `"`.
fn repr(s: &str) -> String {
    let s = s.replace('\\', "\\\\");
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', "\\'"))
    }
}

/// The jobs allowed to skip under `plan`, or `None` when there is no usable plan.
fn expected_skips(plan: &Plan<'_>, heavy_jobs: &BTreeSet<String>) -> Option<BTreeSet<String>> {
    let set = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<BTreeSet<_>>();
    if plan.result != Some("success") {
        return None; // without a plan there is no expectation to check against
    }
    if plan.heavy {
        return Some(set(&["fast-lane", "fast-lane-ios"]));
    }
    let extra = match plan.mode {
        "packages" | "full" if plan.cross_ios => set(&[]),
        "packages" | "full" => set(&["fast-lane-ios"]),
        "none" => set(&["fast-lane", "fast-lane-ios"]),
        "docs" => set(&["fast-lane", "fast-lane-ios", "deps"]),
        _ => return None,
    };
    Some(heavy_jobs.union(&extra).cloned().collect())
}

/// Applies both rules to the jobs' results (`needs`: job -> result); returns
/// whether the run is green and the log the aggregator prints.
pub(super) fn verify(
    declared: &BTreeSet<String>,
    needs: &BTreeMap<String, String>,
    heavy_jobs: &BTreeSet<String>,
    plan: &Plan<'_>,
    event: &str,
) -> (bool, String) {
    let mut log = String::new();
    for (job, result) in needs {
        let _ = writeln!(log, "{job}: {result}");
    }
    let missing: Vec<String> = declared
        .iter()
        .filter(|j| !needs.contains_key(*j))
        .map(|j| repr(j))
        .collect();
    if !missing.is_empty() {
        let _ = writeln!(
            log,
            "::error::jobs declared in ci.yml but absent from the ci aggregator's needs: [{}]",
            missing.join(", ")
        );
    }
    let skipped: BTreeSet<String> = needs
        .iter()
        .filter(|(_, r)| *r == "skipped")
        .map(|(j, _)| j.clone())
        .collect();
    let mut bad: BTreeMap<String, String> = needs
        .iter()
        .filter(|(_, r)| !matches!(r.as_str(), "success" | "skipped"))
        .map(|(j, r)| (j.clone(), r.clone()))
        .collect();
    let py_bool = |b: bool| if b { "True" } else { "False" };
    match expected_skips(plan, heavy_jobs) {
        None => {
            let result = plan.result.map_or_else(|| "None".to_owned(), str::to_owned);
            bad.insert(
                "_plan".to_owned(),
                format!("no usable plan (result={result}, mode={})", repr(plan.mode)),
            );
        }
        Some(expected) => {
            for j in skipped.difference(&expected) {
                let why = format!(
                    "skipped, but this plan (heavy={} mode={}) requires it to run",
                    py_bool(plan.heavy),
                    plan.mode
                );
                bad.insert(j.clone(), why);
            }
            for j in expected.difference(&skipped) {
                if let Some(result) = needs.get(j) {
                    bad.insert(
                        j.clone(),
                        format!(
                            "ran ({result}), but this plan skips it: its if: disagrees with plan"
                        ),
                    );
                }
            }
        }
    }
    if !bad.is_empty() {
        let entries: Vec<String> = bad
            .iter()
            .map(|(j, why)| format!("{}: {}", repr(j), repr(why)))
            .collect();
        let _ = writeln!(
            log,
            "::error::jobs not as planned: {{{}}}",
            entries.join(", ")
        );
    }
    if !missing.is_empty() || !bad.is_empty() {
        return (false, log);
    }
    let _ = writeln!(
        log,
        "ok: {} jobs, all gated, {} skipped as planned (event={event} heavy={} mode={})",
        declared.len(),
        skipped.len(),
        py_bool(plan.heavy),
        plan.mode
    );
    (true, log)
}

/// The jobs the aggregator gates: every declared job but [`UNGATED`].
pub(super) fn gated_jobs(jobs: &[Job]) -> BTreeSet<String> {
    jobs.iter()
        .map(|j| j.name.clone())
        .filter(|n| !UNGATED.contains(&n.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Workflow {
        jobs: Vec<Job>,
        gated: BTreeSet<String>,
        heavy_jobs: BTreeSet<String>,
    }

    fn workflow() -> Workflow {
        let text = super::super::classify::read_normalised(
            &crate::util::repo_root().join(".github/workflows/ci.yml"),
        )
        .expect("ci.yml");
        let jobs = parse_jobs(&text).expect("ci.yml parses");
        let ci = jobs.iter().find(|j| j.name == "ci").expect("a `ci` job");
        let heavy_jobs = ci
            .heavy_jobs
            .clone()
            .expect("the ci job sets HEAVY_JOBS")
            .into_iter()
            .collect();
        let gated = gated_jobs(&jobs);
        Workflow {
            jobs,
            gated,
            heavy_jobs,
        }
    }

    /// Job -> result, for the jobs not at `success`.
    type Results = BTreeMap<String, &'static str>;

    /// Runs the rule with every gated job at `success` unless `results` says otherwise.
    fn aggregate(
        heavy: bool,
        mode: &str,
        results: &Results,
        event: &str,
        cross_ios: bool,
    ) -> (bool, String) {
        let w = workflow();
        let result_of = |j: &str| results.get(j).copied().unwrap_or("success");
        let needs = w
            .gated
            .iter()
            .map(|j| (j.clone(), result_of(j).to_owned()))
            .collect();
        let plan = Plan {
            result: Some(result_of("plan")),
            heavy,
            mode,
            cross_ios,
        };
        verify(&w.gated, &needs, &w.heavy_jobs, &plan, event)
    }

    fn results(entries: &[(&str, &'static str)]) -> Results {
        entries.iter().map(|&(j, r)| (j.to_owned(), r)).collect()
    }

    /// Every heavy job skipped, plus `extra`.
    fn skipped(extra: &[&str]) -> Results {
        let heavy = workflow().heavy_jobs;
        heavy
            .into_iter()
            .chain(extra.iter().map(|&j| j.to_owned()))
            .map(|j| (j, "skipped"))
            .collect()
    }

    fn with(mut base: Results, job: &str, result: &'static str) -> Results {
        base.insert(job.to_owned(), result);
        base
    }

    fn green(heavy: bool, mode: &str, results: &Results, event: &str, cross_ios: bool) {
        let (ok, log) = aggregate(heavy, mode, results, event, cross_ios);
        assert!(ok, "{log}");
    }

    fn red(heavy: bool, mode: &str, results: &Results, event: &str, cross_ios: bool, expect: &str) {
        let (ok, log) = aggregate(heavy, mode, results, event, cross_ios);
        assert!(!ok, "{log}");
        assert!(
            log.contains(expect),
            "{expect:?} not in:
{log}"
        );
    }

    #[test]
    fn heavy_jobs_list_matches_the_jobs_gated_on_heavy() {
        let w = workflow();
        let gated_on_heavy: BTreeSet<String> = w
            .jobs
            .iter()
            .filter(|j| j.condition.as_deref() == Some("needs.plan.outputs.heavy == 'true'"))
            .map(|j| j.name.clone())
            .collect();
        assert_eq!(gated_on_heavy, w.heavy_jobs);
    }

    #[test]
    fn the_aggregator_needs_every_gated_job() {
        let w = workflow();
        let ci = w.jobs.iter().find(|j| j.name == "ci").expect("a `ci` job");
        assert_eq!(ci.needs.iter().cloned().collect::<BTreeSet<_>>(), w.gated);
    }

    #[test]
    fn heavy_lane() {
        let fast = results(&[("fast-lane", "skipped"), ("fast-lane-ios", "skipped")]);
        green(true, "full", &fast, "push", false);
        red(
            true,
            "full",
            &with(fast.clone(), "miri", "skipped"),
            "push",
            false,
            "miri",
        );
        red(
            true,
            "full",
            &with(fast, "test", "failure"),
            "push",
            false,
            "test",
        );
        red(
            true,
            "full",
            &results(&[("fast-lane-ios", "skipped")]),
            "push",
            false,
            "fast-lane",
        );
    }

    #[test]
    fn fast_lane_packages_and_full() {
        for mode in ["packages", "full"] {
            green(
                false,
                mode,
                &skipped(&["fast-lane-ios"]),
                "pull_request",
                false,
            );
        }
        red(
            false,
            "packages",
            &with(skipped(&["fast-lane-ios"]), "fast-lane", "failure"),
            "pull_request",
            false,
            "fast-lane",
        );
        let mut ran_anyway = skipped(&["fast-lane-ios"]);
        ran_anyway.remove("doc");
        red(false, "packages", &ran_anyway, "pull_request", false, "doc");
    }

    #[test]
    fn ios_leg_follows_the_plan() {
        // in scope: it must run and pass
        green(false, "packages", &skipped(&[]), "pull_request", true);
        red(
            false,
            "packages",
            &skipped(&["fast-lane-ios"]),
            "pull_request",
            true,
            "fast-lane-ios",
        );
        red(
            false,
            "packages",
            &with(skipped(&[]), "fast-lane-ios", "failure"),
            "pull_request",
            true,
            "fast-lane-ios",
        );
        // out of scope: it must skip
        red(
            false,
            "packages",
            &skipped(&[]),
            "pull_request",
            false,
            "fast-lane-ios",
        );
    }

    #[test]
    fn fast_lane_tooling_and_docs() {
        green(
            false,
            "none",
            &skipped(&["fast-lane", "fast-lane-ios"]),
            "pull_request",
            false,
        );
        green(
            false,
            "docs",
            &skipped(&["fast-lane", "fast-lane-ios", "deps"]),
            "pull_request",
            false,
        );
        red(
            false,
            "docs",
            &skipped(&["fast-lane", "fast-lane-ios"]),
            "pull_request",
            false,
            "deps",
        );
    }

    /// `checks` is all a docs-only PR compiles, and it holds the markdown
    /// link check: it has no `if:`, and skipping it there is red.
    #[test]
    fn checks_runs_on_a_docs_only_pr() {
        let w = workflow();
        let checks = w
            .jobs
            .iter()
            .find(|j| j.name == "checks")
            .expect("a `checks` job");
        assert_eq!(checks.condition, None);
        red(
            false,
            "docs",
            &with(
                skipped(&["fast-lane", "fast-lane-ios", "deps"]),
                "checks",
                "skipped",
            ),
            "pull_request",
            false,
            // the whole finding: `checks` is the only job out of plan
            "jobs not as planned: {'checks': 'skipped, but this plan (heavy=False mode=docs) \
             requires it to run'}",
        );
    }

    #[test]
    fn failed_plan_is_red() {
        let (ok, log) = aggregate(
            false,
            "",
            &results(&[("plan", "failure")]),
            "pull_request",
            false,
        );
        assert!(!ok);
        assert!(
            log.contains("no usable plan (result=failure, mode='')"),
            "{log}"
        );
    }

    #[test]
    fn a_job_missing_from_needs_is_red() {
        let w = workflow();
        let mut needs: BTreeMap<String, String> = w
            .gated
            .iter()
            .map(|j| (j.clone(), "success".to_owned()))
            .collect();
        needs.remove("miri");
        let plan = Plan {
            result: Some("success"),
            heavy: true,
            mode: "full",
            cross_ios: false,
        };
        let (ok, log) = verify(&w.gated, &needs, &w.heavy_jobs, &plan, "push");
        assert!(!ok);
        assert!(
            log.contains("absent from the ci aggregator's needs: ['miri']"),
            "{log}"
        );
    }

    #[test]
    fn the_reader_handles_quoted_keys_comments_and_refuses_block_needs() {
        let text = "on: push\njobs:\n  \"quoted\":\n    if: always() # why\n    needs: [a, b]\n  plain: # comment\n    needs: a\n\
                    \x20   env:\n      HEAVY_JOBS: >-\n        x y\n        z\n    steps: []\nlater: 1\n";
        let jobs = parse_jobs(text).expect("parses");
        assert_eq!(
            jobs.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(),
            ["quoted", "plain"]
        );
        assert_eq!(jobs[0].condition.as_deref(), Some("always()"));
        assert_eq!(jobs[0].needs, ["a", "b"]);
        assert_eq!(jobs[1].needs, ["a"]);
        assert_eq!(
            jobs[1].heavy_jobs.as_deref(),
            Some(&["x".to_owned(), "y".to_owned(), "z".to_owned()][..])
        );
        let err = parse_jobs("jobs:\n  a:\n    needs:\n      - b\n").expect_err("block list");
        assert!(err.to_string().contains("block list"), "{err}");
        let err = parse_jobs("jobs:\n  {a: 1}\n").expect_err("flow map");
        assert!(err.to_string().contains("ci.yml:2"), "{err}");
    }
}
