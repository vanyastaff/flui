//! Every repository path, package and `llms.txt` link the docs name exists.
//!
//! lychee (`docs-links`) follows Markdown links; this reads what it does not:
//!
//! - a repository path in a code span (`crates/flui-view/ARCHITECTURE.md`)
//!   of a Markdown file, or of `llms.txt`;
//! - the link destinations of `llms.txt`, which lychee reads as plain text;
//! - the package after `-p`/`--package` in a cargo command, in a code span or
//!   a code block, which must be one the checkout has ([`packages`]).
//!
//! [`extract`] says what counts as a path or a package. A path resolves
//! against the repository root, the doc's own directory, or the package the
//! doc sits in or that package's `src/`, so a crate's `ARCHITECTURE.md` may
//! write `src/lib.rs` or `platforms/mod.rs`. It is
//! judged against the files git knows (tracked, or untracked and not ignored)
//! and their directories, not the file system: an ignored file (`target/`,
//! a local `.rust-studio/`) or a name whose case differs passes on one host
//! and fails on Linux CI.
//!
//! The docs are the Markdown files git knows outside the archival roots
//! ([`ARCHIVAL_ROOTS`]), and `llms.txt`. The changelogs (`CHANGELOG.md` and a
//! crate's, the `changelog.d/` fragments) are not read: they name what a
//! release removed.
//!
//! Stale names recorded before the gate sit in [`ALLOWLIST`] as exact counts
//! per doc and kind, each with its reason: a count above the tree ("grew"),
//! below it ("lower it"), a kind with nothing left and a doc that is gone
//! are findings. `--seed` prints the allowlist the tree needs.

mod extract;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::docs_links::ARCHIVAL_ROOTS;
use crate::util::repo_root;

/// The allowlist, relative to the repository root.
pub(crate) const ALLOWLIST: &str = "tools/xtask/allowlists/docs-paths.toml";

/// The docs read besides the Markdown files.
const EXTRA_DOCS: [&str; 1] = ["llms.txt"];

/// Whether `doc` records what a release removed: a `CHANGELOG.md` (the
/// root's or a crate's), or a `changelog.d/` fragment.
fn history(doc: &str) -> bool {
    doc == "CHANGELOG.md" || doc.ends_with("/CHANGELOG.md") || doc.starts_with("changelog.d/")
}

/// The top-level directories of a package's own layout.
const PACKAGE_LAYOUT: [&str; 4] = ["benches", "examples", "src", "tests"];

/// Links to this repository's `main` on GitHub; the rest is a checkout path.
const SELF_MAIN: [&str; 2] = [
    "https://github.com/vanyastaff/flui/blob/main/",
    "https://github.com/vanyastaff/flui/tree/main/",
];

/// Arguments for `cargo xtask docs-paths`.
#[derive(Debug, clap::Args)]
pub(crate) struct DocsPathsArgs {
    /// Print the allowlist the tree needs (to stdout; no file is written).
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask docs-paths`.
pub(crate) fn docs_paths(args: &DocsPathsArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let known = Known::new(listed(&root)?);
    let packages = packages(&root, &known)?;
    let docs = docs(&known);
    let mut found = Vec::new();
    for doc in &docs {
        let text =
            std::fs::read_to_string(root.join(doc)).with_context(|| format!("reading {doc}"))?;
        found.extend(stale(doc, &text, &known, &packages));
    }
    if args.seed {
        print!("{}", seed(&found));
        return Ok(ExitCode::SUCCESS);
    }
    let allow: Allowlist = toml::from_str(&crate::util::read(ALLOWLIST)?)
        .with_context(|| format!("parsing {ALLOWLIST}"))?;
    let findings = judge(&found, &docs, &allow);
    for finding in &findings {
        println!("{finding}");
    }
    if !findings.is_empty() {
        eprintln!(
            "docs-paths: {} finding(s); allowlist {ALLOWLIST}",
            findings.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    let allowed: usize = allow.allow.iter().map(|entry| entry.count).sum();
    println!(
        "docs-paths: {} docs, {allowed} allowlisted in {} entries",
        docs.len(),
        allow.allow.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// The files git knows, repository-relative and `/`-separated, that are on disk.
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
    Ok(out
        .split_terminator('\0')
        // still in the index, deleted in the working tree: gone
        .filter(|path| root.join(path).is_file())
        .map(str::to_owned)
        .collect())
}

/// The packages a `-p` may name: the workspace members, every other package
/// the checkout has a manifest for (the Android examples are excluded from
/// the workspace), and every package in `Cargo.lock` (`cargo update -p wgpu`).
fn packages(root: &std::path::Path, known: &Known) -> anyhow::Result<BTreeSet<String>> {
    #[derive(Deserialize)]
    struct Manifest {
        package: Option<Named>,
    }
    #[derive(Deserialize)]
    struct Lock {
        #[serde(default)]
        package: Vec<Named>,
    }
    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    let mut names: BTreeSet<String> = crate::util::metadata(root)?
        .workspace_packages()
        .into_iter()
        .map(|package| package.name.to_string())
        .collect();
    for manifest in known
        .files
        .iter()
        .filter(|file| *file == "Cargo.toml" || file.ends_with("/Cargo.toml"))
    {
        let text = std::fs::read_to_string(root.join(manifest))
            .with_context(|| format!("reading {manifest}"))?;
        // a template or fixture manifest that is not TOML names no package
        if let Ok(Manifest {
            package: Some(package),
        }) = toml::from_str(&text)
        {
            names.insert(package.name);
        }
    }
    let lock: Lock =
        toml::from_str(&crate::util::read("Cargo.lock")?).context("parsing Cargo.lock")?;
    names.extend(lock.package.into_iter().map(|package| package.name));
    Ok(names)
}

/// The docs the scan reads, sorted.
fn docs(known: &Known) -> Vec<String> {
    known
        .files
        .iter()
        .filter(|path| {
            (extract::has_extension(path, "md")
                && !ARCHIVAL_ROOTS.iter().any(|root| path.starts_with(root))
                && !history(path))
                || EXTRA_DOCS.contains(&path.as_str())
        })
        .cloned()
        .collect()
}

/// The files git knows and every directory above one.
#[derive(Debug, Default)]
struct Known {
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
}

impl Known {
    fn new(files: impl IntoIterator<Item = String>) -> Self {
        let files: BTreeSet<String> = files.into_iter().collect();
        let dirs = files
            .iter()
            .flat_map(|file| file.match_indices('/').map(|(at, _)| file[..at].to_owned()))
            .collect();
        Self { files, dirs }
    }

    /// Whether `path` (repository-relative; a trailing `/` asks for a
    /// directory) is a known file or directory.
    fn has(&self, path: &str) -> bool {
        match path.strip_suffix('/') {
            Some(dir) => self.dirs.contains(dir),
            None => self.files.contains(path) || self.dirs.contains(path),
        }
    }

    /// The directory of the package `doc` sits in: the nearest one above it
    /// with a `Cargo.toml`.
    fn package_of<'a>(&self, doc: &'a str) -> Option<&'a str> {
        doc.match_indices('/')
            .rev()
            .map(|(at, _)| &doc[..at])
            .find(|dir| self.files.contains(&format!("{dir}/Cargo.toml")))
    }

    /// Every package directory: each one with a `Cargo.toml` below the root.
    fn packages(&self) -> impl Iterator<Item = &str> {
        self.files
            .iter()
            .filter_map(|file| file.strip_suffix("/Cargo.toml"))
    }

    /// Whether the code-span `path` in `doc` resolves from the root, the
    /// doc's directory, its package or the package's `src/`; a path in a package's layout
    /// ([`PACKAGE_LAYOUT`]) from any package, since a doc outside a crate
    /// names one crate's `tests/x.rs` with the crate in the prose around it.
    fn resolves(&self, doc: &str, path: &str) -> bool {
        let dir = doc.rsplit_once('/').map(|(dir, _)| dir);
        let package = self.package_of(doc);
        let package_src = package.map(|package| format!("{package}/src"));
        let found = [Some(""), dir, package, package_src.as_deref()]
            .into_iter()
            .flatten()
            .any(|base| self.has(&joined(base, path)));
        let in_layout = PACKAGE_LAYOUT
            .iter()
            .any(|layout| path.split('/').next() == Some(*layout));
        found || (in_layout && self.packages().any(|base| self.has(&joined(base, path))))
    }
}

/// `path` under the directory `base` (`""` is the root).
fn joined(base: &str, path: &str) -> String {
    if base.is_empty() {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

/// What a stale name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// A repository path in a code span.
    Path,
    /// A `-p`/`--package` argument that is no workspace member.
    Package,
    /// A link destination (`llms.txt`).
    Link,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Path, Self::Package, Self::Link];

    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Package => "package",
            Self::Link => "link",
        }
    }
}

/// A name in a doc that points at nothing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Stale {
    doc: String,
    line: usize,
    kind: Kind,
    name: String,
}

impl fmt::Display for Stale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.kind {
            Kind::Path => "no such file or directory",
            Kind::Package => "not a workspace member",
            Kind::Link => "the link resolves to no file or directory",
        };
        write!(
            f,
            "{}:{}: {} `{}`: {why}",
            self.doc,
            self.line,
            self.kind.name(),
            self.name
        )
    }
}

/// Every stale name in `text`, the doc at `doc`.
fn stale(doc: &str, text: &str, known: &Known, packages: &BTreeSet<String>) -> Vec<Stale> {
    let mut found = Vec::new();
    let mut push = |line: usize, kind: Kind, name: &str| {
        found.push(Stale {
            doc: doc.to_owned(),
            line,
            kind,
            name: name.to_owned(),
        });
    };
    for code in extract::code(text) {
        if !code.block {
            for path in extract::paths(&code.text) {
                if !known.resolves(doc, path) {
                    push(code.line, Kind::Path, path);
                }
            }
        }
        for (offset, name) in extract::packages(&code.text) {
            if !packages.contains(name) {
                // `code.line` is the line of the block's first line of text
                push(code.line + offset, Kind::Package, name);
            }
        }
    }
    if EXTRA_DOCS.contains(&doc) {
        for (line, dest) in extract::links(text) {
            if let Some(path) = link_target(doc, &dest)
                && !path.as_deref().is_some_and(|path| known.has(path))
            {
                push(line, Kind::Link, &dest);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// What the link `dest` in `doc` names in the checkout: `None` when it is
/// not a local link (another site, an in-page anchor), `Some(None)` when it
/// climbs above the root.
#[expect(
    clippy::option_option,
    reason = "not local, and local but outside the checkout, are different answers"
)]
fn link_target(doc: &str, dest: &str) -> Option<Option<String>> {
    let local = SELF_MAIN
        .iter()
        .find_map(|prefix| dest.strip_prefix(prefix))
        .map(|rest| format!("/{rest}"));
    let dest = match &local {
        Some(dest) => dest.as_str(),
        None if dest.contains(':') || dest.starts_with('#') || dest.is_empty() => return None,
        None => dest,
    };
    let dest = dest.split(['#', '?']).next().unwrap_or_default();
    let (base, rest) = match dest.strip_prefix('/') {
        Some(rest) => ("", rest),
        None => (doc.rsplit_once('/').map_or("", |(dir, _)| dir), dest),
    };
    let mut segments: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Some(None);
                }
            }
            segment => segments.push(segment),
        }
    }
    Some(Some(segments.join("/")))
}

/// The allowlist file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    #[serde(default)]
    allow: Vec<Entry>,
}

/// The stale names of one kind one doc may keep, exactly, and why.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    kind: String,
    count: usize,
    reason: String,
}

/// One way the tree and the allowlist disagree with the rule.
#[derive(Debug, PartialEq, Eq)]
enum Finding {
    /// A stale name no entry covers.
    Stale(Stale),
    /// More stale names than the entry grants, and each of them.
    Grew {
        doc: String,
        kind: String,
        allowed: usize,
        names: Vec<Stale>,
    },
    Shrank {
        doc: String,
        kind: String,
        count: usize,
        allowed: usize,
    },
    Zero {
        doc: String,
        kind: String,
    },
    Gone {
        doc: String,
        kind: String,
    },
    UnknownKind {
        doc: String,
        kind: String,
    },
    Duplicate {
        doc: String,
        kind: String,
    },
    NoReason {
        doc: String,
        kind: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale(stale) => write!(f, "{stale}"),
            Self::Grew {
                doc,
                kind,
                allowed,
                names,
            } => {
                write!(
                    f,
                    "{doc}: {} stale {kind} name(s), the allowlist grants {allowed}: it grew; \
                     fix the doc instead",
                    names.len()
                )?;
                for name in names {
                    write!(f, "\n  {name}")?;
                }
                Ok(())
            }
            Self::Shrank {
                doc,
                kind,
                count,
                allowed,
            } => write!(
                f,
                "{doc}: {count} stale {kind} name(s), the allowlist grants {allowed}: \
                 lower `count` to {count} in {ALLOWLIST}"
            ),
            Self::Zero { doc, kind } => write!(
                f,
                "{doc}: no stale {kind} name left: remove its entry from {ALLOWLIST}"
            ),
            Self::Gone { doc, kind } => write!(
                f,
                "{doc}: allowlisted for {kind} but not scanned (gone, archival or history): \
                 remove its entry from {ALLOWLIST}"
            ),
            Self::UnknownKind { doc, kind } => write!(
                f,
                "{doc}: allowlisted for an unknown kind {kind:?} (path, package or link)"
            ),
            Self::Duplicate { doc, kind } => {
                write!(f, "{doc}: allowlisted twice for {kind} in {ALLOWLIST}")
            }
            Self::NoReason { doc, kind } => {
                write!(f, "{doc}: the {kind} entry in {ALLOWLIST} has no reason")
            }
        }
    }
}

/// Every finding of `found`, the stale names in the `scanned` docs, against `allow`.
fn judge(found: &[Stale], scanned: &[String], allow: &Allowlist) -> Vec<Finding> {
    let mut groups: BTreeMap<(&str, &str), Vec<&Stale>> = BTreeMap::new();
    for stale in found {
        groups
            .entry((stale.doc.as_str(), stale.kind.name()))
            .or_default()
            .push(stale);
    }
    let mut findings = Vec::new();
    let mut covered: BTreeSet<(&str, &str)> = BTreeSet::new();
    for entry in &allow.allow {
        let (doc, kind) = (entry.path.clone(), entry.kind.clone());
        if !Kind::ALL.iter().any(|known| known.name() == entry.kind) {
            findings.push(Finding::UnknownKind { doc, kind });
            continue;
        }
        if !covered.insert((entry.path.as_str(), entry.kind.as_str())) {
            findings.push(Finding::Duplicate { doc, kind });
            continue;
        }
        if entry.reason.trim().is_empty() {
            findings.push(Finding::NoReason {
                doc: doc.clone(),
                kind: kind.clone(),
            });
        }
        if !scanned.contains(&entry.path) {
            findings.push(Finding::Gone { doc, kind });
            continue;
        }
        let names = groups
            .get(&(entry.path.as_str(), entry.kind.as_str()))
            .map_or(&[][..], Vec::as_slice);
        let (count, allowed) = (names.len(), entry.count);
        if count == 0 {
            findings.push(Finding::Zero { doc, kind });
        } else if count > allowed {
            findings.push(Finding::Grew {
                doc,
                kind,
                allowed,
                names: names.iter().map(|&stale| stale.clone()).collect(),
            });
        } else if count < allowed {
            findings.push(Finding::Shrank {
                doc,
                kind,
                count,
                allowed,
            });
        }
    }
    for ((doc, kind), names) in &groups {
        if !covered.contains(&(*doc, *kind)) {
            findings.extend(names.iter().map(|&stale| Finding::Stale(stale.clone())));
        }
    }
    findings
}

/// The allowlist `found` needs, as TOML; each `reason` left for a person.
fn seed(found: &[Stale]) -> String {
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for stale in found {
        *counts
            .entry((stale.doc.as_str(), stale.kind.name()))
            .or_default() += 1;
    }
    let mut out = String::from(
        "# Stale repository paths, packages and llms.txt links in the docs, written by\n\
         # `cargo xtask docs-paths --seed`. Counts are exact and only go down.\n",
    );
    for ((doc, kind), count) in counts {
        let _ = write!(
            out,
            "\n[[allow]]\npath = \"{doc}\"\nkind = \"{kind}\"\ncount = {count}\nreason = \"\"\n"
        );
    }
    out
}

#[cfg(test)]
mod tests;
