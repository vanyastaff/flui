//! `cargo xtask deps`: the dependency gate.
//!
//! cargo-deny checks the resolved graph of every workspace member against
//! deny.toml (bans, licenses, sources) and against the RustSec database
//! (advisories). cargo-shear checks each manifest against the code: a
//! dependency no code uses, one only tests use declared as a normal one, a
//! `[workspace.dependencies]` entry no member inherits. It only warns when
//! the dependency is optional or a feature names it, and the gate passes
//! those. Nothing compiles.
//!
//! Every step runs even when an earlier one failed, so one run reports every
//! problem. CI runs the two [`Part`]s as separate steps so that the
//! advisories, which change with the RustSec database rather than with the
//! commit, can block only in the heavy lane.

use anyhow::bail;

use super::exec::{Cmd, Runner, Step, installed};

/// A part of the gate, split by what decides its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum Part {
    /// cargo-deny's bans, licenses and sources, and cargo-shear: the commit
    /// decides them, apart from the std-replacement list that
    /// `[bans.std-replacements]` fetches when it runs. A new entry there can
    /// fail a commit that passed, but only for a crate a member declares
    /// directly, and the fix is always this repository's to make (the std
    /// API, or an `ignore` with its reason in deny.toml), so it blocks with
    /// the rest.
    Policy,
    /// cargo-deny's advisories: the RustSec database as of the run, so a
    /// commit that passed can fail the next day with nothing changed, and the
    /// crate named may have no fixed release to move to.
    Advisories,
}

/// A cargo subcommand the gate runs, and the crate that installs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tool {
    /// `cargo <sub>`.
    pub(super) sub: &'static str,
    /// The crate that installs it.
    pub(super) install: &'static str,
}

const DENY: Tool = Tool {
    sub: "deny",
    install: "cargo-deny",
};

const SHEAR: Tool = Tool {
    sub: "shear",
    install: "cargo-shear",
};

/// Every tool the gate runs, once each, read off the full plan so that a
/// step cannot need a tool `cargo xtask doctor full` does not check.
pub(super) fn tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    for (tool, _) in plan(None, false) {
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    tools
}

/// `cargo deny` over every workspace member: the root manifest is the `flui`
/// package, not a virtual manifest, so without `--workspace` the graph starts
/// at the facade and misses flui-cli, the examples and the tools.
fn deny(checks: &[&str]) -> Cmd {
    Cmd::cargo(["deny", "--workspace", "--locked", "check"]).args(checks)
}

/// `cargo shear`, which fails on its errors (a dependency unused or used only
/// by tests, a `[workspace.dependencies]` entry nothing inherits) and only
/// reports its warnings, among them the same findings for an optional
/// dependency or one a feature names, and unlinked or empty files. `annotate`:
/// GitHub workflow commands, so each finding lands on the manifest line in
/// the pull request's diff.
fn shear(annotate: bool) -> Cmd {
    let cmd = Cmd::cargo(["shear", "--locked"]);
    if annotate {
        cmd.args(["--format", "github"])
    } else {
        cmd
    }
}

/// The commands of `only` (both parts when `None`), each with the tool it
/// needs.
fn plan(only: Option<Part>, annotate: bool) -> Vec<(Tool, Cmd)> {
    let mut plan = Vec::new();
    if only != Some(Part::Advisories) {
        plan.push((DENY, deny(&["bans", "licenses", "sources"])));
        plan.push((SHEAR, shear(annotate)));
    }
    if only != Some(Part::Policy) {
        plan.push((DENY, deny(&["advisories"])));
    }
    plan
}

/// `plan` as steps for a host where `present` says which tools are installed:
/// a missing tool's commands become one note saying so, or, under `strict`,
/// one entry of the returned list of missing tools.
fn steps(
    plan: Vec<(Tool, Cmd)>,
    strict: bool,
    mut present: impl FnMut(Tool) -> bool,
) -> (Vec<Step>, Vec<Tool>) {
    let (mut steps, mut missing, mut absent) = (Vec::new(), Vec::new(), Vec::new());
    for (tool, cmd) in plan {
        if absent.contains(&tool) {
            continue;
        }
        if present(tool) {
            steps.push(cmd.into());
            continue;
        }
        absent.push(tool);
        if strict {
            missing.push(tool);
        } else {
            steps.push(Step::Note(format!(
                "cargo {}: not installed, skipped (cargo install --locked {}; CI runs it)",
                tool.sub, tool.install
            )));
        }
    }
    (steps, missing)
}

/// Whether this process runs as a GitHub Actions step.
fn on_github_actions() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true")
}

/// Runs the gate; an error names every failed command and, under `strict`,
/// every tool that is not installed.
pub(super) fn run(runner: Runner, only: Option<Part>, strict: bool) -> anyhow::Result<()> {
    let (steps, missing) = steps(plan(only, on_github_actions()), strict, |tool| {
        runner.dry_run || installed("cargo", &[tool.sub, "--version"])
    });
    let ran = runner.every(&steps);
    if missing.is_empty() {
        return ran;
    }
    let names: Vec<String> = missing
        .iter()
        .map(|tool| {
            format!(
                "cargo {} (cargo install --locked {})",
                tool.sub, tool.install
            )
        })
        .collect();
    let not_installed = format!(
        "deps: not installed, and --strict makes that a failure: {}",
        names.join(", ")
    );
    match ran {
        Ok(()) => bail!("{not_installed}"),
        Err(error) => bail!("{error:#}\n{not_installed}"),
    }
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum as _;

    use super::*;

    fn lines(plan: &[(Tool, Cmd)]) -> Vec<String> {
        plan.iter().map(|(_, cmd)| cmd.to_string()).collect()
    }

    const POLICY: [&str; 2] = [
        "cargo deny --workspace --locked check bans licenses sources",
        "cargo shear --locked",
    ];
    const ADVISORIES: &str = "cargo deny --workspace --locked check advisories";

    #[test]
    fn the_parts_are_the_ci_steps_and_all_runs_both() {
        assert_eq!(lines(&plan(Some(Part::Policy), false)), POLICY);
        assert_eq!(lines(&plan(Some(Part::Advisories), false)), [ADVISORIES]);
        assert_eq!(
            lines(&plan(None, false)),
            [POLICY[0], POLICY[1], ADVISORIES]
        );
        assert_eq!(Part::from_str("policy", false), Ok(Part::Policy));
        assert_eq!(Part::from_str("advisories", false), Ok(Part::Advisories));
    }

    #[test]
    fn ci_lets_only_the_advisories_pass_outside_the_heavy_lane() {
        let ci = std::fs::read_to_string(crate::util::repo_root().join(".github/workflows/ci.yml"))
            .expect("ci.yml")
            .replace("\r\n", "\n");
        let job = ci.split_once("\n  deps:\n").expect("a `deps` job").1;
        let next_job = regex::Regex::new(r"\n  [a-z0-9-]+:\n").expect("BUG: valid regex");
        let job = &job[..next_job.find(job).map_or(job.len(), |m| m.start())];
        // The step that runs `--only <part>`: from its `- name:` to its `run:`.
        let step = |part: &str| {
            let run = format!("run: cargo xtask deps --strict --only {part}\n");
            let at = job
                .find(&run)
                .unwrap_or_else(|| panic!("the deps job has no `{run}`"));
            job[..at].rsplit_once("- name:").expect("a named step").1
        };
        assert!(!step("policy").contains("continue-on-error"));
        assert!(
            step("advisories")
                .contains("continue-on-error: ${{ needs.plan.outputs.heavy != 'true' }}"),
            "{}",
            step("advisories")
        );
    }

    #[test]
    fn shear_annotates_the_diff_on_github_actions() {
        assert_eq!(
            lines(&plan(Some(Part::Policy), true))[1],
            "cargo shear --locked --format github"
        );
    }

    #[test]
    fn tools_names_each_tool_of_the_plan_once() {
        assert_eq!(tools(), [DENY, SHEAR]);
    }

    #[test]
    fn a_missing_tool_is_a_note_or_under_strict_a_failure() {
        let only_shear = |tool: Tool| tool == SHEAR;
        let (skipped, missing) = steps(plan(None, false), false, only_shear);
        assert!(missing.is_empty());
        assert_eq!(
            skipped.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [
                "cargo deny: not installed, skipped (cargo install --locked cargo-deny; CI runs it)",
                "$ cargo shear --locked",
            ]
        );
        let (strict, missing) = steps(plan(None, false), true, only_shear);
        assert_eq!(missing, [DENY]);
        assert_eq!(
            strict.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["$ cargo shear --locked"]
        );
        let (all, missing) = steps(plan(None, false), true, |_| true);
        assert!(missing.is_empty());
        assert_eq!(all.len(), 3);
    }
}
