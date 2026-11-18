//! No two records in `docs/adr/` share a number.
//!
//! Code and documents cite a record by its number alone, and sessions working in parallel each
//! take "the next free one": two pull requests can both add ADR-NNNN and both merge green.

use std::collections::BTreeMap;
use std::process::ExitCode;

use crate::util::{adr_files, repo_root};

/// Arguments for `cargo xtask adr-numbers`.
#[derive(Debug, clap::Args)]
pub(crate) struct AdrNumbersArgs {}

/// `cargo xtask adr-numbers`: check that no two records in `docs/adr/` share a number.
pub(crate) fn adr_numbers(_args: &AdrNumbersArgs) -> anyhow::Result<ExitCode> {
    let files = adr_files(&repo_root());
    anyhow::ensure!(
        !files.is_empty(),
        "docs/adr holds no record: the directory did not read"
    );
    let findings = duplicates(&files);
    if findings.is_empty() {
        println!(
            "adr-numbers: {} record(s), each number used once",
            files.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for finding in &findings {
        eprintln!("adr-numbers: {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// One finding per number that more than one of `files` (`ADR-NNNN-slug.md`) carries.
fn duplicates(files: &[String]) -> Vec<String> {
    let mut by_number: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for file in files {
        if let Some(number) = file
            .strip_prefix("ADR-")
            .and_then(|rest| rest.get(..4))
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
        {
            by_number.entry(number).or_default().push(file);
        }
    }
    by_number
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(number, mut files)| {
            files.sort_unstable();
            format!("ADR-{number} is used by {}", files.join(", "))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_two_records_carry_is_reported() {
        let files = [
            "ADR-0001-first.md",
            "ADR-0002-only.md",
            "ADR-0001-second.md",
            "README.md",
        ]
        .map(str::to_owned);
        assert_eq!(
            duplicates(&files),
            ["ADR-0001 is used by ADR-0001-first.md, ADR-0001-second.md"]
        );
        assert_eq!(duplicates(&files[1..2]), [] as [&str; 0]);
    }
}
