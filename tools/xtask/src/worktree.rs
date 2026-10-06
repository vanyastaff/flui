//! `cargo xtask worktree`: one root for task worktrees, and their cleanup once
//! origin/main holds their branch.
//!
//! Every task works in a worktree of its own (AGENTS.md, "Working here"), and
//! each worktree grows its own multi-GB `target/`. `new` puts them in a single
//! root, `.worktrees/` inside the main checkout (git-ignored, and skipped by
//! the gates that walk the tree); `list` shows every worktree git knows,
//! inside that root or not; `prune` removes the ones that hold nothing
//! origin/main lacks.
//!
//! The decisions are pure functions of git's answers ([`BranchName::parse`],
//! [`parse_worktrees`], [`parse_upstreams`], [`Changes::from_status`],
//! [`Tip::integration`], [`classify`]); the commands only ask git and act on
//! the verdict.
//!
//! A worktree is removed only on a positive merge signal, and only when nothing
//! in it is modified, untracked or ignored, except what is disposable: a root
//! `TASKS.md` and `target/` directories. `git worktree remove` deletes ignored
//! files without asking, and an ignored `.env`, key or tool state is not
//! anyone's to throw away.
//! This repository merges pull requests with merge commits, so a merged
//! branch's tip is an ancestor of origin/main that is *not* on origin/main's
//! first-parent chain: it entered as a merge's second parent. A tip on that
//! chain is one of main's own commits, which is where a branch `new` just
//! created points; such a worktree is unstarted, not merged, and is kept (so
//! is a branch fast-forwarded or rebase-merged onto main: the price of never
//! deleting a fresh one). A squash-merged branch has commits origin/main does
//! not contain, so it is kept too.
//!
//! The branch is then deleted with `git branch -d`, never `-D`: the merge
//! verdict is older than the deletion, and a commit made on the branch in
//! between must survive. `-d` judges against the branch's upstream or the
//! current `HEAD`, not origin/main, so when it refuses, the branch is left and
//! reported for deletion by hand.
//!
//! `list` reads the local origin/main and fetches nothing; `new` fetches main
//! and `prune` fetches with `--prune`, so "gone on origin" is current there.
//! `prune --dry-run` changes nothing, remote-tracking refs included: it skips
//! the fetch and judges against the local, possibly stale, origin/main.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::LazyLock;

use anyhow::{Context, bail, ensure};
use regex::Regex;

use crate::util::{human_size, repo_root, same_dir, tree_size};

/// The directory inside the main checkout that holds the task worktrees.
pub(crate) const ROOT_DIR: &str = ".worktrees";

/// The ref a worktree's branch must be contained in to be removed.
const BASE: &str = "origin/main";

/// The agent checklist at a worktree root: git-ignored scratch, never work.
const TASKS_FILE: &str = "TASKS.md";

/// Arguments for `cargo xtask worktree`.
#[derive(Debug, clap::Args)]
pub(crate) struct WorktreeArgs {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// Create branch `<area>/<slug>` from origin/main at `.worktrees/<slug>` in the main checkout.
    New {
        /// `<area>/<slug>`, each lowercase kebab-case.
        branch: String,
    },
    /// Every worktree: branch state, local changes and `target/` size.
    List,
    /// Remove clean worktrees whose branch was merged into origin/main, with their branches.
    ///
    /// Merged means the branch tip is an ancestor of origin/main but not on its
    /// first-parent chain: it came in through a merge commit. A branch whose tip
    /// is one of main's own commits (just created, or fast-forward/rebase merged)
    /// and a squash-merged branch are kept. A branch `git branch -d` refuses to
    /// delete is left and reported.
    Prune {
        /// Print what would be removed and change nothing: skips the fetch and
        /// judges against the local, possibly stale, origin/main.
        #[arg(long)]
        dry_run: bool,
    },
}

/// `cargo xtask worktree <action>`.
pub(crate) fn worktree(args: &WorktreeArgs) -> anyhow::Result<ExitCode> {
    let git = Git::new(repo_root());
    match &args.action {
        Action::New { branch } => {
            let branch = BranchName::parse(branch)?;
            println!("{}", new(&git, &branch)?.display());
            Ok(ExitCode::SUCCESS)
        }
        Action::List => {
            for worktree in survey(&git)? {
                println!("{worktree}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Action::Prune { dry_run } => {
            let report = prune(&git, *dry_run)?;
            for line in &report.lines {
                println!("{line}");
            }
            Ok(if report.failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

/// A branch name `new` accepts: `<area>/<slug>`, both lowercase kebab-case.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchName {
    full: String,
    slash: usize,
}

impl BranchName {
    fn parse(name: &str) -> anyhow::Result<Self> {
        static SHAPE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*/[a-z0-9]+(-[a-z0-9]+)*$")
                .expect("BUG: static regex is valid")
        });
        ensure!(
            SHAPE.is_match(name),
            "branch `{name}` is not `<area>/<slug>`: two lowercase kebab-case parts \
             (`[a-z0-9]` words joined by single `-`) separated by one `/`, \
             e.g. `tooling/worktree-command`"
        );
        let slash = name.find('/').expect("BUG: the pattern requires one `/`");
        Ok(Self {
            full: name.to_owned(),
            slash,
        })
    }

    /// The part after `/`: the worktree's directory name.
    fn slug(&self) -> &str {
        &self.full[self.slash + 1..]
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full)
    }
}

/// The main checkout, from `git worktree list`'s records and, when this
/// command runs in the main checkout, its `--show-toplevel`.
///
/// Neither the common git dir's parent nor, alone, the first record: for a
/// `git init --separate-git-dir` checkout git reports the git dir itself in
/// the checkout's place, and records the checkout nowhere a linked worktree
/// can read it. So the main checkout's own top level wins, then a first
/// record that holds a `.git`; anything else is refused.
fn main_checkout(entries: &[Entry], main_toplevel: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(toplevel) = main_toplevel {
        return Ok(toplevel.to_path_buf());
    }
    let first = entries
        .first()
        .context("`git worktree list` named no main worktree")?;
    ensure!(
        !first.bare,
        "{} is a bare repository: no main checkout to hold {ROOT_DIR}",
        first.path.display()
    );
    ensure!(
        first.path.join(".git").exists(),
        "{} is a separate git dir whose checkout git does not record; \
         run this from the main checkout",
        first.path.display()
    );
    Ok(first.path.clone())
}

/// One record of `git worktree list --porcelain -z`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    /// The short branch name; `None` when detached or bare.
    branch: Option<String>,
    bare: bool,
    locked: bool,
    /// The directory is gone; `git worktree prune` drops the record.
    prunable: bool,
}

/// Parses `git worktree list --porcelain -z`: NUL-terminated lines, records
/// separated by an empty one, the first record the main worktree.
///
/// `-z` because the newline form C-quotes some paths (a newline or a quote in
/// them, for one) and `-z` prints every path verbatim.
fn parse_worktrees(porcelain: &str) -> anyhow::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    for line in porcelain.split('\0') {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            entries.extend(current.take());
            current = Some(Entry {
                path: PathBuf::from(value),
                ..Entry::default()
            });
            continue;
        }
        if line.is_empty() {
            entries.extend(current.take());
            continue;
        }
        let Some(entry) = current.as_mut() else {
            bail!("`git worktree list --porcelain -z` line before any `worktree`: {line}");
        };
        match key {
            "branch" => {
                entry.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                );
            }
            "bare" => entry.bare = true,
            "locked" => entry.locked = true,
            "prunable" => entry.prunable = true,
            // `HEAD`, `detached` and anything a newer git adds decide nothing here
            _ => {}
        }
    }
    entries.extend(current);
    Ok(entries)
}

/// Which local branches have an upstream that no longer exists, from
/// `git for-each-ref --format=%(refname)%00%(upstream) refs/heads refs/remotes`.
///
/// Comparing ref names keeps this free of `%(upstream:track)`'s wording.
fn parse_upstreams(refs: &str) -> BTreeMap<String, bool> {
    let rows: Vec<(&str, &str)> = refs
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .collect();
    let existing: BTreeSet<&str> = rows.iter().map(|(name, _)| *name).collect();
    rows.iter()
        .filter_map(|(name, upstream)| {
            let branch = name.strip_prefix("refs/heads/")?;
            let gone = !upstream.is_empty() && !existing.contains(upstream);
            Some((branch.to_owned(), gone))
        })
        .collect()
}

/// What a worktree holds beyond its commit.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Changes {
    /// Nothing, or only disposable ignored entries.
    None,
    /// Only an untracked, unignored root `TASKS.md` (beside disposable ignored
    /// entries), which prune deletes itself after rechecking it is still alone.
    TasksOnly,
    /// Ignored entries that are not disposable, in `git status` order.
    Ignored(Vec<String>),
    /// Modified tracked files or untracked, unignored ones.
    Work,
}

impl Changes {
    /// Reads `git status --porcelain=v1 -z --untracked-files=normal
    /// --ignored=matching`.
    fn from_status(status: &str) -> Self {
        let mut tasks = false;
        let mut ignored = Vec::new();
        for record in status.split('\0').filter(|record| !record.is_empty()) {
            if let Some(path) = record.strip_prefix("!! ") {
                if !disposable(path) {
                    ignored.push(path.to_owned());
                }
            } else if record == format!("?? {TASKS_FILE}") {
                tasks = true;
            } else {
                return Self::Work;
            }
        }
        if !ignored.is_empty() {
            Self::Ignored(ignored)
        } else if tasks {
            Self::TasksOnly
        } else {
            Self::None
        }
    }
}

/// An ignored entry `prune` may delete with its worktree: a `target/`
/// directory at any depth, or the root `TASKS.md`.
fn disposable(path: &str) -> bool {
    path == TASKS_FILE || path == "target/" || path.ends_with("/target/")
}

/// What git says of a branch tip against origin/main.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tip {
    /// The tip is an ancestor of origin/main, or origin/main itself.
    reachable: bool,
    /// The tip is on origin/main's first-parent chain: one of main's own
    /// commits rather than a merged branch's.
    first_parent: bool,
    /// Commits on the branch that origin/main lacks.
    ahead: u64,
    /// The branch had an upstream that no longer exists.
    upstream_gone: bool,
}

impl Tip {
    /// Merged only on a positive signal: reachable from origin/main through a
    /// merge commit, not as one of main's own first-parent commits.
    fn integration(self) -> Integration {
        let upstream_gone = self.upstream_gone;
        if !self.reachable {
            Integration::Unmerged {
                ahead: self.ahead,
                upstream_gone,
            }
        } else if self.first_parent {
            Integration::Unstarted { upstream_gone }
        } else {
            Integration::Merged { upstream_gone }
        }
    }
}

/// Where a worktree's commits stand against origin/main.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Integration {
    /// The branch tip came into origin/main through a merge commit.
    Merged {
        upstream_gone: bool,
    },
    /// The branch tip is one of origin/main's first-parent commits: nothing
    /// committed yet, or fast-forward/rebase merged. Nothing proves a merge.
    Unstarted {
        upstream_gone: bool,
    },
    /// `ahead` commits are not on origin/main.
    Unmerged {
        ahead: u64,
        upstream_gone: bool,
    },
    Detached,
}

impl Integration {
    /// `tip` is `None` when detached.
    fn of(tip: Option<Tip>) -> Self {
        tip.map_or(Self::Detached, Tip::integration)
    }
}

/// The worktree's place in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The checkout the git dir belongs to.
    Main,
    /// The checkout running this command.
    Current,
    Other,
}

/// What `classify` decides from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Facts {
    role: Role,
    /// `None` when detached.
    tip: Option<Tip>,
    changes: Changes,
    locked: bool,
    missing: bool,
}

/// What `prune` does with a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Decision {
    /// Remove the worktree and delete its branch: origin/main contains it.
    /// `force` marks a lone untracked `TASKS.md`, which removal deletes after
    /// rechecking that it is still the only change.
    Remove {
        force: bool,
    },
    Keep(Reason),
}

/// Why `prune` keeps a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reason {
    MainCheckout,
    Current,
    Locked,
    Missing,
    Detached,
    Dirty,
    /// Ignored entries that are not disposable, in `git status` order.
    Ignored(Vec<String>),
    Unstarted,
    Unmerged {
        ahead: u64,
        upstream_gone: bool,
    },
}

/// How many ignored entries a [`Reason::Ignored`] line names.
const IGNORED_SHOWN: usize = 3;

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MainCheckout => f.write_str("the main checkout"),
            Self::Current => f.write_str("the current worktree"),
            Self::Locked => f.write_str("locked (`git worktree unlock` first)"),
            Self::Missing => f.write_str("directory gone; `git worktree prune` drops the record"),
            Self::Detached => f.write_str("detached HEAD, no branch to judge"),
            Self::Dirty => f.write_str("uncommitted or untracked changes"),
            Self::Ignored(paths) => {
                let shown = paths.get(..IGNORED_SHOWN).unwrap_or(paths);
                write!(f, "ignored files: {}", shown.join(", "))?;
                if paths.len() > shown.len() {
                    write!(f, " (+{} more)", paths.len() - shown.len())?;
                }
                Ok(())
            }
            Self::Unstarted => write!(
                f,
                "tip is one of {BASE}'s own commits: not started, or merged without a merge commit"
            ),
            Self::Unmerged {
                ahead,
                upstream_gone: true,
            } => write!(
                f,
                "branch gone on origin, but {ahead} commit(s) not on {BASE} (squash-merged?)"
            ),
            Self::Unmerged {
                ahead,
                upstream_gone: false,
            } => write!(f, "{ahead} commit(s) not on {BASE}"),
        }
    }
}

/// Whether `prune` may remove a worktree: only an ordinary one whose branch
/// was merged into origin/main ([`Tip::integration`]) and which holds no work.
fn classify(facts: &Facts) -> Decision {
    let reason = match facts.role {
        Role::Main => Reason::MainCheckout,
        Role::Current => Reason::Current,
        Role::Other if facts.locked => Reason::Locked,
        Role::Other if facts.missing => Reason::Missing,
        Role::Other => match (Integration::of(facts.tip), &facts.changes) {
            (_, Changes::Work) => Reason::Dirty,
            (_, Changes::Ignored(paths)) => Reason::Ignored(paths.clone()),
            (Integration::Detached, _) => Reason::Detached,
            (Integration::Unstarted { .. }, _) => Reason::Unstarted,
            (
                Integration::Unmerged {
                    ahead,
                    upstream_gone,
                },
                _,
            ) => Reason::Unmerged {
                ahead,
                upstream_gone,
            },
            (Integration::Merged { .. }, changes) => {
                return Decision::Remove {
                    force: *changes == Changes::TasksOnly,
                };
            }
        },
    };
    Decision::Keep(reason)
}

/// A worktree with what git and the file system say about it.
#[derive(Debug)]
struct Worktree {
    entry: Entry,
    facts: Facts,
    outside_root: bool,
    target_bytes: Option<u64>,
}

impl fmt::Display for Worktree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.entry.path.display())?;
        match self.facts.role {
            Role::Main => f.write_str("  (main checkout)")?,
            Role::Current => f.write_str("  (current)")?,
            Role::Other => {}
        }
        if self.outside_root {
            write!(f, "  (outside {ROOT_DIR})")?;
        }
        let branch = self.entry.branch.as_deref().unwrap_or("(detached)");
        write!(f, "\n    branch: {branch}")?;
        let state = match Integration::of(self.facts.tip) {
            Integration::Merged {
                upstream_gone: false,
            } => format!("merged into {BASE}"),
            Integration::Merged {
                upstream_gone: true,
            } => format!("merged into {BASE}, branch gone on origin"),
            Integration::Unstarted {
                upstream_gone: false,
            } => format!("at a {BASE} commit, nothing merged"),
            Integration::Unstarted {
                upstream_gone: true,
            } => format!("at a {BASE} commit, nothing merged, branch gone on origin"),
            Integration::Unmerged {
                ahead,
                upstream_gone: false,
            } => format!("{ahead} unmerged commit(s)"),
            Integration::Unmerged {
                ahead,
                upstream_gone: true,
            } => format!("branch gone on origin, {ahead} commit(s) not on {BASE}"),
            Integration::Detached => "detached".to_owned(),
        };
        write!(f, "  state: {state}")?;
        let changes = if self.facts.missing {
            "directory missing"
        } else {
            match self.facts.changes {
                Changes::None | Changes::TasksOnly => "clean",
                Changes::Ignored(_) => "clean, ignored files",
                Changes::Work => "dirty",
            }
        };
        write!(f, "  {changes}")?;
        if self.entry.locked {
            f.write_str("  locked")?;
        }
        if let Some(bytes) = self.target_bytes {
            write!(f, "  target/: {}", human_size(bytes))?;
        }
        Ok(())
    }
}

/// Creates `branch` from a fresh origin/main in `<root>/<slug>`.
///
/// `--no-track`: an upstream of origin/main would make a bare `git pull` merge
/// main and a later `git push` aim at it; the branch gets its own upstream on
/// its first `git push -u`.
fn new(git: &Git, branch: &BranchName) -> anyhow::Result<PathBuf> {
    let (main, _) = git.worktrees()?;
    let root = main.join(ROOT_DIR);
    let path = root.join(branch.slug());
    ensure!(!path.exists(), "{} already exists", path.display());
    ensure!(
        !git.succeeds(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}")
        ])?,
        "branch `{branch}` already exists"
    );
    git.run(&["fetch", "origin", "main"])?;
    std::fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
    git.run(&[
        "worktree",
        "add",
        "--no-track",
        "-b",
        &branch.to_string(),
        utf8(&path)?,
        BASE,
    ])?;
    Ok(path)
}

/// Every worktree git records, with its facts.
fn survey(git: &Git) -> anyhow::Result<Vec<Worktree>> {
    let (main, entries) = git.worktrees()?;
    let root = main.join(ROOT_DIR);
    let upstreams = parse_upstreams(&git.run(&[
        "for-each-ref",
        "--format=%(refname)%00%(upstream)",
        "refs/heads",
        "refs/remotes",
    ])?);
    let first_parent: HashSet<String> = git
        .run(&["rev-list", "--first-parent", BASE])?
        .lines()
        .map(str::to_owned)
        .collect();
    let mut worktrees = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        if entry.bare {
            continue;
        }
        let role = if index == 0 {
            Role::Main
        } else if same_dir(&entry.path, &git.dir) {
            Role::Current
        } else {
            Role::Other
        };
        let missing = entry.prunable || !entry.path.is_dir();
        let tip = match &entry.branch {
            None => None,
            Some(branch) => {
                let tip = format!("refs/heads/{branch}");
                let sha = git.run(&["rev-parse", "--verify", &tip])?;
                let ahead = git.run(&["rev-list", "--count", &format!("{BASE}..{tip}")])?;
                Some(Tip {
                    reachable: git.succeeds(&["merge-base", "--is-ancestor", &tip, BASE])?,
                    first_parent: first_parent.contains(sha.trim()),
                    ahead: ahead
                        .trim()
                        .parse()
                        .context("`git rev-list --count` output")?,
                    upstream_gone: upstreams.get(branch).copied().unwrap_or(false),
                })
            }
        };
        let changes = if missing {
            Changes::None
        } else {
            Changes::from_status(&Git::new(entry.path.clone()).run(&[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=normal",
                "--ignored=matching",
            ])?)
        };
        let outside_root = role != Role::Main
            && !entry
                .path
                .parent()
                .is_some_and(|parent| same_dir(parent, &root));
        let target = entry.path.join("target");
        let target_bytes = target.is_dir().then(|| tree_size(&target));
        worktrees.push(Worktree {
            facts: Facts {
                role,
                tip,
                changes,
                locked: entry.locked,
                missing,
            },
            entry,
            outside_root,
            target_bytes,
        });
    }
    Ok(worktrees)
}

/// What `prune` did, one line per worktree.
#[derive(Debug, Default)]
struct PruneReport {
    lines: Vec<String>,
    failed: bool,
}

/// Fetches, drops stale records, then removes every worktree [`classify`]
/// lets go; with `dry_run` fetches nothing and only reports.
fn prune(git: &Git, dry_run: bool) -> anyhow::Result<PruneReport> {
    let mut report = PruneReport::default();
    if dry_run {
        report.lines.push(format!(
            "dry run: not fetched; judged against the local {BASE}, which may be stale"
        ));
    } else {
        git.run(&["fetch", "--prune", "origin"])?;
    }
    let stale = if dry_run {
        git.run(&["worktree", "prune", "--dry-run", "--verbose"])?
    } else {
        git.run(&["worktree", "prune", "--verbose"])?
    };
    report
        .lines
        .extend(stale.lines().map(|line| format!("stale: {line}")));
    for worktree in survey(git)? {
        let path = worktree.entry.path.display().to_string();
        match classify(&worktree.facts) {
            Decision::Keep(reason) => report.lines.push(format!("kept: {path}: {reason}")),
            Decision::Remove { .. } if dry_run => {
                report.lines.push(format!("would remove: {path}"));
            }
            Decision::Remove { force } => match remove(git, &worktree, force) {
                Ok(removed) => {
                    report.lines.push(format!(
                        "removed: {path} (target/ {})",
                        human_size(removed.freed)
                    ));
                    if let Some(refusal) = removed.branch_kept {
                        report.lines.push(format!("  branch kept: {refusal}"));
                    }
                }
                Err(error) => {
                    report.failed = true;
                    report.lines.push(format!("failed: {path}: {error:#}"));
                }
            },
        }
    }
    Ok(report)
}

/// What [`remove`] did.
#[derive(Debug)]
struct Removed {
    /// The bytes `target/` held.
    freed: u64,
    /// Why the branch was left: `git branch -d` refused it.
    branch_kept: Option<String>,
}

/// Deletes the worktree's `target/`, the worktree, then its branch. Only for
/// a worktree [`classify`] decided to remove.
///
/// The branch goes only through `git branch -d`, never `-D`: the merge verdict
/// predates this call, and a commit made on the branch since must not be lost.
fn remove(git: &Git, worktree: &Worktree, force: bool) -> anyhow::Result<Removed> {
    let path = &worktree.entry.path;
    let target = path.join("target");
    if target.is_dir() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("removing {}", target.display()))?;
    }
    // The survey's verdict may be stale. `git worktree remove` refuses
    // untracked and modified files without `--force` (never passed), but deletes
    // ignored ones silently, so recheck everything, ignored entries included, and
    // remove only what the verdict allowed: nothing, or a lone `TASKS.md`.
    let expected = if force {
        Changes::TasksOnly
    } else {
        Changes::None
    };
    let now = Changes::from_status(&Git::new(path.clone()).run(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignored=matching",
    ])?);
    if now != expected {
        let reason = match now {
            Changes::Ignored(paths) => Reason::Ignored(paths),
            Changes::None | Changes::TasksOnly | Changes::Work => Reason::Dirty,
        };
        bail!(
            "{} changed since it was surveyed ({reason}); keeping it",
            path.display()
        );
    }
    if force {
        std::fs::remove_file(path.join(TASKS_FILE))
            .with_context(|| format!("removing {}", path.join(TASKS_FILE).display()))?;
    }
    git.run(&["worktree", "remove", utf8(path)?])?;
    let branch_kept = match &worktree.entry.branch {
        Some(branch) => git.run(&["branch", "-d", branch]).err().map(|error| {
            format!("{branch}: {error:#}; check it, then `git branch -D` it yourself")
        }),
        None => None,
    };
    Ok(Removed {
        freed: worktree.target_bytes.unwrap_or(0),
        branch_kept,
    })
}

fn utf8(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .with_context(|| format!("{} is not UTF-8", path.display()))
}

/// `git`, run in one directory.
struct Git {
    dir: PathBuf,
}

impl Git {
    fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn output(&self, args: &[&str]) -> anyhow::Result<std::process::Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .context("spawning `git`")
    }

    /// stdout of a command that must succeed.
    fn run(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = self.output(args)?;
        if !out.status.success() {
            bail!(
                "`git {}` in {} failed ({}): {}",
                args.join(" "),
                self.dir.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        String::from_utf8(out.stdout).context("non-UTF-8 `git` output")
    }

    /// Whether a yes/no command said yes (exit 0) or no (exit 1).
    fn succeeds(&self, args: &[&str]) -> anyhow::Result<bool> {
        let out = self.output(args)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => bail!(
                "`git {}` in {} failed ({}): {}",
                args.join(" "),
                self.dir.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    /// The main checkout ([`main_checkout`]) and every worktree git records,
    /// the main one first and at that path.
    fn worktrees(&self) -> anyhow::Result<(PathBuf, Vec<Entry>)> {
        let mut entries =
            parse_worktrees(&self.run(&["worktree", "list", "--porcelain", "-z"])?)?;
        let main = main_checkout(&entries, self.main_toplevel()?.as_deref())?;
        if let Some(first) = entries.first_mut() {
            first.path.clone_from(&main);
        }
        Ok((main, entries))
    }

    /// `--show-toplevel` when this is the main checkout: its git dir is the
    /// common one.
    fn main_toplevel(&self) -> anyhow::Result<Option<PathBuf>> {
        let out = self.run(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--show-toplevel",
        ])?;
        let mut lines = out.lines();
        let (Some(git_dir), Some(common_dir), Some(toplevel)) =
            (lines.next(), lines.next(), lines.next())
        else {
            bail!("`git rev-parse` printed {out:?}, not three paths");
        };
        Ok(same_dir(Path::new(git_dir), Path::new(common_dir)).then(|| PathBuf::from(toplevel)))
    }
}
