//! Every repository path, package and `llms.txt` link the docs name exists.
//!
//! lychee (`docs-links`) follows Markdown links; this reads what it does not:
//!
//! - a repository path in a code span (`crates/flui-view/ARCHITECTURE.md`)
//!   of a Markdown file, or of `llms.txt`;
//! - the link destinations of `llms.txt`, which lychee reads as plain text,
//!   with an `#anchor` into Markdown naming one of its headings;
//! - the package after `-p`/`--package` in a cargo command, in a code span or
//!   a code block, which must be one the checkout has, or for `cargo update`,
//!   `tree`, `pkgid` and `clean` one in `Cargo.lock` ([`Packages`]).
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
mod shell;

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
const SELF_MAIN: [&str; 2] = ["blob/main/", "tree/main/"];

/// This repository on GitHub, whose owner and name match in any case.
const SELF_REPO: &str = "vanyastaff/flui/";

/// GitHub's origin, whose scheme and host match in any case.
const GITHUB: &str = "https://github.com/";

/// The checkout path a link to this repository's `main` names ([`SELF_MAIN`]).
fn self_main(dest: &str) -> Option<&str> {
    let origin = GITHUB.len() + SELF_REPO.len();
    let (github, repo) = dest.get(..origin)?.split_at(GITHUB.len());
    if !github.eq_ignore_ascii_case(GITHUB) || !repo.eq_ignore_ascii_case(SELF_REPO) {
        return None;
    }
    // the branch and the checkout path keep their case
    SELF_MAIN
        .iter()
        .find_map(|prefix| dest[origin..].strip_prefix(prefix))
}

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
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        found.extend(stale(doc, &text, &known, &packages, &read));
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

/// The cargo subcommands whose `-p` takes any package of the resolved graph,
/// a dependency too (`cargo update -p wgpu`, `cargo tree -p parley`,
/// `cargo pkgid -p wgpu`, `cargo clean -p wgpu`).
const GRAPH_SUBCOMMANDS: [&str; 4] = ["clean", "pkgid", "tree", "update"];

/// The packages a `-p` may name.
#[derive(Debug, Default)]
struct Packages {
    /// The workspace members, and every other package the checkout has a
    /// manifest for (the Android examples are excluded from the workspace),
    /// each with its version when the manifest states one.
    local: BTreeMap<String, BTreeSet<String>>,
    /// Every package in `Cargo.lock`, each locked version with its source,
    /// for a [`GRAPH_SUBCOMMANDS`] command.
    locked: BTreeMap<String, Vec<LockedVersion>>,
}

/// One locked version of a `Cargo.lock` package.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedVersion {
    version: String,
    /// `registry+https://…`, `git+https://…?rev=…#sha`; none for a workspace
    /// member.
    source: Option<String>,
}

impl Packages {
    /// Whether the command `selected` is in names a package it can select.
    /// `name@version` picks versions as a cargo package-ID spec does
    /// ([`version_matches`]), a local package's too. A lockfile package locked
    /// at two versions is ambiguous by name alone to every graph subcommand but
    /// `clean`, which cleans them all. A glob (`flui-*`) must match a package
    /// the command can select. `cargo uninstall -p` names an installed binary,
    /// which the checkout cannot know.
    fn selects(&self, selected: &extract::Selected) -> bool {
        let subcommand = selected.subcommand.as_deref();
        if subcommand == Some("uninstall") {
            return true;
        }
        let graph = subcommand.filter(|subcommand| GRAPH_SUBCOMMANDS.contains(subcommand));
        if extract::is_glob(&selected.name) {
            // a pattern selects workspace packages; a graph subcommand's `-p`
            // is a package-ID spec, where `*` is no valid character
            return graph.is_none()
                && self
                    .local
                    .keys()
                    .any(|name| extract::glob_matches(&selected.name, name));
        }
        let version_ok = |known: &str| {
            selected
                .version
                .as_deref()
                .is_none_or(|version| version_matches(version, known))
        };
        match selected.source.as_deref() {
            None => {
                // a graph subcommand weighs the lockfile's same-named packages too
                if graph.is_none()
                    && let Some(versions) = self.local.get(&selected.name)
                {
                    // a manifest that inherits its version states none to check
                    return versions.is_empty() || versions.iter().any(|known| version_ok(known));
                }
            }
            // a path source names one machine's absolute directory: cargo
            // resolves it on that machine only, so a doc cannot rely on it
            Some(source) if source.starts_with("path+") || source.starts_with("file:") => {
                return false;
            }
            Some(_) => {}
        }
        let Some(subcommand) = graph else {
            return false;
        };
        let locked = self
            .locked
            .get(&selected.name)
            .map_or(&[][..], Vec::as_slice);
        let in_lock = locked
            .iter()
            .filter(|locked| {
                version_ok(&locked.version)
                    && selected
                        .source
                        .as_deref()
                        .is_none_or(|source| source_matches(source, locked.source.as_deref()))
            })
            .count();
        // a workspace member is in the lockfile, without a source; a checkout
        // package that is not (an excluded example) is one candidate more
        let only_local = selected.source.is_none()
            && !locked.iter().any(|locked| locked.source.is_none())
            && self.local.get(&selected.name).is_some_and(|versions| {
                versions.is_empty() || versions.iter().any(|known| version_ok(known))
            });
        let matching = in_lock + usize::from(only_local);
        matching == 1 || (matching > 1 && subcommand == "clean")
    }
}

/// Whether the source of a package-ID spec (`registry+https://…/index`, or
/// the URL without its kind) is the `known` source of a locked package, whose
/// `?query` and `#revision` it need not spell.
fn source_matches(spec: &str, known: Option<&str>) -> bool {
    let Some(known) = known else {
        return false;
    };
    let base = known.split(['?', '#']).next().unwrap_or(known);
    let url = base.split_once('+').map_or(base, |(_, url)| url);
    let spec = normalized_url(spec);
    [known, base, url]
        .iter()
        .any(|known| normalized_url(known) == spec)
}

/// `source` as cargo canonicalizes it: the kind (`registry+`), the URL's
/// scheme and host lowercased, and a scheme's default port dropped
/// (`github.com:443`); the path keeps its case.
fn normalized_url(source: &str) -> String {
    let (kind, url) = match source.split_once("://") {
        Some((head, _)) => match head.rsplit_once('+') {
            Some((kind, _)) => (&source[..=kind.len()], &source[kind.len() + 1..]),
            None => ("", source),
        },
        None => return source.to_owned(),
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return source.to_owned();
    };
    let scheme = scheme.to_ascii_lowercase();
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = host.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "https" => ":443",
        "http" => ":80",
        _ => "",
    };
    let host = match host.strip_suffix(default_port) {
        Some(bare) if !default_port.is_empty() => bare.to_owned(),
        _ => host,
    };
    format!("{}{scheme}://{host}{path}", kind.to_ascii_lowercase())
}

/// Whether the version of a package-ID spec (`1`, `1.3`, `1.3.2`,
/// `0.2.0-dev`) picks the `known` version, as cargo matches them: a full
/// version, prerelease included, matches exactly, and build metadata only
/// when the spec states it (`1.2.3` picks `1.2.3+meta`); a partial one
/// matches every version that starts with it at a component boundary, but no
/// prerelease.
fn version_matches(spec: &str, known: &str) -> bool {
    let known = if spec.contains('+') {
        known
    } else {
        known.split('+').next().unwrap_or(known)
    };
    if spec == known {
        return true;
    }
    let partial = spec.split('.').count() < 3;
    partial && !known.contains('-') && known.starts_with(&format!("{spec}."))
}

/// The packages of the checkout at `root`.
fn packages(root: &std::path::Path, known: &Known) -> anyhow::Result<Packages> {
    #[derive(Deserialize)]
    struct Manifest {
        package: Option<Named>,
    }
    #[derive(Deserialize)]
    struct Lock {
        #[serde(default)]
        package: Vec<Locked>,
    }
    #[derive(Deserialize)]
    struct Locked {
        name: String,
        version: String,
        source: Option<String>,
    }
    #[derive(Deserialize)]
    struct Named {
        name: String,
        /// A string, or `{ workspace = true }` inherited from elsewhere.
        version: Option<toml::Value>,
    }
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in crate::util::metadata(root)?.workspace_packages() {
        names
            .entry(package.name.to_string())
            .or_default()
            .insert(package.version.to_string());
    }
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
            let versions = names.entry(package.name).or_default();
            if let Some(version) = package.version.as_ref().and_then(toml::Value::as_str) {
                versions.insert(version.to_owned());
            }
        }
    }
    let lock: Lock =
        toml::from_str(&crate::util::read("Cargo.lock")?).context("parsing Cargo.lock")?;
    let mut locked: BTreeMap<String, Vec<LockedVersion>> = BTreeMap::new();
    for package in lock.package {
        locked.entry(package.name).or_default().push(LockedVersion {
            version: package.version,
            source: package.source,
        });
    }
    Ok(Packages {
        local: names,
        locked,
    })
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
    /// directory; `""` is the root) is a known file or directory.
    fn has(&self, path: &str) -> bool {
        if path.trim_end_matches('/').is_empty() {
            return true;
        }
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
            Kind::Package => {
                "no package the command can select (not in the checkout, or a lockfile \
                 package it cannot pick: another subcommand, or one of several versions)"
            }
            Kind::Link => "the link resolves to no file, directory or heading",
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

/// Every stale name in `text`, the doc at `doc`; `read` reads a file of the
/// checkout, for the headings a link's `#anchor` must name.
fn stale(
    doc: &str,
    text: &str,
    known: &Known,
    packages: &Packages,
    read: &dyn Fn(&str) -> Option<String>,
) -> Vec<Stale> {
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
                if !known.resolves(doc, &path) {
                    push(code.line, Kind::Path, &path);
                }
            }
        }
        for selected in extract::packages_in(&code.text, code.dialect) {
            if !packages.selects(&selected) {
                // `code.line` is the line of the block's first line of text
                push(code.line + selected.line, Kind::Package, &selected.name);
            }
        }
    }
    if EXTRA_DOCS.contains(&doc) {
        for (line, dest) in extract::links(text) {
            let Some(path) = link_target(doc, &dest) else {
                continue;
            };
            let anchor = dest
                .split_once('#')
                .map(|(_, anchor)| percent_decoded(anchor))
                // an empty anchor (`#`) is the top of the document
                .filter(|anchor| !anchor.is_empty());
            let resolves = path.as_deref().is_some_and(|path| {
                known.has(path)
                    && anchor.as_ref().is_none_or(|anchor| {
                        // an anchor into Markdown names a heading, as lychee checks it
                        !(extract::has_extension(path, "md") || path == doc)
                            || read(path)
                                .is_some_and(|text| extract::anchors(&text).contains(anchor))
                    })
            });
            if !resolves {
                push(line, Kind::Link, &dest);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// What the link `dest` in `doc` names in the checkout (`doc` itself for an
/// in-page `#anchor`): `None` when it is not a local link, `Some(None)` when
/// it climbs above the root.
#[expect(
    clippy::option_option,
    reason = "not local, and local but outside the checkout, are different answers"
)]
fn link_target(doc: &str, dest: &str) -> Option<Option<String>> {
    let local = self_main(dest).map(|rest| format!("/{rest}"));
    let dest = match &local {
        Some(dest) => dest.as_str(),
        // an empty path before a query or an anchor is the doc itself
        None if dest.starts_with(['#', '?']) => return Some(Some(doc.to_owned())),
        // another scheme (a `:` before any `/`, `?` or `#`: one in a query or
        // anchor is data), or a scheme-relative `//host/path`
        None if dest
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|head| head.contains(':'))
            || dest.starts_with("//")
            || dest.is_empty() =>
        {
            return None;
        }
        None => dest,
    };
    let dest = percent_decoded(dest.split(['#', '?']).next().unwrap_or_default());
    let (base, rest) = match dest.strip_prefix('/') {
        Some(rest) => ("", rest),
        None => (
            doc.rsplit_once('/').map_or("", |(dir, _)| dir),
            dest.as_str(),
        ),
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

/// `text` with each `%XX` escape of a URL decoded (`review%20probe.md` is the
/// file `review probe.md`); an escape that is not two hex digits stays as written.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escaped {
            decoded.push(byte);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
