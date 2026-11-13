//! `cargo xtask deps`: the dependency gate.
//!
//! cargo-deny checks the resolved graph of every workspace member against
//! deny.toml (bans, licenses, sources) and against the RustSec database
//! (advisories). Nothing compiles. A dependency no code uses, one only tests
//! use declared as a normal one and a `[workspace.dependencies]` entry no
//! member inherits are cargo's own `unused_dependencies` lint, which every
//! CI build reports and `CARGO_BUILD_WARNINGS=deny` makes fatal, per feature
//! and per target the build compiles.
//!
//! Every step runs even when an earlier one failed, so one run reports every
//! problem. CI runs the two [`Part`]s as separate steps so that the
//! advisories, which change with the RustSec database rather than with the
//! commit, can block only in the `wide` and `full` lanes (advisories.yml runs
//! that step daily against main).

use anyhow::bail;

use super::exec::{Cmd, Runner, Step, installed};

/// A part of the gate, split by what decides its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum Part {
    /// cargo-deny's bans, licenses and sources: the commit decides them, apart from the std-replacement list that
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

/// Every tool the gate runs, once each, read off the full plan so that a
/// step cannot need a tool `cargo xtask doctor full` does not check.
pub(super) fn tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    for (tool, _) in plan(None) {
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

/// The commands of `only` (both parts when `None`), each with the tool it
/// needs.
fn plan(only: Option<Part>) -> Vec<(Tool, Cmd)> {
    let mut plan = Vec::new();
    if only != Some(Part::Advisories) {
        plan.push((DENY, deny(&["bans", "licenses", "sources"])));
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

/// Runs the gate; an error names every failed command and, under `strict`,
/// every tool that is not installed.
pub(super) fn run(runner: Runner, only: Option<Part>, strict: bool) -> anyhow::Result<()> {
    let (steps, missing) = steps(plan(only), strict, |tool| {
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
    use super::*;

    fn ci_lets_only_the_advisories_pass_on_a_package_or_tooling_pr() {
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
                .contains("continue-on-error: ${{ needs.plan.outputs.lane == 'tooling' || (needs.plan.outputs.lane == 'wide' && needs.plan.outputs.mode == 'packages' && needs.plan.outputs.heavy_required == 'false') }}"),
            "{}",
            step("advisories")
        );
    }

    fn a_missing_tool_is_a_note_or_under_strict_a_failure() {
        // Both parts need cargo-deny; a missing tool is reported once.
        let (skipped, missing) = steps(plan(None), false, |_| false);
        assert_eq!(missing, [] as [Tool; 0]);
        assert_eq!(
            skipped.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["cargo deny: not installed, skipped (cargo install --locked cargo-deny; CI runs it)"]
        );
        let (strict, missing) = steps(plan(None), true, |_| false);
        assert_eq!(missing, [DENY]);
        assert_eq!(strict.len(), 0);
        let (all, missing) = steps(plan(None), true, |_| true);
        assert_eq!(missing, [] as [Tool; 0]);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn deps_gate_contract() {
        crate::table_test::run_table(
            "deps_gate_contract",
            &[
                (
                    "ci_lets_only_the_advisories_pass_on_a_package_or_tooling_pr",
                    ci_lets_only_the_advisories_pass_on_a_package_or_tooling_pr as fn(),
                ),
                (
                    "a_missing_tool_is_a_note_or_under_strict_a_failure",
                    a_missing_tool_is_a_note_or_under_strict_a_failure as fn(),
                ),
            ],
        );
    }
}
