//! Path classification and the workspace dependency graph.
//!
//! The four modes:
//!
//! - `docs`: every changed file is documentation ([`DOCS_ONLY`]); nothing compiles.
//! - `none`: only repository tooling the `checks` job runs itself, or a
//!   standalone crate (its own `[workspace]`, outside every member's graph);
//!   nothing in the workspace compiles.
//! - `packages`: the changed packages plus every workspace package declaring a
//!   dependency on them (normal, build, optional, target-specific; transitively),
//!   and then the dev-dependents of all of those. A dev edge is the last hop: the
//!   dev-dependent's tests link the change, its library does not.
//! - `full`: something every package depends on changed, or a file nobody here
//!   can attribute: the whole workspace.
//!
//! `heavy_required` is set when a changed file is an input only the wide
//! lane's jobs exercise ([`HEAVY_TRIGGERS`], or an xtask command only such a
//! job runs): the PR then runs the wide lane.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, bail};
use regex::Regex;
use serde::Deserialize;

use super::aggregator::WIDE_CONDITION;

/// Documentation: a change made only of these compiles nothing.
///
/// Crate `README.md` files are deliberately NOT here: several are pulled into
/// doctests with `#[doc = include_str!(...)]` (`cargo xtask paths-filter`
/// proves no `include_str!` target falls inside these patterns). Nothing under
/// `tools/xtask/` is documentation either: its markdown fixtures are test
/// inputs of the `xtask` package.
pub(super) const DOCS_ONLY: &[&str] = &[
    "*.md", // root-level only: `*` does not cross `/` (see `matches`)
    "docs/**",
    "design/**",
    "book/**",
    ".github/**/*.md",
    ".github/ISSUE_TEMPLATE/**",
    ".github/CODEOWNERS",
    ".editorconfig",
    "crates/*/ARCHITECTURE.md",
    "crates/*/CHANGELOG.md",
    // the root license texts' per-crate copies, shipped in each package
    "crates/*/LICENSE*",
    "crates/*/NOTICE",
    "packages/*/ARCHITECTURE.md",
    "packages/*/CHANGELOG.md",
    "packages/*/LICENSE*",
    "packages/*/NOTICE",
    "changelog.d/**", // changelog fragments; `changelog --check` in `checks` judges them
];

/// Inputs whose breakage only the wide lane's jobs block on (feature-matrix,
/// wasm, cross, doc, miri; and `deps`' advisories): the wide lane runs.
pub(super) const HEAVY_TRIGGERS: &[&str] = &[
    "Cargo.toml", // the root manifest: workspace deps, lints, profiles
    "Cargo.lock",
    ".cargo/**",
    "rust-toolchain.toml",
    "rust-toolchain", // the extensionless form; rustup prefers it over .toml
    ".github/workflows/**",
    // Shaders: clippy only embeds them as strings and no build script parses
    // them, so the fast lane never compiles one (`checks`' `wgsl` step is a
    // syntactic uniformity check, in every lane). The wide lane adds
    // live-smoke, whose demo compiles the pipelines it draws with on
    // lavapipe; gpu-test, which compiles every shader module on WARP, runs
    // only from the full lane up (merge queue, main), so a shader the demo
    // does not draw is first compiled there.
    "**/*.wgsl",
    // The `deps` job's advisories step blocks only from the wide lane up, and an
    // edited advisory ignore is exactly what that step judges.
    "deny.toml",
];

/// Everything depends on these, and the fast lane checks what they change.
pub(super) const FULL_TRIGGERS: &[&str] = &[
    "clippy.toml",
    ".config/nextest.toml",
    ".config/insta.yaml", // every snapshot assertion reads it
    // The lane machinery itself: a PR can change its own classification, so it
    // gets the whole workspace rather than the scope it would compute. That is
    // this module, the entry point that dispatches to it and the helpers it
    // runs through.
    "tools/xtask/src/change_scope.rs",
    "tools/xtask/src/change_scope/**",
    "tools/xtask/src/main.rs",
    "tools/xtask/src/util.rs",
];

/// Repository files no package compiles: configuration the `checks` job reads
/// itself, and the macOS/iOS device-check drivers, which only run by hand on a
/// Mac. `tools/xtask/` is not here: it is the `xtask` workspace package, so a
/// change to it scopes that package (its clippy and unit tests) like any other
/// crate, and [`heavy_job_inputs`] adds the wide lane when a wide-lane job
/// runs the command it touches. A standalone crate (see [`standalone_root`])
/// counts as tooling too.
pub(super) const TOOLING: &[&str] = &[
    "tools/device-checks/**",
    "typos.toml",
    ".taplo.toml",
    "rustfmt.toml",
    "gamma.toml", // cargo-gamma, run by hand (docs/testing.md "Mutation Testing")
    ".gitignore",
    ".gitattributes",
    "llms.txt",
    "LICENSE*",
    "NOTICE*",
    ".github/dependabot.yml",
];

/// `fnmatch`-style match: `*` is any run of characters (including `/`), `?` is
/// one character, everything else is literal. No pattern here uses a bracket
/// class, so none is supported.
fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let (mut n, mut p) = (0, 0);
    // The last `*` seen and the name position it was tried against.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                n += 1;
                p += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    star = Some((sp, sn + 1));
                    p = sp + 1;
                    n = sn + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Glob match where `*` stays inside one path segment and `**` spans any.
pub(super) fn matches(path: &str, pattern: &str) -> bool {
    if !pattern.contains('/') && !pattern.contains("**") {
        return !path.contains('/') && fnmatch(path, pattern);
    }
    if let Some(dir) = pattern.strip_suffix("**").filter(|d| d.ends_with('/')) {
        return path.starts_with(dir);
    }
    if let Some((head, tail)) = pattern.split_once("**") {
        let last = path.rsplit('/').next().unwrap_or(path);
        return path.starts_with(head) && fnmatch(last, tail.trim_start_matches('/'));
    }
    path.split('/').count() == pattern.split('/').count() && fnmatch(path, pattern)
}

fn matches_any(path: &str, patterns: &[impl AsRef<str>]) -> bool {
    patterns.iter().any(|p| matches(path, p.as_ref()))
}

/// Whether `path` (repo-relative, `/`-separated) is documentation only.
pub(super) fn is_docs_only(path: &str) -> bool {
    matches_any(path, DOCS_ONLY)
}

/// clap's name for a `Command` variant: `DocStrict` -> `doc-strict`.
fn kebab_case(variant: &str) -> String {
    let mut out = String::with_capacity(variant.len() + 4);
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Repo files the wide lane's jobs run but `checks` does not, read from ci.yml,
/// as patterns for [`matches()`]: for each `cargo xtask <command>` a job gated
/// on the wide lane (`if:` [`WIDE_CONDITION`]) runs, the module implementing it (from
/// the dispatch in `xtask_main`, xtask's `main.rs`) plus the xtask entry point,
/// helpers and manifest every command runs through. A command the dispatch
/// does not name makes all of `tools/xtask/` an input. A command a YAML
/// comment merely mentions runs nothing, so comment lines are skipped.
pub(super) fn heavy_job_inputs(ci_yml: &str, xtask_main: Option<&str>) -> BTreeSet<String> {
    let runs_xtask = Regex::new(r"cargo xtask ([a-z0-9][a-z0-9-]*)").expect("BUG: valid regex");
    let dispatch = Regex::new(r"Command::([A-Za-z0-9]+)\(\w+\)\s*=>\s*([a-z_][a-z0-9_]*)::")
        .expect("BUG: valid regex");
    let modules: BTreeMap<String, String> = xtask_main
        .map(|text| {
            dispatch
                .captures_iter(text)
                .map(|c| (kebab_case(&c[1]), c[2].to_owned()))
                .collect()
        })
        .unwrap_or_default();

    let gate = format!("\n    if: {WIDE_CONDITION}\n");
    let mut found = BTreeSet::new();
    for job in jobs(ci_yml) {
        if !job.contains(&gate) {
            continue;
        }
        let lines = job
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'));
        for command in lines.flat_map(|line| runs_xtask.captures_iter(line)) {
            found.extend(
                [
                    "tools/xtask/src/main.rs",
                    "tools/xtask/src/util.rs",
                    "tools/xtask/Cargo.toml",
                ]
                .map(str::to_owned),
            );
            match modules.get(&command[1]) {
                Some(module) => {
                    found.insert(format!("tools/xtask/src/{module}.rs"));
                    found.insert(format!("tools/xtask/src/{module}/**"));
                }
                None => {
                    found.insert("tools/xtask/**".to_owned());
                }
            }
        }
    }
    found
}

/// The text of each job under ci.yml's `jobs:`, split at the two-space-indented
/// `name:` lines (the text before the first job is included; it names no gate).
fn jobs(ci_yml: &str) -> Vec<&str> {
    let body = ci_yml
        .split_once("\njobs:\n")
        .map_or(ci_yml, |(_, jobs)| jobs);
    let job_start = Regex::new(r"^  [A-Za-z0-9_-]+:\s*$").expect("BUG: valid regex");
    let mut out = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if offset > 0 && job_start.is_match(line.trim_end_matches('\n')) {
            out.push(&body[start..offset]);
            start = offset;
        }
        offset += line.len();
    }
    out.push(&body[start..]);
    out
}

/// Reads a repo file with `\r\n` normalised to `\n`, `None` when it is absent.
pub(super) fn read_normalised(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.replace("\r\n", "\n"))
}

/// A workspace package, as `cargo metadata --no-deps` describes it.
#[derive(Debug, Deserialize)]
pub(super) struct Package {
    pub(super) name: String,
    manifest_path: PathBuf,
    pub(super) dependencies: Vec<Dependency>,
    pub(super) targets: Vec<Target>,
    pub(super) features: BTreeMap<String, Vec<String>>,
    /// `[package.metadata]`, `null` when the manifest has none.
    #[serde(default)]
    metadata: serde_json::Value,
}

impl Package {
    /// Whether the package builds for wasm32: yes, unless its manifest says
    /// `[package.metadata.flui] wasm = false`.
    pub(super) fn builds_for_wasm(&self) -> bool {
        self.metadata
            .pointer("/flui/wasm")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    }
}

/// A declared dependency of a workspace package.
#[derive(Debug, Deserialize)]
pub(super) struct Dependency {
    pub(super) name: String,
    /// `None` for a normal dependency, else `dev` or `build`.
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) optional: bool,
    pub(super) rename: Option<String>,
    path: Option<PathBuf>,
}

/// A build target of a workspace package.
#[derive(Debug, Deserialize)]
pub(super) struct Target {
    pub(super) kind: Vec<String>,
    src_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

/// The workspace packages, which paths each owns, and who depends on whom.
#[derive(Debug)]
pub(super) struct Workspace {
    /// Package name -> its metadata.
    pub(super) packages: BTreeMap<String, Package>,
    /// Package name -> the path prefixes it owns, in metadata order.
    owned: Vec<(String, Vec<String>)>,
    /// Package name -> the workspace packages declaring a normal or build
    /// dependency on it: their libraries contain it, so their dependents are
    /// affected in turn.
    dependents: BTreeMap<String, BTreeSet<String>>,
    /// Package name -> the workspace packages declaring it as a
    /// dev-dependency: their tests link it, their libraries do not, so the
    /// change stops there (unless they also have a normal edge to it).
    dev_dependents: BTreeMap<String, BTreeSet<String>>,
}

/// `path` relative to `root`, `/`-separated; `None` outside it.
fn relative(path: &Path, root: &Path) -> Option<String> {
    let rel = path
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let (path, root) = (path.canonicalize().ok()?, root.canonicalize().ok()?);
            path.strip_prefix(root).ok().map(Path::to_path_buf)
        })?;
    let parts: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}

impl Workspace {
    /// Runs `cargo metadata --no-deps --offline` in `root`.
    ///
    /// Edges come from the DECLARED dependencies: optional and target-specific
    /// edges included, which the resolved graph omits when their feature or
    /// target is not active.
    fn load(root: &Path) -> anyhow::Result<Self> {
        let out = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .current_dir(root)
            .output()
            .context("spawning `cargo metadata`")?;
        if !out.status.success() {
            bail!(
                "`cargo metadata` failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let meta: Metadata =
            serde_json::from_slice(&out.stdout).context("parsing `cargo metadata` output")?;
        Ok(Self::from_packages(root, meta.packages))
    }

    fn from_packages(root: &Path, packages: Vec<Package>) -> Self {
        let names: BTreeSet<String> = packages.iter().map(|p| p.name.clone()).collect();
        let mut owned = Vec::new();
        let mut dependents: BTreeMap<String, BTreeSet<String>> =
            names.iter().map(|n| (n.clone(), BTreeSet::new())).collect();
        let mut dev_dependents = dependents.clone();
        for p in &packages {
            let dir = p
                .manifest_path
                .parent()
                .and_then(|d| relative(d, root))
                .unwrap_or_else(|| ".".to_owned());
            if dir == "." {
                // The root package owns its targets' directories, not the repo.
                let mut prefixes: BTreeSet<String> =
                    ["src/", "tests/", "benches/", "build.rs", "Cargo.toml"]
                        .map(str::to_owned)
                        .into();
                for t in &p.targets {
                    let Some(rel) = relative(&t.src_path, root) else {
                        continue;
                    };
                    let d = rel.rsplit_once('/').map_or("", |(d, _)| d);
                    prefixes.insert(
                        if matches!(d, "" | "examples" | "src" | "tests" | "benches") {
                            rel.clone()
                        } else {
                            format!("{d}/")
                        },
                    );
                }
                owned.push((p.name.clone(), prefixes.into_iter().collect()));
            } else {
                owned.push((p.name.clone(), vec![format!("{dir}/")]));
            }
            for d in &p.dependencies {
                if names.contains(&d.name) && d.name != p.name && d.path.is_some() {
                    let map = if d.kind.as_deref() == Some("dev") {
                        &mut dev_dependents
                    } else {
                        &mut dependents
                    };
                    map.entry(d.name.clone())
                        .or_default()
                        .insert(p.name.clone());
                }
            }
        }
        let packages = packages.into_iter().map(|p| (p.name.clone(), p)).collect();
        Self {
            packages,
            owned,
            dependents,
            dev_dependents,
        }
    }

    /// `seeds` plus every package a change to them can affect: the packages
    /// depending on them through normal and build edges, transitively, and
    /// then the dev-dependents of all of those. A dev-dependent is the last
    /// hop, because its library does not contain the dependency.
    fn affected(&self, seeds: &BTreeSet<String>) -> BTreeSet<String> {
        let mut scope = seeds.clone();
        let mut todo: Vec<String> = seeds.iter().cloned().collect();
        while let Some(pkg) = todo.pop() {
            for user in self.dependents.get(&pkg).into_iter().flatten() {
                if scope.insert(user.clone()) {
                    todo.push(user.clone());
                }
            }
        }
        let tests: Vec<String> = scope
            .iter()
            .flat_map(|pkg| self.dev_dependents.get(pkg).into_iter().flatten())
            .cloned()
            .collect();
        scope.extend(tests);
        scope
    }

    /// The package owning `path`: the longest matching prefix wins.
    fn owning_package(&self, path: &str) -> Option<&str> {
        let mut best: Option<(&str, usize)> = None;
        for (name, prefixes) in &self.owned {
            for pre in prefixes {
                let hit = path == pre || (pre.ends_with('/') && path.starts_with(pre.as_str()));
                if hit && best.is_none_or(|(_, len)| pre.len() > len) {
                    best = Some((name, pre.len()));
                }
            }
        }
        best.map(|(name, _)| name)
    }
}

/// The repository a change is scoped against: its root, the ci.yml and xtask
/// dispatch it reads, and (loaded on first use) its workspace graph.
#[derive(Debug)]
pub(super) struct Repo {
    pub(super) root: PathBuf,
    /// `.github/workflows/ci.yml`, `\r\n`-normalised; `None` when absent.
    pub(super) ci_yml: Option<String>,
    xtask_main: Option<String>,
    workspace: OnceLock<Workspace>,
}

impl Repo {
    /// Opens the repository at `root`.
    pub(super) fn open(root: PathBuf) -> Self {
        let ci_yml = read_normalised(&root.join(".github/workflows/ci.yml"));
        let xtask_main = read_normalised(&root.join("tools/xtask/src/main.rs"));
        Self {
            root,
            ci_yml,
            xtask_main,
            workspace: OnceLock::new(),
        }
    }

    /// The workspace graph, from `cargo metadata` on first use.
    pub(super) fn workspace(&self) -> anyhow::Result<&Workspace> {
        if let Some(ws) = self.workspace.get() {
            return Ok(ws);
        }
        let ws = Workspace::load(&self.root)?;
        Ok(self.workspace.get_or_init(|| ws))
    }

    fn git(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .context("spawning `git`")?;
        if !out.status.success() {
            bail!(
                "`git {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        String::from_utf8(out.stdout).context("non-UTF-8 `git` output")
    }

    /// The files changed since the merge base with `base`, plus (with
    /// `worktree`) staged, unstaged and untracked work, for `cargo xtask check-changed`.
    pub(super) fn changed_files(&self, base: &str, worktree: bool) -> anyhow::Result<Vec<String>> {
        // --no-renames: a move reports BOTH the old and the new path, so the
        // crate a file left is in scope too, not only the crate it arrived in.
        let merge_base = self.git(&["merge-base", base, "HEAD"])?;
        let range = format!("{}..HEAD", merge_base.trim());
        let mut files: BTreeSet<String> = BTreeSet::new();
        let mut add = |out: String| {
            files.extend(out.split('\0').filter(|f| !f.is_empty()).map(str::to_owned));
        };
        add(self.git(&["diff", "--name-only", "--no-renames", "-z", &range])?);
        if worktree {
            add(self.git(&["diff", "--name-only", "--no-renames", "-z", "HEAD"])?);
            add(self.git(&["ls-files", "--others", "--exclude-standard", "-z"])?);
        }
        Ok(files.into_iter().collect())
    }
}

/// How much of the workspace a change reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Docs,
    None,
    Packages,
    Full,
}

impl Mode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::None => "none",
            Self::Packages => "packages",
            Self::Full => "full",
        }
    }
}

/// The classification of a change.
#[derive(Debug, Clone)]
pub(super) struct Scope {
    pub(super) mode: Mode,
    /// The whole scope, sorted: seeds plus their transitive dependents.
    pub(super) packages: Vec<String>,
    /// The packages a changed file belongs to.
    pub(super) seeds: Vec<String>,
    /// Packages whose `Cargo.toml` changed.
    pub(super) manifests: Vec<String>,
    /// Standalone crate directories a changed file belongs to (see
    /// [`standalone_root`]), sorted.
    pub(super) standalone: Vec<String>,
    pub(super) heavy_required: bool,
    pub(super) reason: String,
}

impl Scope {
    fn new(mode: Mode, reason: String) -> Self {
        Self {
            mode,
            packages: Vec::new(),
            seeds: Vec::new(),
            manifests: Vec::new(),
            standalone: Vec::new(),
            heavy_required: false,
            reason,
        }
    }

    /// The whole workspace, no diff (the wide, full and extended lanes).
    pub(super) fn whole_workspace() -> Self {
        Self::new(Mode::Full, "the whole workspace".to_owned())
    }
}

/// The standalone crate `file` (repo-relative) belongs to: the nearest
/// ancestor directory below the repository root whose `Cargo.toml` declares
/// its own `[workspace]`, provided no workspace package has a path dependency
/// into it. Such a crate is outside every member's graph, so a change to it
/// compiles nothing in the workspace. `None` when the nearest such manifest is
/// reached by a path dependency, or there is none (a deleted crate leaves no
/// manifest: its files stay unowned).
fn standalone_root(root: &Path, ws: &Workspace, file: &str) -> Option<String> {
    let mut dir = file;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        dir = parent;
        let Some(text) = read_normalised(&root.join(dir).join("Cargo.toml")) else {
            continue;
        };
        let has_workspace = text
            .parse::<toml::Table>()
            .is_ok_and(|manifest| manifest.contains_key("workspace"));
        if !has_workspace {
            continue;
        }
        let inside = format!("{dir}/");
        let reached = ws
            .packages
            .values()
            .flat_map(|p| &p.dependencies)
            .filter_map(|d| relative(d.path.as_deref()?, root))
            .any(|path| path == dir || path.starts_with(&inside));
        return (!reached).then(|| dir.to_owned());
    }
    None
}

fn first_five(files: &[&str]) -> String {
    files.iter().take(5).copied().collect::<Vec<_>>().join(", ")
}

/// Classifies the changed `files` (repo-relative, `/`-separated).
pub(super) fn classify(repo: &Repo, files: &[String]) -> anyhow::Result<Scope> {
    let code: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|f| !is_docs_only(f))
        .collect();
    if code.is_empty() {
        let reason = if files.is_empty() {
            "no changes"
        } else {
            "only documentation changed"
        };
        return Ok(Scope::new(Mode::Docs, reason.to_owned()));
    }
    let heavy_inputs: Vec<String> = repo
        .ci_yml
        .as_deref()
        .map(|ci| {
            heavy_job_inputs(ci, repo.xtask_main.as_deref())
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    let heavy: BTreeSet<&str> = code
        .iter()
        .copied()
        .filter(|f| matches_any(f, HEAVY_TRIGGERS) || matches_any(f, &heavy_inputs))
        .collect();
    if !heavy.is_empty() {
        let heavy: Vec<&str> = heavy.into_iter().collect();
        let mut scope = Scope::new(
            Mode::Full,
            format!(
                "input of the wide-lane jobs changed: {}",
                first_five(&heavy)
            ),
        );
        scope.heavy_required = true;
        return Ok(scope);
    }
    let full: Vec<&str> = code
        .iter()
        .copied()
        .filter(|f| matches_any(f, FULL_TRIGGERS))
        .collect();
    if !full.is_empty() {
        return Ok(Scope::new(
            Mode::Full,
            format!("workspace-wide input changed: {}", first_five(&full)),
        ));
    }
    let ws = repo.workspace()?;
    let (mut seeds, mut manifests, mut unknown) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    let mut standalone = BTreeSet::new();
    for f in code {
        if matches_any(f, TOOLING) {
            continue;
        }
        match ws.owning_package(f) {
            Some(pkg) => {
                seeds.insert(pkg.to_owned());
                if f == "Cargo.toml" || f.ends_with("/Cargo.toml") {
                    manifests.insert(pkg.to_owned());
                }
            }
            None => match standalone_root(&repo.root, ws, f) {
                Some(dir) => {
                    standalone.insert(dir);
                }
                None => unknown.push(f),
            },
        }
    }
    if !unknown.is_empty() {
        let reason = format!(
            "no package owns: {} (conservatively: everything)",
            first_five(&unknown)
        );
        return Ok(Scope::new(Mode::Full, reason));
    }
    let standalone: Vec<String> = standalone.into_iter().collect();
    if seeds.is_empty() {
        let reason = if standalone.is_empty() {
            "only repository tooling the checks job runs changed".to_owned()
        } else {
            format!(
                "only repository tooling and standalone crates changed: {} (own [workspace])",
                standalone.join(", ")
            )
        };
        let mut scope = Scope::new(Mode::None, reason);
        scope.standalone = standalone;
        return Ok(scope);
    }
    let scope = ws.affected(&seeds);
    let seed_list: Vec<String> = seeds.into_iter().collect();
    let reason = format!(
        "changed: {}; plus {} dependents",
        seed_list.join(", "),
        scope.len() - seed_list.len()
    );
    Ok(Scope {
        mode: Mode::Packages,
        packages: scope.into_iter().collect(),
        seeds: seed_list,
        manifests: manifests.into_iter().collect(),
        standalone,
        heavy_required: false,
        reason,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The repository this crate is built in, shared across tests so
    /// `cargo metadata` runs once.
    pub(crate) fn repo() -> &'static Repo {
        static REPO: OnceLock<Repo> = OnceLock::new();
        REPO.get_or_init(|| Repo::open(crate::util::repo_root()))
    }

    pub(crate) fn scope(files: &[&str]) -> Scope {
        scope_in(repo(), files)
    }

    fn scope_in(repo: &Repo, files: &[&str]) -> Scope {
        classify(
            repo,
            &files.iter().map(|f| (*f).to_owned()).collect::<Vec<_>>(),
        )
        .expect("classify")
    }

    fn documentation_paths() {
        for path in [
            "README.md",
            "docs/testing.md",
            "design/architecture.md",
            "book/src/intro.md",
            ".github/PULL_REQUEST_TEMPLATE.md",
            "crates/flui-view/ARCHITECTURE.md",
            "crates/flui-view/CHANGELOG.md",
            "packages/flui-material/ARCHITECTURE.md",
            "packages/flui-material/CHANGELOG.md",
            "crates/flui-view/LICENSE",
            "crates/flui-view/LICENSE-APACHE",
            "crates/flui-view/NOTICE",
            "packages/flui-material/LICENSE-APACHE",
            "packages/flui-material/NOTICE",
            "changelog.d/tools-changelog-fragments.md",
            "changelog.d/README.md",
        ] {
            assert!(is_docs_only(path), "{path}");
        }
    }

    fn compiled_markdown_is_not_docs() {
        // crate READMEs are include_str!()'d into doctests; a nested .md is not a root .md
        for path in [
            "crates/flui-animation/README.md",
            "packages/flui-material/README.md",
            "crates/flui-cli/templates/platforms/ios/README.md",
            ".github/workflows/ci.yml",
            "src/lib.rs",
            "tools/xtask/README.md",
            "tools/xtask/fixtures/scope/notes.md",
        ] {
            assert!(!is_docs_only(path), "{path}");
        }
    }

    fn docs_and_empty() {
        assert_eq!(scope(&["docs/a.md", "README.md"]).mode, Mode::Docs);
        // a license sweep over every crate compiles nothing
        assert_eq!(
            scope(&[
                "crates/flui-view/LICENSE",
                "crates/flui-view/NOTICE",
                "packages/flui-material/LICENSE-APACHE",
                "packages/flui-material/NOTICE",
            ])
            .mode,
            Mode::Docs
        );
        assert_eq!(scope(&[]).mode, Mode::Docs);
        assert_eq!(scope(&[]).reason, "no changes");
    }

    fn shaders_require_the_heavy_lane() {
        assert!(scope(&["crates/flui-engine/src/shaders/rect_instanced.wgsl"]).heavy_required);
    }

    fn heavy_inputs_require_the_heavy_lane() {
        for path in [
            "Cargo.lock",
            "Cargo.toml",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "rust-toolchain",
            ".github/workflows/ci.yml",
            "deny.toml",
        ] {
            let s = scope(&[path]);
            assert_eq!((s.mode, s.heavy_required), (Mode::Full, true), "{path}");
        }
    }

    fn inputs_only_a_heavy_job_runs_require_the_heavy_lane() {
        // read out of ci.yml, not restated
        let inputs = heavy_job_inputs(
            repo().ci_yml.as_deref().expect("ci.yml"),
            repo().xtask_main.as_deref(),
        );
        assert!(
            !inputs.is_empty(),
            "the wide lane's jobs run no xtask command?"
        );
        for path in inputs.iter().filter(|p| !p.contains('*')) {
            assert!(scope(&[path]).heavy_required, "{path}");
        }
    }

    fn heavy_job_inputs_from_a_workflow() {
        let ci = format!(
            "on: push\njobs:\n  checks:\n    runs-on: x\n    steps:\n      - run: cargo xtask checks\n\
             \x20 doc:\n    if: {WIDE_CONDITION}\n    steps:\n\
             \x20     # see cargo xtask device ios-sim\n\
             \x20     - run: cargo xtask doc-strict --all\n      - run: cargo xtask not-a-command\n\
             \x20 platform:\n    if: {}\n    steps:\n      - run: cargo xtask device windows-a11y\n",
            super::super::aggregator::FULL_CONDITION
        );
        let main = "match c {\n    Command::Checks(args) => tasks::checks(&args),\n    \
                    Command::DocStrict(args) => doc_strict::doc_strict(&args),\n    \
                    Command::Device(args) => device::device(&args),\n}\n";
        let inputs = heavy_job_inputs(&ci, Some(main));
        let expected = [
            "tools/xtask/**",
            "tools/xtask/Cargo.toml",
            "tools/xtask/src/doc_strict.rs",
            "tools/xtask/src/doc_strict/**",
            "tools/xtask/src/main.rs",
            "tools/xtask/src/util.rs",
        ];
        assert_eq!(
            inputs.iter().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        assert!(matches_any(
            "tools/xtask/src/doc_strict/flags.rs",
            &expected
        ));
        // a job not gated on the wide lane (checks, or a platform job only
        // `full` runs) contributes nothing, nor does a command a comment
        // merely mentions
        assert!(!inputs.contains("tools/xtask/src/tasks.rs"));
        assert!(!inputs.contains("tools/xtask/src/device.rs"));
        assert_eq!(kebab_case("WasmTestCrates"), "wasm-test-crates");
    }

    fn lane_machinery_gets_the_whole_workspace() {
        // xtask's dispatch and shared helpers run inside the wide lane's jobs
        // too (feature-matrix, doc, wasm-check call `cargo xtask`), so a change
        // to them needs the wide lane as well as the whole workspace.
        for path in ["tools/xtask/src/main.rs", "tools/xtask/src/util.rs"] {
            let s = scope(&[path]);
            assert_eq!((s.mode, s.heavy_required), (Mode::Full, true), "{path}");
        }
        // Named triggers, not merely unowned files: a later TOOLING pattern or
        // package that covers `.config/` cannot take them out of full scope.
        for path in [
            "tools/xtask/src/change_scope.rs",
            "tools/xtask/src/change_scope/classify.rs",
            "clippy.toml",
            ".config/nextest.toml",
            ".config/insta.yaml",
        ] {
            let s = scope(&[path]);
            assert_eq!((s.mode, s.heavy_required), (Mode::Full, false), "{path}");
            assert!(
                s.reason.starts_with("workspace-wide input changed: "),
                "{path}: {}",
                s.reason
            );
        }
    }

    fn xtask_changes_scope_the_xtask_package() {
        // Modules only `checks` runs (fonts) and fixtures stay in the fast
        // lane; xtask's manifest is a heavy-lane input like its dispatch.
        assert!(scope(&["tools/xtask/Cargo.toml"]).heavy_required);
        for path in [
            "tools/xtask/src/fonts.rs",
            "tools/xtask/fixtures/scope/notes.md",
        ] {
            let s = scope(&[path]);
            assert_eq!(
                (s.mode, s.packages.as_slice()),
                (Mode::Packages, ["xtask".to_owned()].as_slice()),
                "{path}"
            );
        }
    }

    fn checks_only_tooling_compiles_nothing() {
        assert_eq!(
            scope(&[
                "tools/device-checks/check-macos-a11y.py",
                "typos.toml",
                "gamma.toml",
            ])
            .mode,
            Mode::None
        );
    }

    fn unattributable_file_is_full() {
        let s = scope(&["some-new-dir/thing.txt"]);
        assert_eq!(s.mode, Mode::Full);
        assert_eq!(
            s.reason,
            "no package owns: some-new-dir/thing.txt (conservatively: everything)"
        );
    }

    /// A throwaway repository holding `tools/spike`, a crate with its own
    /// `[workspace]`, beside the workspace `packages` returns for its root.
    fn with_a_standalone_crate(packages: impl FnOnce(&Path) -> Vec<Package>) -> (TempRepo, Repo) {
        let tmp = TempRepo::new();
        std::fs::create_dir_all(tmp.0.join("tools/spike/src")).expect("mkdir");
        std::fs::write(
            tmp.0.join("tools/spike/Cargo.toml"),
            "[package]\nname = \"spike\"\n\n[workspace]\n",
        )
        .expect("write");
        let repo = Repo {
            root: tmp.0.clone(),
            ci_yml: None,
            xtask_main: None,
            workspace: OnceLock::from(Workspace::from_packages(&tmp.0, packages(&tmp.0))),
        };
        (tmp, repo)
    }

    fn a_standalone_crate_is_tooling() {
        let (_tmp, repo) = with_a_standalone_crate(|_| Vec::new());
        let s = scope_in(
            &repo,
            &["tools/spike/Cargo.toml", "tools/spike/src/main.rs"],
        );
        assert_eq!(
            (s.mode, s.standalone.as_slice()),
            (Mode::None, ["tools/spike".to_owned()].as_slice())
        );
        assert!(
            s.reason.contains("tools/spike") && s.reason.contains("[workspace]"),
            "{}",
            s.reason
        );
    }

    fn a_standalone_crate_beside_a_member_change_is_not_unowned() {
        let (_tmp, repo) = with_a_standalone_crate(|root| {
            vec![
                serde_json::from_value(serde_json::json!({
                    "name": "member",
                    "manifest_path": root.join("member/Cargo.toml"),
                    "dependencies": [],
                    "targets": [],
                    "features": {},
                }))
                .expect("a package"),
            ]
        });
        let s = scope_in(&repo, &["tools/spike/src/main.rs", "member/src/lib.rs"]);
        assert_eq!(s.mode, Mode::Packages, "{}", s.reason);
        assert_eq!(s.standalone, ["tools/spike"]);
        assert_eq!(s.packages, ["member"]);
    }

    fn a_crate_a_member_depends_on_is_not_standalone() {
        let tmp = TempRepo::new();
        std::fs::create_dir_all(tmp.0.join("sub/src")).expect("mkdir");
        std::fs::write(
            tmp.0.join("sub/Cargo.toml"),
            "[package]\nname = \"sub\"\n\n[workspace]\n",
        )
        .expect("write");
        let member = |path: Option<PathBuf>| -> Package {
            serde_json::from_value(serde_json::json!({
                "name": "member",
                "manifest_path": tmp.0.join("member/Cargo.toml"),
                "dependencies": [{
                    "name": "sub", "kind": null, "rename": null, "optional": false, "path": path,
                }],
                "targets": [],
                "features": {},
            }))
            .expect("a package")
        };
        let apart = Workspace::from_packages(&tmp.0, vec![member(None)]);
        assert_eq!(
            standalone_root(&tmp.0, &apart, "sub/src/lib.rs").as_deref(),
            Some("sub")
        );
        let reached = Workspace::from_packages(&tmp.0, vec![member(Some(tmp.0.join("sub")))]);
        assert_eq!(standalone_root(&tmp.0, &reached, "sub/src/lib.rs"), None);
        // the repository root's own manifest never makes a file standalone
        std::fs::write(tmp.0.join("Cargo.toml"), "[workspace]\n").expect("write");
        assert_eq!(standalone_root(&tmp.0, &apart, "loose.txt"), None);
        assert_eq!(standalone_root(&tmp.0, &apart, "other/x.rs"), None);
    }

    fn a_dev_dependent_is_the_last_hop() {
        // base <- mid (normal) <- tester (dev) <- above_tester (normal);
        // base <- both (normal and dev) <- above_both (normal).
        let root = Path::new("/ws");
        let package = |name: &str, deps: &[(&str, Option<&str>)]| -> Package {
            let deps: Vec<_> = deps
                .iter()
                .map(|(dep, kind)| {
                    serde_json::json!({
                        "name": dep, "kind": kind, "rename": null, "optional": false,
                        "path": root.join(dep),
                    })
                })
                .collect();
            serde_json::from_value(serde_json::json!({
                "name": name,
                "manifest_path": root.join(name).join("Cargo.toml"),
                "dependencies": deps,
                "targets": [],
                "features": {},
            }))
            .expect("a package")
        };
        let ws = Workspace::from_packages(
            root,
            vec![
                package("base", &[]),
                package("mid", &[("base", None)]),
                package("tester", &[("mid", Some("dev"))]),
                package("above_tester", &[("tester", None)]),
                package("both", &[("base", None), ("base", Some("dev"))]),
                package("above_both", &[("both", None)]),
            ],
        );
        let affected = ws.affected(&BTreeSet::from(["base".to_owned()]));
        let expected: BTreeSet<String> = ["base", "mid", "tester", "both", "above_both"]
            .map(str::to_owned)
            .into();
        // tester's tests link mid, so they run; its library does not
        // contain mid, so nothing depending on tester is affected.
        assert_eq!(affected, expected);
    }

    fn optional_dependency_edges_count() {
        // declared but feature-gated: the resolved graph would miss these
        assert!(
            scope(&["packages/flui-cupertino/src/lib.rs"])
                .packages
                .contains(&"flui".to_owned())
        );
        assert!(
            scope(&["crates/flui-assets/src/lib.rs"])
                .packages
                .contains(&"flui-widgets".to_owned())
        );
    }

    fn root_package_owns_its_targets_directories() {
        let s = scope(&["examples/material_demo/tree.rs"]); // an [[example]] whose main.rs is in a subdirectory
        assert_eq!(s.mode, Mode::Packages);
        assert!(s.packages.contains(&"flui".to_owned()));
        let s = scope(&["examples/web_counter/src/lib.rs"]); // a separate package under examples/
        assert_eq!(
            (s.mode, s.packages),
            (Mode::Packages, vec!["flui-web-counter".to_owned()])
        );
    }

    fn a_changed_manifest_is_reported() {
        assert_eq!(
            scope(&["packages/flui-material/Cargo.toml"]).manifests,
            ["flui-material"]
        );
    }

    /// A throwaway git repository under the system temp dir, removed on drop.
    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let dir =
                std::env::temp_dir().join(format!("xtask-scope-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn git(&self, args: &[&str]) {
            let status = Command::new("git")
                .args(args)
                .current_dir(&self.0)
                .output()
                .expect("git");
            assert!(
                status.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn a_move_puts_both_crates_in_scope() {
        let tmp = TempRepo::new();
        tmp.git(&["init", "-q", "-b", "main"]);
        tmp.git(&["config", "user.email", "t@example.invalid"]);
        tmp.git(&["config", "user.name", "t"]);
        tmp.git(&["config", "core.autocrlf", "false"]);
        std::fs::create_dir(tmp.0.join("a")).expect("mkdir");
        std::fs::write(tmp.0.join("a/x.rs"), "fn x() {}\n".repeat(20)).expect("write");
        tmp.git(&["add", "."]);
        tmp.git(&["commit", "-qm", "base"]);
        tmp.git(&["checkout", "-qb", "change"]);
        std::fs::create_dir(tmp.0.join("b")).expect("mkdir");
        tmp.git(&["mv", "a/x.rs", "b/x.rs"]);
        tmp.git(&["commit", "-qm", "move"]);
        let repo = Repo::open(tmp.0.clone());
        assert_eq!(
            repo.changed_files("main", false).expect("diff"),
            ["a/x.rs", "b/x.rs"]
        );
        // uncommitted and untracked work joins with --worktree
        std::fs::write(tmp.0.join("new file.rs"), "").expect("write");
        assert_eq!(
            repo.changed_files("main", true).expect("diff"),
            ["a/x.rs", "b/x.rs", "new file.rs"]
        );
    }

    #[test]
    fn change_scope_classification() {
        crate::table_test::run_table(
            "change_scope_classification",
            &[
                ("documentation_paths", documentation_paths as fn()),
                (
                    "compiled_markdown_is_not_docs",
                    compiled_markdown_is_not_docs as fn(),
                ),
                ("docs_and_empty", docs_and_empty as fn()),
                (
                    "shaders_require_the_heavy_lane",
                    shaders_require_the_heavy_lane as fn(),
                ),
                (
                    "heavy_inputs_require_the_heavy_lane",
                    heavy_inputs_require_the_heavy_lane as fn(),
                ),
                (
                    "inputs_only_a_heavy_job_runs_require_the_heavy_lane",
                    inputs_only_a_heavy_job_runs_require_the_heavy_lane as fn(),
                ),
                (
                    "heavy_job_inputs_from_a_workflow",
                    heavy_job_inputs_from_a_workflow as fn(),
                ),
                (
                    "lane_machinery_gets_the_whole_workspace",
                    lane_machinery_gets_the_whole_workspace as fn(),
                ),
                (
                    "xtask_changes_scope_the_xtask_package",
                    xtask_changes_scope_the_xtask_package as fn(),
                ),
                (
                    "checks_only_tooling_compiles_nothing",
                    checks_only_tooling_compiles_nothing as fn(),
                ),
                (
                    "unattributable_file_is_full",
                    unattributable_file_is_full as fn(),
                ),
                (
                    "a_standalone_crate_is_tooling",
                    a_standalone_crate_is_tooling as fn(),
                ),
                (
                    "a_standalone_crate_beside_a_member_change_is_not_unowned",
                    a_standalone_crate_beside_a_member_change_is_not_unowned as fn(),
                ),
                (
                    "a_crate_a_member_depends_on_is_not_standalone",
                    a_crate_a_member_depends_on_is_not_standalone as fn(),
                ),
                (
                    "a_dev_dependent_is_the_last_hop",
                    a_dev_dependent_is_the_last_hop as fn(),
                ),
                (
                    "optional_dependency_edges_count",
                    optional_dependency_edges_count as fn(),
                ),
                (
                    "root_package_owns_its_targets_directories",
                    root_package_owns_its_targets_directories as fn(),
                ),
                (
                    "a_changed_manifest_is_reported",
                    a_changed_manifest_is_reported as fn(),
                ),
                (
                    "a_move_puts_both_crates_in_scope",
                    a_move_puts_both_crates_in_scope as fn(),
                ),
            ],
        );
    }
}
