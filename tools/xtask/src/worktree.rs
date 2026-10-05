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
//! [`classify`]); the commands only ask git and act on the verdict. A worktree
//! is removed only when its branch tip is an ancestor of origin/main and
//! nothing in it is modified or untracked and unignored (a root `TASKS.md`
//! aside). Its branch is then deleted with `git branch -d`, falling back to
//! `-D` only because that ancestry was just proven: `-d` judges against the
//! branch's upstream or the current `HEAD`, not origin/main. A squash-merged
//! branch has commits origin/main does not contain, so it is kept.
//!
//! `list` reads the local origin/main and fetches nothing; `new` fetches main
//! and `prune` fetches with `--prune`, so "gone on origin" is current there.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
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
    /// Remove clean worktrees whose branch origin/main contains, with their branches.
    Prune {
        /// Print what would be removed and change nothing.
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

/// The worktree root for the repository whose common git dir is `common_dir`:
/// `.worktrees` in the main checkout that owns it.
fn worktree_root(common_dir: &Path) -> anyhow::Result<PathBuf> {
    let main = common_dir.parent().with_context(|| {
        format!(
            "{} has no main checkout above it to hold {ROOT_DIR}",
            common_dir.display()
        )
    })?;
    Ok(main.join(ROOT_DIR))
}

/// One record of `git worktree list --porcelain`.
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

/// Parses `git worktree list --porcelain`: records separated by blank lines,
/// the first one the main worktree.
fn parse_worktrees(porcelain: &str) -> anyhow::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    for line in porcelain.lines() {
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
            bail!("`git worktree list --porcelain` line before any `worktree`: {line}");
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Changes {
    None,
    /// Only an untracked root `TASKS.md`, which `git worktree remove` needs
    /// `--force` to delete.
    TasksOnly,
    /// Modified tracked files or untracked, unignored ones.
    Work,
}

impl Changes {
    /// Reads `git status --porcelain=v1 -z --untracked-files=normal`.
    fn from_status(status: &str) -> Self {
        let mut tasks = false;
        for record in status.split('\0').filter(|record| !record.is_empty()) {
            if record == format!("?? {TASKS_FILE}") {
                tasks = true;
            } else {
                return Self::Work;
            }
        }
        if tasks { Self::TasksOnly } else { Self::None }
    }
}

/// Where a worktree's commits stand against origin/main.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Integration {
    /// The branch tip is an ancestor of origin/main.
    Merged {
        upstream_gone: bool,
    },
    /// `ahead` commits are not on origin/main.
    Unmerged {
        ahead: u64,
        upstream_gone: bool,
    },
    Detached,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Facts {
    role: Role,
    integration: Integration,
    changes: Changes,
    locked: bool,
    missing: bool,
}

/// What `prune` does with a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// Remove the worktree (with `--force` when only `TASKS.md` stands in the
    /// way) and delete its branch: origin/main contains it.
    Remove {
        force: bool,
    },
    Keep(Reason),
}

/// Why `prune` keeps a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    MainCheckout,
    Current,
    Locked,
    Missing,
    Detached,
    Dirty,
    Unmerged { ahead: u64, upstream_gone: bool },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MainCheckout => f.write_str("the main checkout"),
            Self::Current => f.write_str("the current worktree"),
            Self::Locked => f.write_str("locked (`git worktree unlock` first)"),
            Self::Missing => f.write_str("directory gone; `git worktree prune` drops the record"),
            Self::Detached => f.write_str("detached HEAD, no branch to judge"),
            Self::Dirty => f.write_str("uncommitted or untracked changes"),
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
/// origin/main contains and which holds no work.
fn classify(facts: &Facts) -> Decision {
    let reason = match facts.role {
        Role::Main => Reason::MainCheckout,
        Role::Current => Reason::Current,
        Role::Other if facts.locked => Reason::Locked,
        Role::Other if facts.missing => Reason::Missing,
        Role::Other => match (facts.integration, facts.changes) {
            (_, Changes::Work) => Reason::Dirty,
            (Integration::Detached, _) => Reason::Detached,
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
                    force: changes == Changes::TasksOnly,
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
        let state = match self.facts.integration {
            Integration::Merged {
                upstream_gone: false,
            } => format!("merged into {BASE}"),
            Integration::Merged {
                upstream_gone: true,
            } => format!("merged into {BASE}, branch gone on origin"),
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
    let root = worktree_root(&git.common_dir()?)?;
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
    let root = worktree_root(&git.common_dir()?)?;
    let entries = parse_worktrees(&git.run(&["worktree", "list", "--porcelain"])?)?;
    let upstreams = parse_upstreams(&git.run(&[
        "for-each-ref",
        "--format=%(refname)%00%(upstream)",
        "refs/heads",
        "refs/remotes",
    ])?);
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
        let integration = match &entry.branch {
            None => Integration::Detached,
            Some(branch) => {
                let upstream_gone = upstreams.get(branch).copied().unwrap_or(false);
                let tip = format!("refs/heads/{branch}");
                if git.succeeds(&["merge-base", "--is-ancestor", &tip, BASE])? {
                    Integration::Merged { upstream_gone }
                } else {
                    let ahead = git.run(&["rev-list", "--count", &format!("{BASE}..{tip}")])?;
                    Integration::Unmerged {
                        ahead: ahead
                            .trim()
                            .parse()
                            .context("`git rev-list --count` output")?,
                        upstream_gone,
                    }
                }
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
                integration,
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
/// lets go; with `dry_run` only reports.
fn prune(git: &Git, dry_run: bool) -> anyhow::Result<PruneReport> {
    let mut report = PruneReport::default();
    git.run(&["fetch", "--prune", "origin"])?;
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
                Ok(freed) => report
                    .lines
                    .push(format!("removed: {path} (target/ {})", human_size(freed))),
                Err(error) => {
                    report.failed = true;
                    report.lines.push(format!("failed: {path}: {error:#}"));
                }
            },
        }
    }
    Ok(report)
}

/// Deletes the worktree's `target/`, the worktree, then its branch. Only for
/// a worktree [`classify`] decided to remove. Returns the bytes `target/` held.
fn remove(git: &Git, worktree: &Worktree, force: bool) -> anyhow::Result<u64> {
    let path = &worktree.entry.path;
    let target = path.join("target");
    if target.is_dir() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("removing {}", target.display()))?;
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(utf8(path)?);
    git.run(&args)?;
    if let Some(branch) = &worktree.entry.branch
        && git.run(&["branch", "-d", branch]).is_err()
    {
        // `-d` judges against the upstream or HEAD; classify proved the tip
        // is an ancestor of origin/main, which is what makes `-D` safe here
        git.run(&["branch", "-D", branch])?;
    }
    Ok(worktree.target_bytes.unwrap_or(0))
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

    /// The repository's common git dir, absolute.
    fn common_dir(&self) -> anyhow::Result<PathBuf> {
        let dir = self.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        Ok(PathBuf::from(dir.trim()))
    }
}
