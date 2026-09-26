//! The `ci` aggregator's rule (ci.yml, job `ci`), and a reader for the parts of
//! ci.yml it depends on.
//!
//! Two rules:
//!
//! 1. COMPLETENESS -- every job in the workflow except `ci` itself (and
//!    `notify-main-red`, which runs after it) must appear in `ci`'s `needs`.
//!    Without this, a job added without editing that list ran, passed or
//!    failed, and gated nothing.
//! 2. EXACTLY THE EXPECTED SKIPS -- which jobs run is decided by `plan`'s
//!    `lane` alone, and the aggregator recomputes it instead of trusting each
//!    job's `if:`. Every lane runs `checks` and `plan`; then:
//!    - `docs`: nothing else;
//!    - `tooling`: `deps`, and `standalone` when plan names a standalone crate;
//!    - `fast`: `deps`, `fast-lane`, and `fast-lane-ios` when plan puts the
//!      iOS runner in scope (`cross_ios`);
//!    - `wide`: `deps` and the `HEAVY_JOBS` list (every Linux job);
//!    - `full`: `wide` plus `FULL_JOBS` (the Windows and macOS jobs);
//!    - `extended`: `full` plus `EXTENDED_JOBS`.
//!
//!    Every other declared job must skip. Any other skip (a drifted `if:`, a
//!    cancellation) or a job that ran where it should have skipped fails the
//!    aggregator; so does a job no lane runs, because it can only skip.
//!
//! 3. THE EVENT'S FLOOR -- a run on `main` (`push`, `merge_group`) must take
//!    `full` or `extended`, a nightly or manual run (`schedule`,
//!    `workflow_dispatch`) must take `extended`. Rule 2 trusts `plan`'s lane;
//!    this rule checks it against the event outside `Lane::decide`, so a
//!    regression that maps `push` to a narrow lane cannot skip every heavy
//!    job with a green `ci` on `main`.
//!
//! A regression here either lets a job silently skip (red main that looks
//! green) or fails every PR.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, bail};

/// Jobs the aggregator does not gate: itself, and the job that runs after it.
const UNGATED: [&str; 2] = ["ci", "notify-main-red"];

/// The `if:` of the jobs the `wide`, `full` and `extended` lanes run (`HEAVY_JOBS`).
pub(super) const WIDE_CONDITION: &str =
    r#"contains(fromJSON('["wide","full","extended"]'), needs.plan.outputs.lane)"#;
/// The `if:` of the jobs only `full` and `extended` add (`FULL_JOBS`). Only
/// the tests read it: they pin `FULL_JOBS` to the jobs it gates.
#[cfg(test)]
pub(super) const FULL_CONDITION: &str =
    r#"contains(fromJSON('["full","extended"]'), needs.plan.outputs.lane)"#;
/// The `if:` of the jobs only `extended` adds (`EXTENDED_JOBS`); read by the
/// tests, like [`FULL_CONDITION`].
#[cfg(test)]
pub(super) const EXTENDED_CONDITION: &str = "needs.plan.outputs.lane == 'extended'";

/// One job of ci.yml, as far as the aggregator reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Job {
    pub(super) name: String,
    /// The job-level `if:`, verbatim.
    pub(super) condition: Option<String>,
    /// The job's `needs`.
    pub(super) needs: Vec<String>,
    /// Each `<NAME>_JOBS: >-` folded scalar anywhere in the job, split on
    /// whitespace, by name.
    pub(super) lists: BTreeMap<String, Vec<String>>,
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

/// The `NAME_JOBS` of a `NAME_JOBS: >-` line.
fn job_list_key(content: &str) -> Option<&str> {
    let key = content.strip_suffix(": >-")?;
    (key.ends_with("_JOBS") && key.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .then_some(key)
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
                    lists: BTreeMap::new(),
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
                if let Some(key) = job_list_key(content) {
                    let mut names = Vec::new();
                    while i < lines.len() && (lines[i].trim().is_empty() || indent(lines[i]) > n) {
                        names.extend(lines[i].split_whitespace().map(str::to_owned));
                        i += 1;
                    }
                    job.lists.insert(key.to_owned(), names);
                }
            }
        }
    }
    Ok(jobs)
}

/// The jobs each lane adds, as the `ci` job lists them.
#[derive(Debug, Clone, Default)]
pub(super) struct LaneJobs {
    /// `HEAVY_JOBS`: what `wide` (and so `full` and `extended`) runs.
    pub(super) wide: BTreeSet<String>,
    /// `FULL_JOBS`: what `full` (and so `extended`) adds.
    pub(super) full: BTreeSet<String>,
    /// `EXTENDED_JOBS`: what only `extended` adds.
    pub(super) extended: BTreeSet<String>,
}

/// What `plan` decided, as the aggregator receives it.
#[derive(Debug, Clone)]
pub(super) struct Plan<'a> {
    /// `plan`'s own result (`success`, `failure`, ...).
    pub(super) result: Option<&'a str>,
    /// `docs`, `tooling`, `fast`, `wide`, `full` or `extended`.
    pub(super) lane: &'a str,
    pub(super) cross_ios: bool,
    /// Whether plan names a standalone crate for the `standalone` job.
    pub(super) standalone: bool,
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

/// The lanes a run of `event` may take, or `None` when any lane may (a pull
/// request narrows by its diff; an event ci.yml does not trigger on fails in
/// `plan` before it reaches this rule).
fn lanes_allowed(event: &str) -> Option<&'static [&'static str]> {
    match event {
        "push" | "merge_group" => Some(&["full", "extended"]),
        "schedule" | "workflow_dispatch" => Some(&["extended"]),
        _ => None,
    }
}

/// The jobs `plan`'s lane runs, or `None` when there is no usable plan.
fn planned_runs(plan: &Plan<'_>, lanes: &LaneJobs) -> Option<BTreeSet<String>> {
    if plan.result != Some("success") {
        return None; // without a plan there is no expectation to check against
    }
    let mut runs: BTreeSet<String> = ["checks", "plan"].map(str::to_owned).into();
    let mut add = |names: &[&str]| runs.extend(names.iter().map(|&n| n.to_owned()));
    match plan.lane {
        "docs" => {}
        "tooling" if plan.standalone => add(&["deps", "standalone"]),
        "tooling" => add(&["deps"]),
        "fast" if plan.cross_ios => add(&["deps", "fast-lane", "fast-lane-ios"]),
        "fast" => add(&["deps", "fast-lane"]),
        "wide" | "full" | "extended" => {
            add(&["deps"]);
            runs.extend(lanes.wide.iter().cloned());
            if plan.lane != "wide" {
                runs.extend(lanes.full.iter().cloned());
            }
            if plan.lane == "extended" {
                runs.extend(lanes.extended.iter().cloned());
            }
        }
        _ => return None,
    }
    Some(runs)
}

/// Applies both rules to the jobs' results (`needs`: job -> result); returns
/// whether the run is green and the log the aggregator prints.
pub(super) fn verify(
    declared: &BTreeSet<String>,
    needs: &BTreeMap<String, String>,
    lanes: &LaneJobs,
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
    if let Some(allowed) = lanes_allowed(event)
        && !allowed.contains(&plan.lane)
    {
        bad.insert(
            "_plan".to_owned(),
            format!(
                "event={event} must take lane {}, but plan chose lane={}",
                allowed.join(" or "),
                repr(plan.lane)
            ),
        );
    }
    match planned_runs(plan, lanes) {
        None => {
            let result = plan.result.map_or_else(|| "None".to_owned(), str::to_owned);
            bad.insert(
                "_plan".to_owned(),
                format!("no usable plan (result={result}, lane={})", repr(plan.lane)),
            );
        }
        Some(runs) => {
            for j in skipped.intersection(&runs) {
                let why = format!(
                    "skipped, but this plan (lane={}) requires it to run",
                    plan.lane
                );
                bad.insert(j.clone(), why);
            }
            for (j, result) in needs {
                if !runs.contains(j) && result != "skipped" {
                    bad.insert(
                        j.clone(),
                        format!(
                            "ran ({result}), but this plan (lane={}) skips it: its if: disagrees with plan",
                            plan.lane
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
        "ok: {} jobs, all gated, {} skipped as planned (event={event} lane={})",
        declared.len(),
        skipped.len(),
        plan.lane
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
        lanes: LaneJobs,
    }

    fn workflow() -> Workflow {
        let text = super::super::classify::read_normalised(
            &crate::util::repo_root().join(".github/workflows/ci.yml"),
        )
        .expect("ci.yml");
        let jobs = parse_jobs(&text).expect("ci.yml parses");
        let ci = jobs.iter().find(|j| j.name == "ci").expect("a `ci` job");
        let list = |name: &str| -> BTreeSet<String> {
            ci.lists
                .get(name)
                .unwrap_or_else(|| panic!("the ci job sets {name}"))
                .iter()
                .cloned()
                .collect()
        };
        let lanes = LaneJobs {
            wide: list("HEAVY_JOBS"),
            full: list("FULL_JOBS"),
            extended: list("EXTENDED_JOBS"),
        };
        let gated = gated_jobs(&jobs);
        Workflow { jobs, gated, lanes }
    }

    /// Job -> result, for the jobs not at `success`.
    type Results = BTreeMap<String, &'static str>;

    /// What a run looks like: its lane, plan's two flags, and the results.
    struct Run<'a> {
        lane: &'a str,
        cross_ios: bool,
        standalone: bool,
        event: &'a str,
    }

    const PR: &str = "pull_request";

    fn run(lane: &str) -> Run<'_> {
        Run {
            lane,
            cross_ios: false,
            standalone: false,
            event: PR,
        }
    }

    /// Runs the rule with every gated job at `success` unless `results` says otherwise.
    fn aggregate(r: &Run<'_>, results: &Results) -> (bool, String) {
        let w = workflow();
        let result_of = |j: &str| results.get(j).copied().unwrap_or("success");
        let needs = w
            .gated
            .iter()
            .map(|j| (j.clone(), result_of(j).to_owned()))
            .collect();
        let plan = Plan {
            result: Some(result_of("plan")),
            lane: r.lane,
            cross_ios: r.cross_ios,
            standalone: r.standalone,
        };
        verify(&w.gated, &needs, &w.lanes, &plan, r.event)
    }

    /// These jobs skipped, the rest succeeded.
    fn skipping<'a>(jobs: impl IntoIterator<Item = &'a String>) -> Results {
        jobs.into_iter().map(|j| (j.clone(), "skipped")).collect()
    }

    fn names(jobs: &[&str]) -> Vec<String> {
        jobs.iter().map(|&j| j.to_owned()).collect()
    }

    fn with(mut base: Results, job: &str, result: &'static str) -> Results {
        base.insert(job.to_owned(), result);
        base
    }

    fn without(mut base: Results, job: &str) -> Results {
        base.remove(job);
        base
    }

    fn green(r: &Run<'_>, results: &Results) {
        let (ok, log) = aggregate(r, results);
        assert!(ok, "{log}");
    }

    fn red(r: &Run<'_>, results: &Results, expect: &str) {
        let (ok, log) = aggregate(r, results);
        assert!(!ok, "{log}");
        assert!(log.contains(expect), "{expect:?} not in:\n{log}");
    }

    /// The jobs whose `if:` is exactly `condition`.
    fn gated_on(w: &Workflow, condition: &str) -> BTreeSet<String> {
        w.jobs
            .iter()
            .filter(|j| j.condition.as_deref() == Some(condition))
            .map(|j| j.name.clone())
            .collect()
    }

    /// The fast-lane jobs and `standalone`, which no whole-workspace lane runs.
    fn not_whole_workspace() -> Vec<String> {
        names(&["fast-lane", "fast-lane-ios", "standalone"])
    }

    #[test]
    fn heavy_jobs_list_matches_the_jobs_gated_on_heavy() {
        let w = workflow();
        assert_eq!(gated_on(&w, WIDE_CONDITION), w.lanes.wide);
    }

    #[test]
    fn lane_lists_match_the_job_conditions() {
        let w = workflow();
        assert_eq!(gated_on(&w, FULL_CONDITION), w.lanes.full);
        assert_eq!(gated_on(&w, EXTENDED_CONDITION), w.lanes.extended);
        assert!(w.lanes.wide.is_disjoint(&w.lanes.full));
        assert!(w.lanes.wide.is_disjoint(&w.lanes.extended));
        assert!(w.lanes.full.is_disjoint(&w.lanes.extended));
        assert!(
            !w.lanes.wide.is_empty() && !w.lanes.full.is_empty() && !w.lanes.extended.is_empty()
        );
    }

    #[test]
    fn every_gated_job_uses_a_known_condition() {
        let w = workflow();
        let known = [
            WIDE_CONDITION,
            FULL_CONDITION,
            EXTENDED_CONDITION,
            "needs.plan.outputs.lane == 'fast'",
            "needs.plan.outputs.lane == 'fast' && needs.plan.outputs.cross_ios == 'true'",
            "needs.plan.outputs.lane == 'tooling' && needs.plan.outputs.standalone != ''",
            "needs.plan.outputs.lane != 'docs'",
        ];
        for job in w.jobs.iter().filter(|j| w.gated.contains(&j.name)) {
            match job.condition.as_deref() {
                None => assert!(
                    ["checks", "plan"].contains(&job.name.as_str()),
                    "{} has no if:",
                    job.name
                ),
                Some(condition) => assert!(
                    known.contains(&condition),
                    "{}: `if: {condition}` is not a lane condition the aggregator knows",
                    job.name
                ),
            }
        }
    }

    #[test]
    fn the_aggregator_needs_every_gated_job() {
        let w = workflow();
        let ci = w.jobs.iter().find(|j| j.name == "ci").expect("a `ci` job");
        assert_eq!(ci.needs.iter().cloned().collect::<BTreeSet<_>>(), w.gated);
    }

    #[test]
    fn wide_lane_skips_platform_jobs() {
        let w = workflow();
        let skipped: Vec<String> = not_whole_workspace()
            .into_iter()
            .chain(w.lanes.full.iter().cloned())
            .chain(w.lanes.extended.iter().cloned())
            .collect();
        let wide = skipping(&skipped);
        assert_eq!(wide.get("gpu-test"), Some(&"skipped"));
        green(&run("wide"), &wide);
        red(
            &run("wide"),
            &with(wide.clone(), "clippy", "skipped"),
            "clippy",
        );
        red(&run("wide"), &without(wide, "gpu-test"), "gpu-test");
    }

    #[test]
    fn full_lane() {
        let w = workflow();
        let skipped: Vec<String> = not_whole_workspace()
            .into_iter()
            .chain(w.lanes.extended.iter().cloned())
            .collect();
        let main = Run {
            event: "push",
            ..run("full")
        };
        let full = skipping(&skipped);
        green(&main, &full);
        red(&main, &with(full.clone(), "miri", "skipped"), "miri");
        red(
            &main,
            &with(full.clone(), "gpu-test", "skipped"),
            "gpu-test",
        );
        red(&main, &with(full.clone(), "test", "failure"), "test");
        red(&main, &without(full, "fast-lane"), "fast-lane");
    }

    #[test]
    fn extended_jobs_skipped_on_main_is_green_and_on_schedule_is_red() {
        let w = workflow();
        assert!(w.lanes.extended.contains("macos-ci"));
        let main_push = skipping(
            &not_whole_workspace()
                .into_iter()
                .chain(w.lanes.extended.iter().cloned())
                .collect::<Vec<_>>(),
        );
        let push = Run {
            event: "push",
            ..run("full")
        };
        green(&push, &main_push);
        let nightly = Run {
            event: "schedule",
            ..run("extended")
        };
        red(&nightly, &main_push, "macos-ci");
        red(&nightly, &main_push, "test-windows");
        green(&nightly, &skipping(&not_whole_workspace()));
        // an extended job that ran on main disagrees with its `if:`
        red(&push, &without(main_push, "macos-ci"), "macos-ci");
    }

    #[test]
    fn fast_lane() {
        let w = workflow();
        let whole: Vec<String> = w
            .lanes
            .wide
            .iter()
            .chain(&w.lanes.full)
            .chain(&w.lanes.extended)
            .cloned()
            .chain(names(&["fast-lane-ios", "standalone"]))
            .collect();
        let fast = skipping(&whole);
        green(&run("fast"), &fast);
        red(
            &run("fast"),
            &with(fast.clone(), "fast-lane", "failure"),
            "fast-lane",
        );
        red(&run("fast"), &with(fast.clone(), "deps", "skipped"), "deps");
        red(&run("fast"), &without(fast, "doc"), "doc");
    }

    #[test]
    fn ios_leg_follows_the_plan() {
        let w = workflow();
        let whole: Vec<String> = w
            .lanes
            .wide
            .iter()
            .chain(&w.lanes.full)
            .chain(&w.lanes.extended)
            .cloned()
            .chain(names(&["standalone"]))
            .collect();
        let ios = Run {
            cross_ios: true,
            ..run("fast")
        };
        // in scope: it must run and pass
        green(&ios, &skipping(&whole));
        red(
            &ios,
            &with(skipping(&whole), "fast-lane-ios", "skipped"),
            "fast-lane-ios",
        );
        red(
            &ios,
            &with(skipping(&whole), "fast-lane-ios", "failure"),
            "fast-lane-ios",
        );
        // out of scope: it must skip
        red(&run("fast"), &skipping(&whole), "fast-lane-ios");
    }

    #[test]
    fn tooling_and_docs_lanes() {
        let w = workflow();
        let compiling: Vec<String> = w
            .lanes
            .wide
            .iter()
            .chain(&w.lanes.full)
            .chain(&w.lanes.extended)
            .cloned()
            .chain(names(&["fast-lane", "fast-lane-ios"]))
            .collect();
        let tooling = skipping(compiling.iter().chain(&names(&["standalone"])));
        green(&run("tooling"), &tooling);
        red(
            &run("tooling"),
            &with(tooling.clone(), "deps", "skipped"),
            "deps",
        );
        // a standalone crate changed: its job must run
        let standalone = Run {
            standalone: true,
            ..run("tooling")
        };
        red(&standalone, &tooling, "standalone");
        green(&standalone, &without(tooling.clone(), "standalone"));
        red(
            &run("tooling"),
            &without(tooling, "standalone"),
            "standalone",
        );

        let docs = skipping(compiling.iter().chain(&names(&["standalone", "deps"])));
        green(&run("docs"), &docs);
        red(&run("docs"), &without(docs, "deps"), "deps");
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
        let everything_but_checks: Vec<String> = w
            .gated
            .iter()
            .filter(|j| !["checks", "plan"].contains(&j.as_str()))
            .cloned()
            .collect();
        red(
            &run("docs"),
            &with(skipping(&everything_but_checks), "checks", "skipped"),
            // the whole finding: `checks` is the only job out of plan
            "jobs not as planned: {'checks': 'skipped, but this plan (lane=docs) requires it to run'}",
        );
    }

    #[test]
    fn failed_plan_is_red() {
        let (ok, log) = aggregate(&run(""), &skipping(&names(&["plan"])));
        assert!(!ok);
        let (ok, log2) = aggregate(&run(""), &with(Results::new(), "plan", "failure"));
        assert!(!ok);
        assert!(
            log2.contains("no usable plan (result=failure, lane='')"),
            "{log2}"
        );
        assert!(log.contains("no usable plan (result=skipped"), "{log}");
        // a lane this rule does not know is no plan either
        let (ok, log) = aggregate(&run("heavy"), &Results::new());
        assert!(!ok && log.contains("lane='heavy'"), "{log}");
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
            lane: "full",
            cross_ios: false,
            standalone: false,
        };
        let (ok, log) = verify(&w.gated, &needs, &w.lanes, &plan, "push");
        assert!(!ok);
        assert!(
            log.contains("absent from the ci aggregator's needs: ['miri']"),
            "{log}"
        );
    }

    #[test]
    fn a_job_no_lane_runs_is_red() {
        // a job added with a new `if:` but in no lane list can only skip,
        // which every lane reports
        let w = workflow();
        let mut gated = w.gated.clone();
        gated.insert("new-job".to_owned());
        let needs: BTreeMap<String, String> = gated
            .iter()
            .map(|j| (j.clone(), "success".to_owned()))
            .collect();
        let plan = Plan {
            result: Some("success"),
            lane: "extended",
            cross_ios: false,
            standalone: false,
        };
        let (ok, log) = verify(&gated, &needs, &w.lanes, &plan, "schedule");
        assert!(!ok && log.contains("'new-job': 'ran (success)"), "{log}");
    }

    #[test]
    fn a_narrow_lane_on_main_or_nightly_is_red() {
        // every job the narrow lane skips did skip, so rule 2 alone is green:
        // only the event's floor catches a plan that under-ran main
        let w = workflow();
        let whole: Vec<String> = w
            .lanes
            .wide
            .iter()
            .chain(&w.lanes.full)
            .chain(&w.lanes.extended)
            .cloned()
            .chain(names(&["fast-lane-ios", "standalone"]))
            .collect();
        let fast = skipping(&whole);
        green(&run("fast"), &fast);
        for event in ["push", "merge_group"] {
            red(
                &Run {
                    event,
                    ..run("fast")
                },
                &fast,
                &format!(
                    "event={event} must take lane full or extended, but plan chose lane='fast'"
                ),
            );
        }
        let wide = skipping(
            &not_whole_workspace()
                .into_iter()
                .chain(w.lanes.full.iter().cloned())
                .chain(w.lanes.extended.iter().cloned())
                .collect::<Vec<_>>(),
        );
        red(
            &Run {
                event: "push",
                ..run("wide")
            },
            &wide,
            "lane='wide'",
        );
        let full = skipping(
            &not_whole_workspace()
                .into_iter()
                .chain(w.lanes.extended.iter().cloned())
                .collect::<Vec<_>>(),
        );
        green(
            &Run {
                event: "merge_group",
                ..run("full")
            },
            &full,
        );
        for event in ["schedule", "workflow_dispatch"] {
            red(
                &Run {
                    event,
                    ..run("full")
                },
                &full,
                &format!("event={event} must take lane extended, but plan chose lane='full'"),
            );
        }
    }

    #[test]
    fn the_reader_handles_quoted_keys_comments_and_refuses_block_needs() {
        let text = "on: push\njobs:\n  \"quoted\":\n    if: always() # why\n    needs: [a, b]\n  plain: # comment\n    needs: a\n\
                    \x20   env:\n      HEAVY_JOBS: >-\n        x y\n        z\n      FULL_JOBS: >-\n        w\n\
                    \x20     NOT_A_LIST: >-\n        v\n    steps: []\nlater: 1\n";
        let jobs = parse_jobs(text).expect("parses");
        assert_eq!(
            jobs.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(),
            ["quoted", "plain"]
        );
        assert_eq!(jobs[0].condition.as_deref(), Some("always()"));
        assert_eq!(jobs[0].needs, ["a", "b"]);
        assert_eq!(jobs[1].needs, ["a"]);
        assert_eq!(
            jobs[1].lists.get("HEAVY_JOBS").map(Vec::as_slice),
            Some(&["x".to_owned(), "y".to_owned(), "z".to_owned()][..])
        );
        assert_eq!(
            jobs[1].lists.get("FULL_JOBS").map(Vec::as_slice),
            Some(&["w".to_owned()][..])
        );
        assert_eq!(jobs[1].lists.len(), 2, "{:?}", jobs[1].lists);
        let err = parse_jobs("jobs:\n  a:\n    needs:\n      - b\n").expect_err("block list");
        assert!(err.to_string().contains("block list"), "{err}");
        let err = parse_jobs("jobs:\n  {a: 1}\n").expect_err("flow map");
        assert!(err.to_string().contains("ci.yml:2"), "{err}");
    }
}
