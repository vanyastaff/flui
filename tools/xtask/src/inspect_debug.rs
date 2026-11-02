//! `cargo xtask inspect-debug`: the `Value::Debug` escape hatch of
//! `flui_foundation::inspect` is counted, and the count only goes down.
//!
//! `Value::Debug(..)` describes a field by its `Debug` text: the one `Value`
//! with no type behind it (R50). Every site is a field that lacks a typed
//! mapping, so the sites are listed per file with the reason, and a new one is
//! a finding until it is listed or replaced by a typed value.
//!
//! **What counts.** In tracked `.rs` files under `crates/`, `packages/` and
//! `src/`, a line outside a `//` comment holding `Value::Debug(` that is not
//! the tail of a longer name (`OwnedValue::Debug(`, `InspectValue::Debug(`
//! are other enums). Left out: `crates/flui-foundation/src/inspect/` (the
//! infrastructure), `crates/flui-macros/`, and any path containing `/tests/`.
//!
//! **The list.** [`ALLOWLIST`] counts exactly, per file ([`crate::ratchet`]): a
//! count above the tree is a finding ("grew"), below it ("lower it"), and a
//! listed path that is gone is one. `--seed` prints the list the tree needs.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::ratchet::Exits;
use crate::util;

/// The allowlist, repository-relative.
const ALLOWLIST: &str = "tools/xtask/allowlists/inspect-debug.toml";

/// Directories the scan reads.
const ROOTS: [&str; 3] = ["crates/", "packages/", "src/"];

/// Paths the scan leaves out.
const SKIPPED: [&str; 2] = ["crates/flui-foundation/src/inspect/", "crates/flui-macros/"];

/// Arguments for `cargo xtask inspect-debug`.
#[derive(Debug, clap::Args)]
pub(crate) struct InspectDebugArgs {
    /// Print the allowlist the tree needs instead of checking it.
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask inspect-debug`. Exit code 1 lists every finding.
pub(crate) fn inspect_debug(args: &InspectDebugArgs) -> anyhow::Result<ExitCode> {
    let root = util::repo_root();
    let mut counts = BTreeMap::new();
    for path in listed(&root)?.into_iter().filter(|path| in_scope(path)) {
        let text =
            std::fs::read_to_string(root.join(&path)).with_context(|| format!("reading {path}"))?;
        let count = count_sites(&text);
        if count > 0 {
            counts.insert(path, count);
        }
    }
    if args.seed {
        print!("{}", seed(&counts));
        return Ok(ExitCode::SUCCESS);
    }
    let allow: Allowlist =
        toml::from_str(&util::read(ALLOWLIST)?).with_context(|| format!("parsing {ALLOWLIST}"))?;
    let findings = check(&counts, &allow, &Exits::from_repo(&root)?);
    if findings.is_empty() {
        println!(
            "inspect-debug: {} `Value::Debug` site(s) in {} file(s), each counted in {ALLOWLIST}",
            counts.values().sum::<usize>(),
            counts.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("inspect-debug: {} problem(s)", findings.len());
    for finding in &findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// Whether the scan reads `path`.
fn in_scope(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext == "rs")
        && ROOTS.iter().any(|root| path.starts_with(root))
        && !SKIPPED.iter().any(|skip| path.starts_with(skip))
        && !path.contains("/tests/")
}

/// The `Value::Debug(` sites of one file.
fn count_sites(text: &str) -> usize {
    const NEEDLE: &str = "Value::Debug(";
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| {
            line.match_indices(NEEDLE)
                .filter(|(at, _)| {
                    !line[..*at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
                .count()
        })
        .sum()
}

/// The files git knows, repository-relative.
fn listed(root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .context("running `git ls-files`")?;
    if !out.status.success() {
        bail!("`git ls-files` failed ({})", out.status);
    }
    let out = String::from_utf8(out.stdout).context("`git ls-files` printed a non-UTF-8 path")?;
    let mut files: Vec<String> = out
        .split_terminator('\0')
        // still in the index, deleted in the working tree: nothing to read
        .filter(|path| root.join(path).is_file())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// The allowlist file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One file's exact count.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Repository-relative, `/`-separated.
    path: String,
    /// `Value::Debug(` sites in the file today.
    count: usize,
    /// Why the fields have no typed value.
    reason: String,
    /// The ADR whose change removes the entry.
    exit: String,
}

/// What is wrong with the tree against the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    Unlisted {
        path: String,
        count: usize,
    },
    Grew {
        path: String,
        count: usize,
        allowed: usize,
    },
    LowerIt {
        path: String,
        count: usize,
        allowed: usize,
    },
    Gone {
        path: String,
    },
    Duplicate {
        path: String,
    },
    NoReason {
        path: String,
    },
    BadExit {
        path: String,
        exit: String,
        why: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted { path, count } => write!(
                f,
                "{path}: {count} `Value::Debug` site(s), not in {ALLOWLIST}: give the field a \
                 typed value, or list the file with its reason"
            ),
            Self::Grew {
                path,
                count,
                allowed,
            } => write!(
                f,
                "{path}: {count} `Value::Debug` site(s), the allowlist grants {allowed}: it \
                 grew; give the new field a typed value"
            ),
            Self::LowerIt {
                path,
                count,
                allowed,
            } => write!(
                f,
                "{path}: {count} `Value::Debug` site(s), the allowlist grants {allowed}: lower \
                 it to {count} in {ALLOWLIST}"
            ),
            Self::Gone { path } => write!(
                f,
                "{path}: listed, but it has no `Value::Debug` site or no longer exists; remove \
                 it from {ALLOWLIST}"
            ),
            Self::Duplicate { path } => write!(f, "{path}: listed twice in {ALLOWLIST}"),
            Self::NoReason { path } => write!(f, "{path}: the allowlist entry has no reason"),
            Self::BadExit { path, exit, why } => {
                write!(f, "{path}: the allowlist entry's exit {exit:?}: {why}")
            }
        }
    }
}

/// Every finding of `counts` against `allow`.
fn check(counts: &BTreeMap<String, usize>, allow: &Allowlist, exits: &Exits) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut listed: BTreeMap<&str, &Entry> = BTreeMap::new();
    for entry in &allow.allow {
        let path = entry.path.clone();
        if listed.insert(&entry.path, entry).is_some() {
            findings.push(Finding::Duplicate { path: path.clone() });
        }
        if entry.reason.trim().is_empty() {
            findings.push(Finding::NoReason { path: path.clone() });
        }
        if let Err(why) = exits.check(&entry.exit) {
            findings.push(Finding::BadExit {
                path: path.clone(),
                exit: entry.exit.clone(),
                why,
            });
        }
        let allowed = entry.count;
        findings.extend(match counts.get(&entry.path).copied() {
            None => Some(Finding::Gone { path }),
            Some(count) if count > allowed => Some(Finding::Grew {
                path,
                count,
                allowed,
            }),
            Some(count) if count < allowed => Some(Finding::LowerIt {
                path,
                count,
                allowed,
            }),
            Some(_) => None,
        });
    }
    for (path, &count) in counts {
        if !listed.contains_key(path.as_str()) {
            findings.push(Finding::Unlisted {
                path: path.clone(),
                count,
            });
        }
    }
    findings.sort();
    findings
}

/// The allowlist `counts` needs, as TOML; `reason` and `exit` left for a person.
fn seed(counts: &BTreeMap<String, usize>) -> String {
    let mut out = String::from(
        "# `Value::Debug(` sites of flui_foundation::inspect, written by\n\
         # `cargo xtask inspect-debug --seed`. Counts are exact and only go down.\n",
    );
    for (path, count) in counts {
        let _ = write!(
            out,
            "\n[[allow]]\npath = \"{path}\"\ncount = {count}\nreason = \"\"\nexit = \"\"\n"
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exits() -> Exits {
        Exits::from_adrs(["0242".to_owned()])
    }

    fn entry(path: &str, count: usize) -> Entry {
        Entry {
            path: path.to_owned(),
            count,
            reason: "no value mapping".to_owned(),
            exit: "ADR-0242".to_owned(),
        }
    }

    fn findings(counts: &[(&str, usize)], allow: Vec<Entry>) -> Vec<Finding> {
        let counts = counts
            .iter()
            .map(|(path, count)| ((*path).to_owned(), *count))
            .collect();
        check(&counts, &Allowlist { allow }, &exits())
    }

    fn counts_sites() {
        let text = "out.value(\"a\", Value::Debug(&x));\n\
                    // Value::Debug(&y) in a comment\n\
                    OwnedValue::Debug(t) => InspectValue::Debug(t),\n\
                    flui_foundation::inspect::Value::Debug(&z),\n\
                    Value::Debug(a); Value::Debug(b);\n";
        assert_eq!(count_sites(text), 4);
    }

    fn scopes_paths() {
        assert!(in_scope("crates/flui-view/src/inspect.rs"));
        assert!(in_scope("src/lib.rs"));
        assert!(!in_scope("crates/flui-foundation/src/inspect/value.rs"));
        assert!(!in_scope("crates/flui-macros/src/lib.rs"));
        assert!(!in_scope("crates/flui-view/tests/inspect.rs"));
        assert!(!in_scope("tools/xtask/src/inspect_debug.rs"));
        assert!(!in_scope("crates/flui-view/README.md"));
    }

    fn exact_count_passes() {
        assert_eq!(findings(&[("a.rs", 2)], vec![entry("a.rs", 2)]), vec![]);
    }

    fn grew_is_a_finding() {
        assert_eq!(
            findings(&[("a.rs", 3)], vec![entry("a.rs", 2)]),
            vec![Finding::Grew {
                path: "a.rs".into(),
                count: 3,
                allowed: 2
            }]
        );
    }

    fn lower_it_is_a_finding() {
        assert_eq!(
            findings(&[("a.rs", 1)], vec![entry("a.rs", 2)]),
            vec![Finding::LowerIt {
                path: "a.rs".into(),
                count: 1,
                allowed: 2
            }]
        );
    }

    fn gone_is_a_finding() {
        assert_eq!(
            findings(&[], vec![entry("a.rs", 2)]),
            vec![Finding::Gone {
                path: "a.rs".into()
            }]
        );
    }

    fn unlisted_and_bad_entries_are_findings() {
        assert_eq!(
            findings(&[("b.rs", 1)], vec![]),
            vec![Finding::Unlisted {
                path: "b.rs".into(),
                count: 1
            }]
        );
        let mut bad = entry("a.rs", 1);
        bad.reason = " ".into();
        bad.exit = "ADR-9999".into();
        let found = findings(&[("a.rs", 1)], vec![bad, entry("a.rs", 1)]);
        assert!(found.contains(&Finding::NoReason {
            path: "a.rs".into()
        }));
        assert!(found.contains(&Finding::Duplicate {
            path: "a.rs".into()
        }));
        assert!(
            found
                .iter()
                .any(|finding| matches!(finding, Finding::BadExit { .. }))
        );
    }

    #[test]
    fn inspect_debug_contract() {
        crate::table_test::run_table(
            "inspect_debug_contract",
            &[
                ("counts_sites", counts_sites as fn()),
                ("scopes_paths", scopes_paths as fn()),
                ("exact_count_passes", exact_count_passes as fn()),
                ("grew_is_a_finding", grew_is_a_finding as fn()),
                ("lower_it_is_a_finding", lower_it_is_a_finding as fn()),
                ("gone_is_a_finding", gone_is_a_finding as fn()),
                (
                    "unlisted_and_bad_entries_are_findings",
                    unlisted_and_bad_entries_are_findings as fn(),
                ),
            ],
        );
    }
}
