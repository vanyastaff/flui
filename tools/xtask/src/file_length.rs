//! No Rust source file has more than [`LIMIT`] production lines (ADR-0081 §5).
//!
//! Production is what a non-test build compiles: the files reached from every
//! library, binary, proc-macro and build-script target root through `mod`
//! declarations, without the modules and items whose `cfg` implies `test`
//! ([`modules`]). Test, bench and example targets are not production, so
//! `tests/`, `benches/` and `examples/` drop out by construction. A file's
//! production lines are its physical lines minus the lines of its test-only
//! items, each counted from its first outer attribute (doc comments included)
//! to its last token. A test-only statement or field inside production code
//! still counts; the count errs high, never low.
//!
//! Files over the limit today sit in [`ALLOWLIST`] with their exact count, the
//! reason and the exit that removes them ([`crate::ratchet`]). An entry is a
//! finding when the file grew past its count, shrank below it (lower the
//! count), came under the limit (remove the entry) or is gone. Exit 1 with one
//! line per finding. `--self-test` runs the scan over a planted crate tree and
//! fails unless exactly the planted findings come back; `--seed` prints the
//! allowlist the tree needs, with an empty `exit` the scan refuses until it is
//! filled in.
//!
//! Limits: modules declared inside a macro invocation are invisible to `syn`,
//! and `include!`d files (generated under `OUT_DIR`) are not read. The scan
//! reports, as a note, how many tracked `.rs` files under a production target's
//! source directory the walk did not reach.

mod modules;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::ratchet::Exits;
use crate::util::{ScratchDir, repo_root};

/// The most production lines a file may have (ADR-0081 §5), inclusive.
const LIMIT: usize = 3000;

/// The allowlist, relative to the repository root.
pub(crate) const ALLOWLIST: &str = "tools/xtask/allowlists/file-length.toml";

/// Target kinds whose code a non-test build compiles.
const PRODUCTION_KINDS: [&str; 8] = [
    "lib",
    "rlib",
    "dylib",
    "cdylib",
    "staticlib",
    "proc-macro",
    "bin",
    "custom-build",
];

/// Arguments for `cargo xtask file-length`.
#[derive(Debug, clap::Args)]
pub(crate) struct FileLengthArgs {
    /// Run the scan over a planted crate tree instead of the workspace.
    #[arg(long, conflicts_with = "seed")]
    self_test: bool,
    /// Print the allowlist the workspace needs (to stdout; no file is written).
    #[arg(long)]
    seed: bool,
}

/// `cargo xtask file-length`.
pub(crate) fn file_length(args: &FileLengthArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return self_test();
    }
    let root = repo_root();
    let meta = crate::util::metadata(&root)?;
    let targets = production_targets(&meta);
    let tree = modules::walk(targets.iter().map(|(path, _)| path.clone()));
    if args.seed {
        print!("{}", seed(&root, &tree));
        return Ok(ExitCode::SUCCESS);
    }
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST)?)
        .with_context(|| format!("parsing {ALLOWLIST}"))?;
    let exits = Exits::from_repo(&root)?;
    let findings = scan(&root, &tree, &allow, &exits);
    for finding in &findings {
        println!("{finding}");
    }
    let unreached = unreached(&root, &targets, &tree)?;
    if !findings.is_empty() {
        eprintln!(
            "file-length: {} finding(s); allowlist {ALLOWLIST}",
            findings.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "file-length: {} production files, none over {LIMIT} lines but {} allowlisted \
         ({unreached} tracked .rs file(s) under target source directories not reached)",
        tree.modules.len(),
        allow.allow.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Each production target root and the directory its sources live in.
fn production_targets(meta: &cargo_metadata::Metadata) -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut roots = Vec::new();
    for package in meta.workspace_packages() {
        for target in &package.targets {
            let kinds: Vec<String> = target.kind.iter().map(ToString::to_string).collect();
            if !kinds
                .iter()
                .any(|kind| PRODUCTION_KINDS.contains(&kind.as_str()))
            {
                continue;
            }
            let src = PathBuf::from(target.src_path.as_std_path());
            // a build script's directory is the whole package: not a source directory
            let dir = (!kinds.iter().any(|kind| kind == "custom-build"))
                .then(|| src.parent().map(Path::to_path_buf))
                .flatten();
            roots.push((src, dir));
        }
    }
    roots
}

/// The allowlist file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    #[serde(default)]
    allow: Vec<Entry>,
}

/// One file allowed over the limit, with its exact count.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Repository-relative, `/`-separated.
    path: String,
    /// Its production lines today.
    lines: usize,
    /// Why it is over the limit.
    reason: String,
    /// The ADR or plan step whose change removes the entry.
    exit: String,
}

impl Allowlist {
    fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }
}

/// One way the tree and the allowlist disagree with the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    Over {
        path: String,
        lines: usize,
    },
    Walk(String),
    Grew {
        path: String,
        lines: usize,
        allowed: usize,
    },
    Shrank {
        path: String,
        lines: usize,
        allowed: usize,
    },
    Fixed {
        path: String,
        lines: usize,
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

impl Finding {
    /// `(path, kind)`: what the self-test compares.
    fn identity(&self) -> (String, &'static str) {
        match self {
            Self::Over { path, .. } => (path.clone(), "over"),
            Self::Walk(line) => (
                line.split(':').next().unwrap_or_default().to_owned(),
                "walk",
            ),
            Self::Grew { path, .. } => (path.clone(), "grew"),
            Self::Shrank { path, .. } => (path.clone(), "shrank"),
            Self::Fixed { path, .. } => (path.clone(), "fixed"),
            Self::Gone { path } => (path.clone(), "gone"),
            Self::Duplicate { path } => (path.clone(), "duplicate"),
            Self::NoReason { path } => (path.clone(), "no-reason"),
            Self::BadExit { path, .. } => (path.clone(), "bad-exit"),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Over { path, lines } => {
                write!(f, "{path}: {lines} production lines (limit {LIMIT})")
            }
            Self::Walk(problem) => write!(f, "{problem}"),
            Self::Grew {
                path,
                lines,
                allowed,
            } => write!(
                f,
                "{path}: {lines} production lines, the allowlist grants {allowed}: it grew; \
                 move code out instead of raising the entry"
            ),
            Self::Shrank {
                path,
                lines,
                allowed,
            } => write!(
                f,
                "{path}: {lines} production lines, the allowlist grants {allowed}: \
                 lower `lines` to {lines} in {ALLOWLIST}"
            ),
            Self::Fixed { path, lines } => write!(
                f,
                "{path}: {lines} production lines, within the limit: remove its entry from {ALLOWLIST}"
            ),
            Self::Gone { path } => write!(
                f,
                "{path}: allowlisted but not a production file: remove its entry from {ALLOWLIST}"
            ),
            Self::Duplicate { path } => write!(f, "{path}: allowlisted twice in {ALLOWLIST}"),
            Self::NoReason { path } => {
                write!(f, "{path}: the allowlist entry has an empty `reason`")
            }
            Self::BadExit { path, exit, why } => {
                write!(f, "{path}: the allowlist entry's exit {exit:?}: {why}")
            }
        }
    }
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every finding of `tree` against `allow`.
fn scan(root: &Path, tree: &modules::Tree, allow: &Allowlist, exits: &Exits) -> Vec<Finding> {
    // the walk's paths are absolute; show them as the rest of the report does
    let prefix = format!("{}{}", root.display(), std::path::MAIN_SEPARATOR);
    let mut findings: Vec<Finding> = tree
        .problems
        .iter()
        .map(|problem| Finding::Walk(problem.to_string().replace(&prefix, "").replace('\\', "/")))
        .collect();
    let counts: BTreeMap<String, usize> = tree
        .modules
        .values()
        .map(|module| (relative(root, &module.path), module.production_lines()))
        .collect();
    let mut entries: BTreeMap<&str, &Entry> = BTreeMap::new();
    for entry in &allow.allow {
        if entries.insert(entry.path.as_str(), entry).is_some() {
            findings.push(Finding::Duplicate {
                path: entry.path.clone(),
            });
        }
        if entry.reason.trim().is_empty() {
            findings.push(Finding::NoReason {
                path: entry.path.clone(),
            });
        }
        if let Err(why) = exits.check(&entry.exit) {
            findings.push(Finding::BadExit {
                path: entry.path.clone(),
                exit: entry.exit.clone(),
                why,
            });
        }
        let path = entry.path.clone();
        let allowed = entry.lines;
        findings.extend(match counts.get(&entry.path).copied() {
            None => Some(Finding::Gone { path }),
            Some(lines) if lines <= LIMIT => Some(Finding::Fixed { path, lines }),
            Some(lines) if lines > allowed => Some(Finding::Grew {
                path,
                lines,
                allowed,
            }),
            Some(lines) if lines < allowed => Some(Finding::Shrank {
                path,
                lines,
                allowed,
            }),
            Some(_) => None,
        });
    }
    for (path, &lines) in &counts {
        if lines > LIMIT && !entries.contains_key(path.as_str()) {
            findings.push(Finding::Over {
                path: path.clone(),
                lines,
            });
        }
    }
    findings
}

/// The allowlist `tree` needs, as TOML; `exit` and `reason` left for a person.
fn seed(root: &Path, tree: &modules::Tree) -> String {
    let mut out = String::from(
        "# Rust files over the production-line limit (ADR-0081 §5), written by\n\
         # `cargo xtask file-length --seed`. Counts are exact and only go down.\n",
    );
    for module in tree.modules.values() {
        let lines = module.production_lines();
        if lines > LIMIT {
            let _ = write!(
                out,
                "\n[[allow]]\npath = \"{}\"\nlines = {lines}\nreason = \"\"\nexit = \"\"\n",
                relative(root, &module.path)
            );
        }
    }
    out
}

/// Tracked `.rs` files under a production target's source directory that the
/// walk reached neither as production nor as a test module.
fn unreached(
    root: &Path,
    targets: &[(PathBuf, Option<PathBuf>)],
    tree: &modules::Tree,
) -> anyhow::Result<usize> {
    let listed = Command::new("git")
        .args(["ls-files", "-z", "--cached", "--", "*.rs"])
        .current_dir(root)
        .output()
        .context("running `git ls-files`")?;
    if !listed.status.success() {
        bail!("`git ls-files` failed ({})", listed.status);
    }
    let listed =
        String::from_utf8(listed.stdout).context("`git ls-files` printed a non-UTF-8 path")?;
    let dirs: Vec<String> = targets
        .iter()
        .filter_map(|(_, dir)| dir.as_deref())
        .map(|dir| format!("{}/", relative(root, dir)))
        .collect();
    let reached: BTreeSet<String> = tree
        .modules
        .keys()
        .chain(&tree.test_files)
        .map(|path| relative(root, path))
        .collect();
    Ok(listed
        .split_terminator('\0')
        .filter(|path| dirs.iter().any(|dir| path.starts_with(dir.as_str())))
        .filter(|path| !reached.contains(*path))
        .count())
}

/// A planted file: its path under the self-test crate and its text.
fn planted() -> Vec<(&'static str, String)> {
    let lines = |n: usize| "// production line\n".repeat(n);
    vec![
        (
            "src/lib.rs",
            "mod big;\nmod edge;\nmod mixed;\nmod anytest;\n#[cfg(test)]\nmod tests_file;\n\
             mod inner;\n#[path = \"sub/renamed.rs\"]\nmod renamed;\nmod nested;\nmod a;\n\
             mod implonly;\nmod granted;\nmod grew;\nmod shrank;\nmod fixed;\nmod badexit;\n\
             mod missing;\n"
                .to_owned(),
        ),
        ("src/big.rs", lines(LIMIT + 1)),
        ("src/edge.rs", lines(LIMIT)),
        (
            "src/mixed.rs",
            format!(
                "{}#[cfg(all(test, not(target_os = \"android\")))]\nmod tests {{\n{}}}\n",
                lines(LIMIT - 10),
                lines(4000)
            ),
        ),
        (
            "src/anytest.rs",
            format!(
                "{}#[cfg(any(test, feature = \"x\"))]\nmod helpers {{\n{}}}\n",
                lines(LIMIT - 10),
                lines(17)
            ),
        ),
        ("src/tests_file.rs", lines(5000)),
        ("src/inner.rs", format!("#![cfg(test)]\n{}", lines(5000))),
        (
            "src/sub/renamed.rs",
            format!("mod child;\n{}", lines(LIMIT)),
        ),
        ("src/sub/child.rs", lines(LIMIT + 2)),
        ("src/nested/mod.rs", "mod deep;\n".to_owned()),
        ("src/nested/deep.rs", lines(LIMIT + 5)),
        (
            "src/a.rs",
            "mod b;\nmod inl {\n    #[path = \"p.rs\"]\n    mod p;\n}\n".to_owned(),
        ),
        ("src/a/b.rs", lines(LIMIT + 3)),
        ("src/a/inl/p.rs", lines(LIMIT + 4)),
        (
            "src/implonly.rs",
            format!(
                "{}struct S;\nimpl S {{\n    /// test support\n    #[cfg(test)]\n    fn big() {{\n{}    }}\n}}\n",
                lines(LIMIT - 10),
                lines(200)
            ),
        ),
        ("src/granted.rs", lines(3100)),
        ("src/grew.rs", lines(3105)),
        ("src/shrank.rs", lines(3050)),
        ("src/fixed.rs", lines(2000)),
        ("src/badexit.rs", lines(3100)),
    ]
}

/// The allowlist the self-test runs with.
const PLANTED_ALLOWLIST: &str = include_str!("../fixtures/file-length/allowlist.toml.txt");
/// The plan the self-test's exits resolve against.
const PLANTED_PLAN: &str = include_str!("../fixtures/ratchet/plan.md.txt");

/// What the self-test must report, and nothing else.
const EXPECTED: [(&str, &str); 13] = [
    ("src/big.rs", "over"),
    ("src/anytest.rs", "over"),
    ("src/sub/renamed.rs", "over"),
    ("src/sub/child.rs", "over"),
    ("src/nested/deep.rs", "over"),
    ("src/a/b.rs", "over"),
    ("src/a/inl/p.rs", "over"),
    ("src/lib.rs", "walk"),
    ("src/grew.rs", "grew"),
    ("src/shrank.rs", "shrank"),
    ("src/fixed.rs", "fixed"),
    ("src/gone.rs", "gone"),
    ("src/badexit.rs", "bad-exit"),
];

type Identity = (String, &'static str);

/// Writes the planted tree under a fresh directory and scans it.
fn self_test_findings() -> anyhow::Result<(ScratchDir, Vec<Finding>)> {
    let dir = ScratchDir::new("file-length")?;
    let root = dir.path().to_path_buf();
    for (path, text) in planted() {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("BUG: planted files sit in src/"))?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    let tree = modules::walk([root.join("src/lib.rs")]);
    let allow = Allowlist::parse(PLANTED_ALLOWLIST).context("parsing the planted allowlist")?;
    let exits = Exits::from_parts(["0081".to_owned()], PLANTED_PLAN);
    let findings = scan(&root, &tree, &allow, &exits);
    Ok((dir, findings))
}

/// `(missed, false positives)` of the scan over the planted tree.
fn self_test_diff() -> anyhow::Result<(Vec<Identity>, Vec<Identity>)> {
    let (_dir, findings) = self_test_findings()?;
    let seen: BTreeSet<Identity> = findings.iter().map(Finding::identity).collect();
    let expected: BTreeSet<Identity> = EXPECTED
        .iter()
        .map(|&(path, kind)| (path.to_owned(), kind))
        .collect();
    Ok((
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    ))
}

/// `cargo xtask file-length --self-test`.
fn self_test() -> anyhow::Result<ExitCode> {
    let (missed, extra) = self_test_diff()?;
    for (path, kind) in &missed {
        println!("self-test: MISSED ({path}, {kind})");
    }
    for (path, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({path}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "file-length: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests;
