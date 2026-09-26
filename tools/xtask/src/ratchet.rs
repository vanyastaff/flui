//! What the allowlists of the source gates (`markers`, `file-length`) have in
//! common: each entry names the change that removes it, and that name must
//! point at something that exists.
//!
//! An exit is either an ADR (`ADR-0081`, with a file under `docs/adr/`) or a
//! step of the architecture migration plan (a step id such as `W1-A5`, with a
//! table row that starts with it in [`PLAN`]). A step id is otherwise a
//! process marker; as an exit it is a checked reference, which is why the
//! `markers` scan reads the allowlists' `exit` values as references, not prose.
//!
//! Both allowlists count exactly, so a count only goes down: an entry above
//! the tree is a finding ("grew"), and so is one below it ("lower it"). What
//! the gates cannot see is an entry added or raised in the same change as the
//! code it excuses; that shows up only in the diff of the allowlist file.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Context;
use regex::Regex;

/// The living migration plan whose table rows name the steps an exit may cite.
pub(crate) const PLAN: &str = "docs/plans/2026-09-25-architecture-migration-plan.md";

/// The ADR directory an `ADR-NNNN` exit must have a file in.
const ADR_DIR: &str = "docs/adr";

static ADR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ADR-(\d{4})$").expect("BUG: static regex"));
static STEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^W\d+-[ABP]\d+[a-z]?$").expect("BUG: static regex"));

/// The exits that exist: ADR numbers and plan step ids.
#[derive(Debug, Default)]
pub(crate) struct Exits {
    adrs: BTreeSet<String>,
    steps: BTreeSet<String>,
}

impl Exits {
    /// The ADRs under `docs/adr/` and the steps in [`PLAN`] of the checkout at `root`.
    pub(crate) fn from_repo(root: &Path) -> anyhow::Result<Self> {
        let dir = root.join(ADR_DIR);
        let mut adrs = Vec::new();
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))?
        {
            let name = entry?.file_name();
            if let Some(number) = name
                .to_str()
                .and_then(|name| name.strip_prefix("ADR-"))
                .and_then(|rest| rest.get(..4))
            {
                adrs.push(number.to_owned());
            }
        }
        let plan =
            std::fs::read_to_string(root.join(PLAN)).with_context(|| format!("reading {PLAN}"))?;
        Ok(Self::from_parts(adrs, &plan))
    }

    /// `adrs` (four-digit numbers) and the step ids that begin a table row of `plan`.
    pub(crate) fn from_parts(adrs: impl IntoIterator<Item = String>, plan: &str) -> Self {
        let steps = plan
            .lines()
            .filter_map(|line| line.strip_prefix("| ")?.split_once(" |"))
            .map(|(cell, _)| cell.trim())
            .filter(|cell| STEP.is_match(cell))
            .map(str::to_owned)
            .collect();
        Self {
            adrs: adrs.into_iter().collect(),
            steps,
        }
    }

    /// `Err` with the reason when `exit` names nothing that exists.
    pub(crate) fn check(&self, exit: &str) -> Result<(), String> {
        if let Some(number) = ADR.captures(exit).map(|c| c[1].to_owned()) {
            return if self.adrs.contains(&number) {
                Ok(())
            } else {
                Err(format!("no {ADR_DIR}/ADR-{number}-*.md"))
            };
        }
        if STEP.is_match(exit) {
            return if self.steps.contains(exit) {
                Ok(())
            } else {
                Err(format!("no table row for it in {PLAN}"))
            };
        }
        if exit.is_empty() {
            return Err("empty; name the ADR or plan step that removes the entry".to_owned());
        }
        Err("neither an ADR (ADR-NNNN) nor a migration-plan step".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN_FIXTURE: &str = include_str!("../fixtures/ratchet/plan.md.txt");
    const CASES: &str = include_str!("../fixtures/ratchet/exits.txt");

    #[test]
    fn exit_names_an_adr_or_a_plan_step() {
        let exits = Exits::from_parts(["0081".to_owned()], PLAN_FIXTURE);
        let mut checked = 0;
        for line in CASES.lines().filter(|line| !line.trim().is_empty()) {
            let (verdict, exit) = line.split_once(' ').unwrap_or((line, ""));
            let exit = exit.trim_matches('"');
            match verdict {
                "accept" => assert_eq!(exits.check(exit), Ok(()), "{exit:?}"),
                "reject" => assert!(exits.check(exit).is_err(), "{exit:?} accepted"),
                other => panic!("unknown verdict {other:?} in exits.txt"),
            }
            checked += 1;
        }
        assert!(checked >= 6, "the fixture lost its cases");
    }

    #[test]
    fn the_real_plan_and_adrs_are_readable() {
        let exits = Exits::from_repo(&crate::util::repo_root()).expect("reads");
        assert!(exits.check("ADR-0078").is_ok());
        assert!(!exits.steps.is_empty(), "no step rows found in {PLAN}");
    }
}
