//! What the allowlists of the source gates (`markers`, `file-length`, `testing`)
//! have in common: each entry names the change that removes it, and that name
//! must point at something that exists.
//!
//! An exit is an ADR (`ADR-0081`), and the ADR must have a file under
//! `docs/adr/`: the decision that removes the entry is the record the entry
//! waits on.
//!
//! The allowlists count exactly, so a count only goes down: an entry above
//! the tree is a finding ("grew"), and so is one below it ("lower it"). What
//! the gates cannot see is an entry added or raised in the same change as the
//! code it excuses; that shows up only in the diff of the allowlist file.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Context;
use regex::Regex;

/// The ADR directory an `ADR-NNNN` exit must have a file in.
const ADR_DIR: &str = "docs/adr";

static ADR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ADR-(\d{4})$").expect("BUG: static regex"));

/// The exits that exist: the numbers of the ADRs.
#[derive(Debug, Default)]
pub(crate) struct Exits {
    adrs: BTreeSet<String>,
}

impl Exits {
    /// The ADRs under `docs/adr/` of the checkout at `root`.
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
        Ok(Self::from_adrs(adrs))
    }

    /// The exits for `adrs`, four-digit numbers.
    pub(crate) fn from_adrs(adrs: impl IntoIterator<Item = String>) -> Self {
        Self {
            adrs: adrs.into_iter().collect(),
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
        if exit.is_empty() {
            return Err("empty; name the ADR that removes the entry".to_owned());
        }
        Err("not an ADR (ADR-NNNN)".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &str = include_str!("../fixtures/ratchet/exits.txt");

    fn exit_names_an_existing_adr() {
        let exits = Exits::from_adrs(["0081".to_owned()]);
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

    fn the_real_adrs_are_readable() {
        let exits = Exits::from_repo(&crate::util::repo_root()).expect("reads");
        assert!(exits.check("ADR-0078").is_ok());
    }

    #[test]
    fn ratchet_exits_contract() {
        crate::table_test::run_table(
            "ratchet_exits_contract",
            &[
                (
                    "exit_names_an_existing_adr",
                    exit_names_an_existing_adr as fn(),
                ),
                (
                    "the_real_adrs_are_readable",
                    the_real_adrs_are_readable as fn(),
                ),
            ],
        );
    }
}
