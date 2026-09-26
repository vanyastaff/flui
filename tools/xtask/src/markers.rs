//! No process markers outside the archival roots (AGENTS.md, ADR-0078 §4).
//!
//! A process marker is a label of the process that produced a change rather
//! than a statement of what the code does: a cycle, phase letter, wave, slice
//! or PR label, a spec task id, a tracker id. The rule is about prose, so the
//! scan reads prose and nothing else ([`lex`]): the comment, doc and string
//! tokens of Rust ([`tokens`]), the text events of a Markdown parser, and TOML,
//! YAML and WGSL files whole. It never reads code structure, so a type or path
//! in code that happens to share a marker's shape is not a finding; an
//! identifier is matched only against the snake-case forms of the classes. The classes and
//! what each deliberately does not match are in [`classes`].
//!
//! Files are the ones git knows (tracked, or untracked and not ignored) with
//! a `.rs`, `.md`, `.toml`, `.yml`, `.yaml` or `.wgsl` extension, outside the
//! archival roots ([`crate::docs_links::ARCHIVAL_ROOTS`]), which must be the
//! list AGENTS.md states (the scan fails otherwise). Markers recorded
//! before the gate sit in [`ALLOWLIST`] as exact counts per file and class;
//! the file names one reason and one exit for all of them. A count above the
//! tree ("grew"), below it ("lower it"), a class with no markers left, a path
//! that is gone and an unknown class are findings. The allowlists' `exit`
//! values are references ([`crate::ratchet`]) and are not scanned.
//!
//! Exit 1 with one line per finding. `--self-test` scans the planted fixtures
//! and fails unless exactly the planted findings come back; `--seed` prints
//! the allowlist the tree needs, with an empty `reason` and `exit` the scan
//! refuses until they are filled in.
//!
//! Limits, not coverage: test-case labels (a letter and a digit collide with
//! function keys, key names and CPU registers); numbered phases used as process
//! labels; the roadmap's track names; markers in file names; a bare history
//! reference to a pull request by number; a marker inside backticks, which
//! Markdown makes quotation.

mod classes;
mod lex;
mod tokens;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::ops::Range;
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::docs_links::ARCHIVAL_ROOTS;
use crate::ratchet::Exits;
use crate::util::repo_root;

/// The allowlist, relative to the repository root.
pub(crate) const ALLOWLIST: &str = "tools/xtask/allowlists/markers.toml";

/// The allowlists whose `exit` values are references, not prose.
const EXIT_LISTS: [&str; 2] = [ALLOWLIST, crate::file_length::ALLOWLIST];

/// The extensions the scan reads.
const EXTENSIONS: [&str; 6] = ["rs", "md", "toml", "yml", "yaml", "wgsl"];

/// Arguments for `cargo xtask markers`.
#[derive(Debug, clap::Args)]
pub(crate) struct MarkersArgs {
    /// Scan the planted fixtures instead of the repository.
    #[arg(long, conflicts_with = "seed")]
    self_test: bool,
    /// Print the allowlist the tree needs (to stdout; no file is written).
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask markers`.
pub(crate) fn markers(args: &MarkersArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return self_test();
    }
    let root = repo_root();
    let files = listed(&root)?;
    let mut found = Vec::new();
    for path in &files {
        let full = root.join(path);
        let text = std::fs::read_to_string(&full)
            .with_context(|| format!("reading {}", full.display()))?;
        found.extend(find(path, &text, EXIT_LISTS.contains(&path.as_str())));
    }
    if args.seed {
        print!("{}", seed(&found));
        return Ok(ExitCode::SUCCESS);
    }
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST)?)
        .with_context(|| format!("parsing {ALLOWLIST}"))?;
    let exits = Exits::from_repo(&root)?;
    let scanned: BTreeSet<String> = files.into_iter().collect();
    let findings = judge(&found, &scanned, &allow, &exits);
    for finding in &findings {
        println!("{finding}");
    }
    // AGENTS.md is docs-only to the lanes, so its list is checked here, where every PR runs
    let drift = archival_drift(&crate::util::read("AGENTS.md")?);
    if let Err(why) = &drift {
        println!("AGENTS.md: {why}");
    }
    if !findings.is_empty() || drift.is_err() {
        eprintln!(
            "markers: {} finding(s); allowlist {ALLOWLIST}",
            findings.len() + usize::from(drift.is_err())
        );
        return Ok(ExitCode::FAILURE);
    }
    let allowed: usize = allow.allow.iter().map(|entry| entry.count).sum();
    println!(
        "markers: {} files, {allowed} allowlisted in {} entries",
        scanned.len(),
        allow.allow.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Whether the archival roots AGENTS.md names (`agents`, its text) are
/// exactly [`ARCHIVAL_ROOTS`], the ones the scan skips.
fn archival_drift(agents: &str) -> Result<(), String> {
    let flat = agents.split_whitespace().collect::<Vec<_>>().join(" ");
    let list = flat
        .split_once("Archival roots are exempt (")
        .and_then(|(_, rest)| rest.split_once(")."))
        .map(|(list, _)| list)
        .ok_or("no \"Archival roots are exempt (…).\" list")?;
    let mut listed = BTreeSet::new();
    for item in list.split('`').skip(1).step_by(2) {
        match item.split_once('{') {
            Some((prefix, braced)) => {
                for name in braced.trim_end_matches('}').split(',') {
                    listed.insert(format!("{prefix}{}/", name.trim()));
                }
            }
            None => {
                listed.insert(format!("{item}/"));
            }
        }
    }
    let roots: BTreeSet<String> = ARCHIVAL_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect();
    if listed == roots {
        return Ok(());
    }
    let only_listed: Vec<&String> = listed.difference(&roots).collect();
    let only_skipped: Vec<&String> = roots.difference(&listed).collect();
    Err(format!(
        "the archival roots it lists differ from the ones the scan skips \
         (listed only: {only_listed:?}; skipped only: {only_skipped:?}); \
         change both, `ARCHIVAL_ROOTS` in tools/xtask/src/docs_links.rs"
    ))
}

/// The files git knows that the scan reads, repository-relative.
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
        .filter(|path| {
            path.rsplit_once('.')
                .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext))
        })
        .filter(|path| !ARCHIVAL_ROOTS.iter().any(|root| path.starts_with(root)))
        // still in the index, deleted in the working tree: nothing to read
        .filter(|path| root.join(path).is_file())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// One marker in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Marker {
    path: String,
    line: usize,
    class: &'static str,
    matched: String,
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: \"{}\"",
            self.path, self.line, self.class, self.matched
        )
    }
}

/// Every marker in `text`, the file at `path`. With `exits_are_references`,
/// `text` is an allowlist and its `exit` values are not read.
fn find(path: &str, text: &str, exits_are_references: bool) -> Vec<Marker> {
    let masked;
    let text = if exits_are_references {
        masked = lex::blank(text, &exit_spans(text));
        masked.as_str()
    } else {
        text
    };
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let mut found: Vec<Marker> = lex::chunks(text, lex::Kind::of(path))
        .iter()
        .flat_map(|chunk| {
            classes::matches(&chunk.text, chunk.reading)
                .into_iter()
                .map(|(range, class)| Marker {
                    path: path.to_owned(),
                    line: line_starts
                        .partition_point(|&start| start <= chunk.file_offset(range.start)),
                    class,
                    matched: chunk.text[range].to_owned(),
                })
        })
        .collect();
    found.sort();
    found
}

/// The byte ranges of the `exit` values of an allowlist, top-level and per entry.
fn exit_spans(text: &str) -> Vec<Range<usize>> {
    #[derive(Deserialize)]
    struct File {
        exit: Option<toml::Spanned<String>>,
        #[serde(default)]
        allow: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        exit: Option<toml::Spanned<String>>,
    }
    // an allowlist that does not parse is its own gate's finding; read it all
    let Ok(file) = toml::from_str::<File>(text) else {
        return Vec::new();
    };
    file.exit
        .iter()
        .chain(file.allow.iter().filter_map(|entry| entry.exit.as_ref()))
        .map(toml::Spanned::span)
        .collect()
}

/// The allowlist file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    /// Why the markers are there; one reason for every entry.
    #[serde(default)]
    reason: String,
    /// The ADR or plan step whose change removes every entry.
    #[serde(default)]
    exit: String,
    #[serde(default)]
    allow: Vec<Entry>,
}

/// The markers of one class one file may carry, exactly.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    class: String,
    count: usize,
}

impl Allowlist {
    fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }
}

/// One way the tree and the allowlist disagree with the rule.
#[derive(Debug)]
enum Finding {
    /// A marker no entry covers.
    Marker(Marker),
    /// More markers than the entry grants; the markers of the group.
    Grew {
        path: String,
        class: String,
        allowed: usize,
        markers: Vec<Marker>,
    },
    Shrank {
        path: String,
        class: String,
        count: usize,
        allowed: usize,
    },
    Zero {
        path: String,
        class: String,
    },
    Gone {
        path: String,
        class: String,
    },
    UnknownClass {
        path: String,
        class: String,
    },
    Duplicate {
        path: String,
        class: String,
    },
    NoReason,
    BadExit {
        exit: String,
        why: String,
    },
}

impl Finding {
    /// `(path, line, label)`: what the self-test compares.
    fn identity(&self) -> (String, usize, String) {
        let group =
            |kind: &str, path: &str, class: &str| (path.to_owned(), 0, format!("{kind} {class}"));
        match self {
            Self::Marker(marker) => (marker.path.clone(), marker.line, marker.class.to_owned()),
            Self::Grew { path, class, .. } => group("grew", path, class),
            Self::Shrank { path, class, .. } => group("shrank", path, class),
            Self::Zero { path, class } => group("zero", path, class),
            Self::Gone { path, class } => group("gone", path, class),
            Self::UnknownClass { path, class } => group("unknown", path, class),
            Self::Duplicate { path, class } => group("duplicate", path, class),
            Self::NoReason => (String::new(), 0, "no-reason".to_owned()),
            Self::BadExit { .. } => (String::new(), 0, "bad-exit".to_owned()),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Marker(marker) => write!(f, "{marker}"),
            Self::Grew {
                path,
                class,
                allowed,
                markers,
            } => {
                write!(
                    f,
                    "{path}: {} {class} marker(s), the allowlist grants {allowed}: it grew; \
                     state the invariant instead",
                    markers.len()
                )?;
                for marker in markers {
                    write!(f, "\n  {marker}")?;
                }
                Ok(())
            }
            Self::Shrank {
                path,
                class,
                count,
                allowed,
            } => write!(
                f,
                "{path}: {count} {class} marker(s), the allowlist grants {allowed}: \
                 lower `count` to {count} in {ALLOWLIST}"
            ),
            Self::Zero { path, class } => write!(
                f,
                "{path}: no {class} marker left: remove its entry from {ALLOWLIST}"
            ),
            Self::Gone { path, class } => write!(
                f,
                "{path}: allowlisted for {class} but not scanned (gone, or archival): \
                 remove its entry from {ALLOWLIST}"
            ),
            Self::UnknownClass { path, class } => {
                write!(f, "{path}: allowlisted for an unknown class {class:?}")
            }
            Self::Duplicate { path, class } => {
                write!(f, "{path}: allowlisted twice for {class} in {ALLOWLIST}")
            }
            Self::NoReason => write!(f, "{ALLOWLIST}: `reason` is empty"),
            Self::BadExit { exit, why } => write!(f, "{ALLOWLIST}: exit {exit:?}: {why}"),
        }
    }
}

/// Every finding of `found`, the markers in the `scanned` files, against `allow`.
fn judge(
    found: &[Marker],
    scanned: &BTreeSet<String>,
    allow: &Allowlist,
    exits: &Exits,
) -> Vec<Finding> {
    let mut groups: BTreeMap<(&str, &str), Vec<&Marker>> = BTreeMap::new();
    for marker in found {
        groups
            .entry((marker.path.as_str(), marker.class))
            .or_default()
            .push(marker);
    }
    let mut findings = Vec::new();
    if !allow.allow.is_empty() {
        if allow.reason.trim().is_empty() {
            findings.push(Finding::NoReason);
        }
        if let Err(why) = exits.check(&allow.exit) {
            findings.push(Finding::BadExit {
                exit: allow.exit.clone(),
                why,
            });
        }
    }
    let mut covered: BTreeSet<(&str, &str)> = BTreeSet::new();
    for entry in &allow.allow {
        let (path, class) = (entry.path.clone(), entry.class.clone());
        if !classes::known(&entry.class) {
            findings.push(Finding::UnknownClass { path, class });
            continue;
        }
        if !covered.insert((entry.path.as_str(), entry.class.as_str())) {
            findings.push(Finding::Duplicate { path, class });
            continue;
        }
        if !scanned.contains(&entry.path) {
            findings.push(Finding::Gone { path, class });
            continue;
        }
        let markers = groups
            .get(&(entry.path.as_str(), entry.class.as_str()))
            .map_or(&[][..], Vec::as_slice);
        let count = markers.len();
        let allowed = entry.count;
        if count == 0 {
            findings.push(Finding::Zero { path, class });
        } else if count > allowed {
            findings.push(Finding::Grew {
                path,
                class,
                allowed,
                markers: markers.iter().map(|&m| m.clone()).collect(),
            });
        } else if count < allowed {
            findings.push(Finding::Shrank {
                path,
                class,
                count,
                allowed,
            });
        }
    }
    for ((path, class), markers) in &groups {
        if !covered.contains(&(*path, *class)) {
            findings.extend(markers.iter().map(|&m| Finding::Marker(m.clone())));
        }
    }
    findings
}

/// The allowlist `found` needs, as TOML; `reason` and `exit` left for a person.
fn seed(found: &[Marker]) -> String {
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for marker in found {
        *counts
            .entry((marker.path.as_str(), marker.class))
            .or_default() += 1;
    }
    let mut out = String::from(
        "# Process markers outside the archival roots, written by `cargo xtask markers --seed`.\n\
         # Counts are exact and only go down. `reason` and `exit` apply to every entry.\n\
         reason = \"\"\nexit = \"\"\n",
    );
    for ((path, class), count) in counts {
        let _ = write!(
            out,
            "\n[[allow]]\npath = \"{path}\"\nclass = \"{class}\"\ncount = {count}\n"
        );
    }
    out
}

/// The planted files: a virtual path, the text, and whether it is an allowlist.
const PLANTED: [(&str, &str, bool); 4] = [
    (
        "planted.rs",
        include_str!("../fixtures/markers/planted.rs.txt"),
        false,
    ),
    (
        "planted.md",
        include_str!("../fixtures/markers/planted.md.txt"),
        false,
    ),
    (
        "planted.toml",
        include_str!("../fixtures/markers/planted.toml.txt"),
        false,
    ),
    ("allowlist.toml", PLANTED_ALLOWLIST, true),
];

/// The allowlist the self-test judges the planted markers against.
const PLANTED_ALLOWLIST: &str = include_str!("../fixtures/markers/allowlist.toml.txt");
/// The plan the planted allowlist's exit resolves against.
const PLANTED_PLAN: &str = include_str!("../fixtures/ratchet/plan.md.txt");

/// What the planted allowlist does to the planted markers (its comments say why).
const RATCHET_EXPECTED: [(&str, &str); 6] = [
    ("planted.rs", "grew slice"),
    ("planted.rs", "shrank tracker-h"),
    ("planted.toml", "zero cycle"),
    ("gone.rs", "gone cycle"),
    ("planted.md", "unknown bogus"),
    ("planted.rs", "duplicate cycle"),
];

type Identity = (String, usize, String);

/// The findings a fixture's own notes ask for: each line carrying
/// `expect:` and the classes it must produce.
fn annotated(path: &str, text: &str) -> BTreeSet<Identity> {
    let mut expected = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if let Some((_, classes)) = line.split_once("expect:") {
            for class in classes
                .split(|c: char| c.is_whitespace() || c == ',')
                .map(|class| class.trim_end_matches(['*', '/', '-', '>']))
                .filter(|class| !class.is_empty())
            {
                expected.insert((path.to_owned(), index + 1, class.to_owned()));
            }
        }
    }
    expected
}

/// Every marker in the planted files.
fn planted_markers() -> Vec<Marker> {
    PLANTED
        .iter()
        .flat_map(|&(path, text, allowlist)| find(path, text, allowlist))
        .collect()
}

/// `(missed, false positives)` of the scan, and then of the ratchet, over the fixtures.
fn self_test_diff() -> anyhow::Result<(Vec<Identity>, Vec<Identity>)> {
    let found = planted_markers();
    let expected: BTreeSet<Identity> = PLANTED
        .iter()
        .flat_map(|&(path, text, _)| annotated(path, text))
        .collect();
    let seen: BTreeSet<Identity> = found
        .iter()
        .map(|m| (m.path.clone(), m.line, m.class.to_owned()))
        .collect();
    let mut missed: Vec<Identity> = expected.difference(&seen).cloned().collect();
    let mut extra: Vec<Identity> = seen.difference(&expected).cloned().collect();

    let allow = Allowlist::parse(PLANTED_ALLOWLIST).context("parsing the planted allowlist")?;
    let exits = Exits::from_parts(Vec::new(), PLANTED_PLAN);
    let scanned: BTreeSet<String> = PLANTED.iter().map(|&(path, ..)| path.to_owned()).collect();
    let entries: BTreeSet<(&str, &str)> = allow
        .allow
        .iter()
        .map(|e| (e.path.as_str(), e.class.as_str()))
        .collect();
    let judged: BTreeSet<Identity> = judge(&found, &scanned, &allow, &exits)
        .iter()
        .map(Finding::identity)
        .collect();
    let want: BTreeSet<Identity> = expected
        .iter()
        .filter(|(path, _, class)| !entries.contains(&(path.as_str(), class.as_str())))
        .cloned()
        .chain(
            RATCHET_EXPECTED
                .iter()
                .map(|&(path, label)| (path.to_owned(), 0, label.to_owned())),
        )
        .collect();
    // a marker the scan gets wrong is wrong in both passes; report it once
    missed.extend(
        want.difference(&judged)
            .filter(|id| !missed.contains(id))
            .cloned()
            .collect::<Vec<_>>(),
    );
    extra.extend(
        judged
            .difference(&want)
            .filter(|id| !extra.contains(id))
            .cloned()
            .collect::<Vec<_>>(),
    );
    Ok((missed, extra))
}

/// `cargo xtask markers --self-test`.
fn self_test() -> anyhow::Result<ExitCode> {
    let (missed, extra) = self_test_diff()?;
    for (path, line, class) in &missed {
        println!("self-test: MISSED {path}:{line}: {class}");
    }
    for (path, line, class) in &extra {
        println!("self-test: FALSE POSITIVE {path}:{line}: {class}");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "markers: self-test ok ({} planted markers, {} allowlist findings, no others)",
        planted_markers().len(),
        RATCHET_EXPECTED.len()
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests;
