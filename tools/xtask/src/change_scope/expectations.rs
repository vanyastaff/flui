//! The expectations a change edits, and the pull-request body that names each one (ADR-0331).
//!
//! An expectation is a file or a line that says what correct is: the API snapshot, a trybuild
//! `*.stderr`, an insta `*.snap`, a golden protocol schema, a perf baseline, an allowlist entry,
//! a test, an exemption in `deny.toml` or `typos.toml`. A change that edits one also changes
//! what the checks accept, so the same diff can fix a defect or hide one. `cargo xtask
//! expectations` lists every such edit; with `--body` it fails unless the body's
//! `## Expectations` section names each path on a line that also says why.
//!
//! ```text
//! cargo xtask expectations                       # vs origin/main
//! cargo xtask expectations --worktree            # also uncommitted work
//! cargo xtask expectations --base <sha> --body body.md   # CI (expectations.yml)
//! ```
//!
//! It reads git's list of changed files, the token trees of the changed Rust files and the TOML
//! tables of the changed data files, each before and after; no pattern runs over source text.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use proc_macro2::{Delimiter, TokenStream, TokenTree};

use super::classify::Repo;
use crate::util::repo_root;

/// Arguments for `cargo xtask expectations`.
#[derive(Debug, clap::Args)]
pub(crate) struct ExpectationsArgs {
    /// Diff against the merge base with this revision.
    #[arg(long, default_value = "origin/main")]
    base: String,
    /// Compare the working tree (uncommitted work too) instead of `HEAD`.
    #[arg(long)]
    worktree: bool,
    /// The pull request's body; fail unless its `## Expectations` section names every edit.
    #[arg(long)]
    body: Option<PathBuf>,
}

/// What kind of expectation an edit touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// Lines removed from `api/flui.txt` (ADR-0324).
    ApiSnapshot,
    /// A trybuild `*.stderr` changed or deleted.
    Trybuild,
    /// An insta `*.snap` changed or deleted.
    Snapshot,
    /// A golden agent-protocol schema added, changed or deleted (ADR-0095).
    ProtocolSchema,
    /// A perf baseline whose count rose or left, or that was deleted.
    PerfBaseline,
    /// An allowlist of a gate gained an entry, a count or a name.
    Allowlist,
    /// A Rust file has fewer tests: test attributes and table-test cases (`case as fn()`).
    TestsRemoved,
    /// A Rust file has more `#[ignore]` attributes.
    TestIgnored,
    /// `deny.toml` or `typos.toml` gained a value.
    Exemption,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::ApiSnapshot => "API snapshot",
            Self::Trybuild => "trybuild expectation",
            Self::Snapshot => "insta snapshot",
            Self::ProtocolSchema => "protocol schema",
            Self::PerfBaseline => "perf baseline",
            Self::Allowlist => "allowlist",
            Self::TestsRemoved => "tests removed",
            Self::TestIgnored => "test ignored",
            Self::Exemption => "exemption",
        }
    }
}

/// One edit to an expectation: the path a body names, and what changed there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Edit {
    path: String,
    kind: Kind,
    detail: String,
}

/// How git reports a file between the merge base and the compared tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Added,
    Modified,
    Deleted,
}

impl Status {
    fn verb(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "changed",
            Self::Deleted => "deleted",
        }
    }
}

/// One changed file. `old` and `new` hold its text at the merge base and now when
/// [`reads_text`] says the check compares it; `old` is `None` for an added file and `new` for a
/// deleted one.
#[derive(Debug)]
struct Change {
    status: Status,
    path: String,
    old: Option<String>,
    new: Option<String>,
}

const API_SNAPSHOT: &str = "api/flui.txt";
const PROTOCOL_SCHEMAS: &str = "crates/flui-protocol/tests/schema/";
const ALLOWLISTS: &str = "tools/xtask/allowlists/";
const SIZE_BASELINE: &str = "perf/size.toml";
const PERF_BASELINES: &str = "/perf/baseline/";
const EXEMPTION_FILES: [&str; 2] = ["deny.toml", "typos.toml"];

fn has_extension(path: &str, extension: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

fn is_allowlist(path: &str) -> bool {
    path.starts_with(ALLOWLISTS) && has_extension(path, "toml")
}

fn is_perf_baseline(path: &str) -> bool {
    path == SIZE_BASELINE || (path.contains(PERF_BASELINES) && has_extension(path, "toml"))
}

/// Whether the check compares the file's text, and not only how git reports it.
fn reads_text(path: &str) -> bool {
    path == API_SNAPSHOT
        || has_extension(path, "rs")
        || is_allowlist(path)
        || is_perf_baseline(path)
        || EXEMPTION_FILES.contains(&path)
}

/// What a Rust file's tokens say about its tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TestCounts {
    /// Test attributes (`#[test]`, a path ending in `test` such as `#[tokio::test]`,
    /// `#[wasm_bindgen_test]`) and table-test cases (`case as fn()`).
    tests: usize,
    /// `#[ignore]` attributes.
    ignored: usize,
}

/// Counts the tests of a Rust file from its token trees, so a `#[test]` in a comment or a string
/// is not one, and one inside a macro invocation (`proptest! { … }`) is. Text that does not lex
/// has none.
fn test_counts(text: &str) -> TestCounts {
    let mut counts = TestCounts::default();
    if let Ok(tokens) = text.parse::<TokenStream>() {
        count_tests(tokens, &mut counts);
    }
    counts
}

fn count_tests(tokens: TokenStream, counts: &mut TestCounts) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut rest: &[TokenTree] = &trees;
    while let [first, tail @ ..] = rest {
        rest = tail;
        match first {
            TokenTree::Punct(hash) if hash.as_char() == '#' => {
                // `#[…]` or `#![…]`.
                let after_bang = match tail {
                    [TokenTree::Punct(bang), inner @ ..] if bang.as_char() == '!' => inner,
                    _ => tail,
                };
                if let [TokenTree::Group(group), after @ ..] = after_bang
                    && group.delimiter() == Delimiter::Bracket
                {
                    count_attribute(group.stream(), counts);
                    rest = after;
                }
            }
            TokenTree::Ident(word) if word == "as" && casts_to_bare_fn(tail) => counts.tests += 1,
            TokenTree::Group(group) => count_tests(group.stream(), counts),
            _ => {}
        }
    }
}

/// Whether the tokens after `as` are `fn()` with no return type: a table-test case,
/// `("name", case as fn())`.
fn casts_to_bare_fn(after_as: &[TokenTree]) -> bool {
    let [TokenTree::Ident(word), TokenTree::Group(args), rest @ ..] = after_as else {
        return false;
    };
    let returns = matches!(
        rest,
        [TokenTree::Punct(dash), TokenTree::Punct(angle), ..]
            if dash.as_char() == '-' && angle.as_char() == '>'
    );
    word == "fn"
        && args.delimiter() == Delimiter::Parenthesis
        && args.stream().is_empty()
        && !returns
}

/// Counts one attribute's content (the tokens inside `#[…]`) by the last segment of its path;
/// `cfg_attr(predicate, a, b)` counts `a` and `b`.
fn count_attribute(attribute: TokenStream, counts: &mut TestCounts) {
    let trees: Vec<TokenTree> = attribute.into_iter().collect();
    let mut name: Option<String> = None;
    let mut rest: &[TokenTree] = &trees;
    loop {
        match rest {
            [TokenTree::Ident(segment), tail @ ..] => {
                name = Some(segment.to_string());
                rest = tail;
            }
            [TokenTree::Punct(colon), tail @ ..] if colon.as_char() == ':' => rest = tail,
            _ => break,
        }
    }
    match name.as_deref() {
        Some("test" | "wasm_bindgen_test") => counts.tests += 1,
        Some("ignore") => counts.ignored += 1,
        Some("cfg_attr") => {
            let [TokenTree::Group(args), ..] = rest else {
                return;
            };
            let mut conditional = TokenStream::new();
            let mut predicate_passed = false;
            for tree in args.stream() {
                if matches!(&tree, TokenTree::Punct(comma) if comma.as_char() == ',') {
                    if predicate_passed {
                        count_attribute(std::mem::take(&mut conditional), counts);
                    }
                    predicate_passed = true;
                } else if predicate_passed {
                    conditional.extend([tree]);
                }
            }
            count_attribute(conditional, counts);
        }
        _ => {}
    }
}

/// `cargo xtask expectations`: list the expectations a change edits; with `--body`, fail on
/// each the body does not name.
pub(crate) fn expectations(args: &ExpectationsArgs) -> anyhow::Result<ExitCode> {
    let repo = Repo::open(repo_root());
    let changes = read_changes(&repo, &args.base, args.worktree)?;
    let edits = edits(&changes);
    print!("{}", report(&edits));
    let Some(body_path) = &args.body else {
        return Ok(ExitCode::SUCCESS);
    };
    let body = std::fs::read_to_string(body_path)
        .with_context(|| format!("reading the body {}", body_path.display()))?;
    let unnamed = unnamed(&edits, &body);
    if unnamed.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "expectations: the pull request body does not name {} edited path(s) under \
         `## Expectations` (ADR-0331):",
        unnamed.len()
    );
    for path in &unnamed {
        eprintln!("  {path}");
    }
    eprintln!(
        "Fix: add a `## Expectations` section to the body with one line per path: the path (or \
         its directory, with the trailing `/`) and why the old expectation was wrong or the new \
         one is right; a moved test names where it went. Editing the body re-runs this check."
    );
    Ok(ExitCode::FAILURE)
}

/// Reads the changed files from git, with the text of those the check compares.
fn read_changes(repo: &Repo, base: &str, worktree: bool) -> anyhow::Result<Vec<Change>> {
    let merge_base = repo.git(&["merge-base", base, "HEAD"])?;
    let merge_base = merge_base.trim();
    let range = format!("{merge_base}..HEAD");
    // The working tree against the merge base, or the commits since it.
    let compared: &str = if worktree { merge_base } else { &range };
    let status = repo.git(&["diff", "--name-status", "--no-renames", "-z", compared])?;
    let mut changes = Vec::new();
    for (status, path) in parse_name_status(&status) {
        let read = reads_text(&path);
        let old = if read && status != Status::Added {
            Some(repo.git(&["show", &format!("{merge_base}:{path}")])?)
        } else {
            None
        };
        let new = if !read || status == Status::Deleted {
            None
        } else if worktree {
            let text = std::fs::read_to_string(repo.root.join(&path))
                .with_context(|| format!("reading {path}"))?;
            Some(text)
        } else {
            Some(repo.git(&["show", &format!("HEAD:{path}")])?)
        };
        changes.push(Change {
            status,
            path,
            old,
            new,
        });
    }
    Ok(changes)
}

/// `git diff --name-status -z`: a status letter and a path, each NUL-terminated. A status git
/// writes for no file this check reads (a type change) is skipped.
fn parse_name_status(out: &str) -> Vec<(Status, String)> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let status = match status.chars().next() {
            Some('A') => Status::Added,
            Some('M') => Status::Modified,
            Some('D') => Status::Deleted,
            _ => continue,
        };
        changes.push((status, path.to_owned()));
    }
    changes
}

/// Every edit to an expectation among `changes`, sorted by path.
fn edits(changes: &[Change]) -> Vec<Edit> {
    let mut edits = BTreeSet::new();
    for change in changes {
        let Change {
            status,
            path,
            old,
            new,
        } = change;
        let mut edit = |kind: Kind, detail: String| {
            edits.insert(Edit {
                path: path.clone(),
                kind,
                detail,
            });
        };
        let edited = *status != Status::Added;
        let (old_text, new_text) = (old.as_deref().unwrap_or(""), new.as_deref().unwrap_or(""));
        if has_extension(path, "stderr") {
            if edited {
                edit(Kind::Trybuild, status.verb().to_owned());
            }
        } else if has_extension(path, "snap") {
            if edited {
                edit(Kind::Snapshot, status.verb().to_owned());
            }
        } else if path.starts_with(PROTOCOL_SCHEMAS) && has_extension(path, "json") {
            edit(Kind::ProtocolSchema, status.verb().to_owned());
        } else if path == API_SNAPSHOT {
            let removed = lines_removed(old_text, new_text);
            if removed > 0 {
                edit(Kind::ApiSnapshot, format!("{removed} line(s) removed"));
            }
        } else if is_perf_baseline(path) {
            if *status == Status::Deleted {
                edit(Kind::PerfBaseline, status.verb().to_owned());
            } else if edited {
                for detail in baseline_rises(old_text, new_text) {
                    edit(Kind::PerfBaseline, detail);
                }
            }
        } else if is_allowlist(path) {
            if let Some(new) = new {
                for detail in allowlist_growth(old.as_deref(), new) {
                    edit(Kind::Allowlist, detail);
                }
            }
        } else if EXEMPTION_FILES.contains(&path.as_str()) {
            if let Some(detail) = values_added(old_text, new_text) {
                edit(Kind::Exemption, detail);
            }
        } else if has_extension(path, "rs") {
            let (was, is) = (test_counts(old_text), test_counts(new_text));
            if was.tests > is.tests {
                let fewer = was.tests - is.tests;
                edit(
                    Kind::TestsRemoved,
                    format!("{fewer} fewer test(s): {} -> {}", was.tests, is.tests),
                );
            }
            if is.ignored > was.ignored {
                edit(
                    Kind::TestIgnored,
                    format!("`#[ignore]` {} -> {}", was.ignored, is.ignored),
                );
            }
        }
    }
    edits.into_iter().collect()
}

/// How many lines of `old` have no counterpart in `new`, each line counted as often as it
/// occurs. The API snapshot writes an item's whole path on its line, so a moved line is not a
/// removed one.
fn lines_removed(old: &str, new: &str) -> usize {
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for line in new.lines() {
        *left.entry(line).or_default() += 1;
    }
    old.lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| match left.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .count()
}

/// The leaves of a TOML value as a path and a value, with `[]` for an array's items, so an item
/// is the same wherever it stands in its array. A `reason` is prose about its entry and is left
/// out.
fn toml_leaves(prefix: &str, value: &toml::Value, leaves: &mut Vec<(String, toml::Value)>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                if key == "reason" {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                toml_leaves(&path, value, leaves);
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                toml_leaves(&format!("{prefix}[]"), item, leaves);
            }
        }
        leaf => leaves.push((prefix.to_owned(), leaf.clone())),
    }
}

fn leaves_of(text: &str) -> Option<Vec<(String, toml::Value)>> {
    let table = text.parse::<toml::Table>().ok()?;
    let mut leaves = Vec::new();
    toml_leaves("", &toml::Value::Table(table), &mut leaves);
    Some(leaves)
}

/// The values `new` has and `old` did not, of `deny.toml` or `typos.toml`: a new key, a new
/// array item, or a key whose value changed. `None` when there is none.
fn values_added(old: &str, new: &str) -> Option<String> {
    let (Some(old), Some(new)) = (leaves_of(old), leaves_of(new)) else {
        return Some("does not parse as TOML".to_owned());
    };
    let written = |leaves: Vec<(String, toml::Value)>| -> BTreeSet<String> {
        leaves
            .into_iter()
            .map(|(path, value)| format!("{path} = {value}"))
            .collect()
    };
    let (old, new) = (written(old), written(new));
    let added: Vec<&str> = new.difference(&old).map(String::as_str).collect();
    if added.is_empty() {
        return None;
    }
    let shown = added
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<&str>>()
        .join(", ");
    let more = if added.len() > 3 { ", …" } else { "" };
    Some(format!("{} value(s) added: {shown}{more}", added.len()))
}

/// What a perf baseline lost between `old` and `new`: each count that rose and each count that
/// is no longer written. A fall tightens the baseline and a new count adds to it.
fn baseline_rises(old: &str, new: &str) -> Vec<String> {
    let (Some(old), Some(new)) = (leaves_of(old), leaves_of(new)) else {
        return vec!["does not parse as TOML".to_owned()];
    };
    let new: BTreeMap<String, toml::Value> = new.into_iter().collect();
    let mut rises = Vec::new();
    for (path, was) in &old {
        match (was.as_integer(), new.get(path).map(toml::Value::as_integer)) {
            (Some(was), Some(Some(is))) if is > was => rises.push(format!("{path} {was} -> {is}")),
            (_, None) => rises.push(format!("{path} removed")),
            _ => {}
        }
    }
    rises
}

/// How an allowlist grew from `old` to `new`: each entry with no counterpart before, each count
/// that rose and each list field that gained a member. An entry is a table of a top-level array
/// (`[[allow]]`, `positions = [...]`); it is matched by its fields other than `reason`, `exit`,
/// the integers (counts) and the arrays of strings (names), which are compared instead. A file
/// that does not parse counts as one growth, so the body explains it.
fn allowlist_growth(old: Option<&str>, new: &str) -> Vec<String> {
    let parse = |text: &str| text.parse::<toml::Table>();
    let Ok(new) = parse(new) else {
        return vec!["does not parse as TOML".to_owned()];
    };
    let old = match old.map(parse) {
        None => toml::Table::new(),
        Some(Ok(table)) => table,
        Some(Err(_)) => return vec!["did not parse as TOML before".to_owned()],
    };
    let before: BTreeMap<String, Measured> = entries(&old)
        .into_iter()
        .map(|m| (m.key.clone(), m))
        .collect();
    let mut growth = Vec::new();
    for entry in entries(&new) {
        let Some(prior) = before.get(&entry.key) else {
            growth.push(format!("new entry {}", entry.key));
            continue;
        };
        for (field, value) in &entry.counts {
            let was = prior.counts.get(field).copied().unwrap_or(0);
            if *value > was {
                growth.push(format!("{} {field} {was} -> {value}", entry.key));
            }
        }
        for (field, members) in &entry.lists {
            let empty = BTreeSet::new();
            let was = prior.lists.get(field).unwrap_or(&empty);
            let gained: Vec<&str> = members.difference(was).map(String::as_str).collect();
            if !gained.is_empty() {
                growth.push(format!(
                    "{} {field} gained {}",
                    entry.key,
                    gained.join(", ")
                ));
            }
        }
    }
    growth
}

/// An allowlist entry: its identity, its counts and its lists of names.
#[derive(Debug)]
struct Measured {
    key: String,
    counts: BTreeMap<String, i64>,
    lists: BTreeMap<String, BTreeSet<String>>,
}

fn entries(table: &toml::Table) -> Vec<Measured> {
    let mut out = Vec::new();
    for (array_name, value) in table {
        let Some(array) = value.as_array() else {
            continue;
        };
        for item in array {
            let Some(entry) = item.as_table() else {
                continue;
            };
            let mut identity = toml::Table::new();
            let mut counts = BTreeMap::new();
            let mut lists = BTreeMap::new();
            for (field, value) in entry {
                match value {
                    toml::Value::Integer(n) => {
                        counts.insert(field.clone(), *n);
                    }
                    toml::Value::Array(items) if items.iter().all(toml::Value::is_str) => {
                        let names: BTreeSet<String> = items
                            .iter()
                            .filter_map(toml::Value::as_str)
                            .map(str::to_owned)
                            .collect();
                        lists.insert(field.clone(), names);
                    }
                    _ if field == "reason" || field == "exit" => {}
                    _ => {
                        identity.insert(field.clone(), value.clone());
                    }
                }
            }
            let mut key = format!("{array_name} {{");
            for (i, (field, value)) in identity.iter().enumerate() {
                let sep = if i == 0 { " " } else { ", " };
                let _ = write!(key, "{sep}{field} = {value}");
            }
            key.push_str(" }");
            out.push(Measured { key, counts, lists });
        }
    }
    out
}

/// The report `expectations` prints: one line per edit, or a line saying there is none.
fn report(edits: &[Edit]) -> String {
    let mut out = String::new();
    if edits.is_empty() {
        out.push_str("expectations: this change edits none\n");
        return out;
    }
    let paths: BTreeSet<&str> = edits.iter().map(|e| e.path.as_str()).collect();
    let _ = writeln!(
        out,
        "expectations: {} edit(s) in {} path(s); the pull request body names each path under \
         `## Expectations` with its reason (ADR-0331)",
        edits.len(),
        paths.len()
    );
    for edit in edits {
        let _ = writeln!(
            out,
            "- `{}`: {}, {}",
            edit.path,
            edit.kind.label(),
            edit.detail
        );
    }
    out
}

/// The edited paths `body` does not name: each needs a line inside the `## Expectations`
/// section that holds the path, or the directory it sits in (written with its trailing `/`, so
/// one line covers a suite's regenerated files), and at least a few words besides it.
fn unnamed(edits: &[Edit], body: &str) -> Vec<String> {
    let section = expectations_section(body);
    let paths: BTreeSet<&str> = edits.iter().map(|e| e.path.as_str()).collect();
    // A name stands alone: `crates/x/tests/` inside `crates/x/tests/b.rs` names nothing.
    let in_a_path = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '/');
    let names = |line: &str, name: &str| {
        line.match_indices(name).any(|(start, _)| {
            let (before, after) = (&line[..start], &line[start + name.len()..]);
            let alone = !before.ends_with(|c: char| in_a_path(c) || c == '.')
                && !after.starts_with(in_a_path);
            let reason = before.chars().chain(after.chars());
            alone && reason.filter(|c| c.is_alphanumeric()).count() >= MIN_REASON_CHARS
        })
    };
    paths
        .into_iter()
        .filter(|path| {
            let directory = path.rfind('/').map(|slash| &path[..=slash]);
            !section.iter().any(|line| {
                names(line, path) || directory.is_some_and(|directory| names(line, directory))
            })
        })
        .map(str::to_owned)
        .collect()
}

/// The fewest letters and digits a line needs besides the path to count as a reason.
const MIN_REASON_CHARS: usize = 12;

/// The lines under a `## Expectations` heading (any level from `#` to `###`), up to the next
/// heading of that range.
fn expectations_section(body: &str) -> Vec<&str> {
    let heading = |line: &str| {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        (1..=3).contains(&hashes) && trimmed[hashes..].starts_with(' ')
    };
    let mut inside = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        if heading(line) {
            inside = line
                .trim_start()
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("expectations");
            continue;
        }
        if inside {
            lines.push(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(status: Status, path: &str, old: Option<&str>, new: Option<&str>) -> Change {
        Change {
            status,
            path: path.to_owned(),
            old: old.map(str::to_owned),
            new: new.map(str::to_owned),
        }
    }

    fn modified(path: &str, old: &str, new: &str) -> Vec<Change> {
        vec![change(Status::Modified, path, Some(old), Some(new))]
    }

    fn kinds(edits: &[Edit]) -> Vec<(&str, Kind)> {
        edits.iter().map(|e| (e.path.as_str(), e.kind)).collect()
    }

    fn golden_files_count_when_changed_or_deleted_and_a_schema_also_when_added() {
        let found = edits(&[
            change(
                Status::Modified,
                "crates/flui-view/tests/ui/bad.stderr",
                None,
                None,
            ),
            change(
                Status::Added,
                "crates/flui-view/tests/ui/new.stderr",
                None,
                None,
            ),
            change(
                Status::Deleted,
                "crates/flui-objects/tests/snapshots/a.snap",
                None,
                None,
            ),
            change(
                Status::Added,
                "crates/flui-protocol/tests/schema/protocol-0.11.json",
                None,
                None,
            ),
            change(
                Status::Modified,
                "crates/flui-view/src/lib.rs",
                Some(""),
                Some(""),
            ),
        ]);
        assert_eq!(
            kinds(&found),
            [
                ("crates/flui-objects/tests/snapshots/a.snap", Kind::Snapshot),
                (
                    "crates/flui-protocol/tests/schema/protocol-0.11.json",
                    Kind::ProtocolSchema
                ),
                ("crates/flui-view/tests/ui/bad.stderr", Kind::Trybuild),
            ]
        );
        assert_eq!(found[0].detail, "deleted");
        assert_eq!(found[1].detail, "added");
    }

    fn api_lines_removed_count_and_lines_added_or_moved_do_not() {
        let old = "pub fn flui::a()\npub fn flui::b()\n\nimpl Clone for flui::T\nimpl Clone for flui::T\n";
        let found = edits(&modified(
            API_SNAPSHOT,
            old,
            "pub fn flui::c()\nimpl Clone for flui::T\n",
        ));
        assert_eq!(kinds(&found), [(API_SNAPSHOT, Kind::ApiSnapshot)]);
        assert_eq!(found[0].detail, "3 line(s) removed");
        let moved = "impl Clone for flui::T\npub fn flui::b()\npub fn flui::new()\n\
                     impl Clone for flui::T\npub fn flui::a()\n";
        assert_eq!(edits(&modified(API_SNAPSHOT, old, moved)), []);
    }

    fn a_file_that_loses_tests_counts_and_a_conversion_to_a_table_does_not() {
        let path = "crates/x/tests/a.rs";
        let two = "#[test]\nfn a() {}\nfn table() { run(&[(\"b\", b as fn())]) }\n";
        let found = edits(&modified(path, two, "fn table() { run(&[]) }\n"));
        assert_eq!(kinds(&found), [(path, Kind::TestsRemoved)]);
        assert_eq!(found[0].detail, "2 fewer test(s): 2 -> 0");
        let attributes = "#[test]\nfn a() {}\n#[tokio::test]\nasync fn b() {}\n";
        let table = "#[test]\nfn table() { run(&[(\"a\", a as fn()), (\"b\", b as fn())]) }\n";
        assert_eq!(edits(&modified(path, attributes, table)), []);
        let deleted = [change(Status::Deleted, path, Some(attributes), None)];
        assert_eq!(kinds(&edits(&deleted)), [(path, Kind::TestsRemoved)]);
        let added = [change(Status::Added, path, None, Some(attributes))];
        assert_eq!(edits(&added), []);
    }

    fn tests_are_counted_in_tokens_and_not_in_text() {
        let prose = "// #[test]\n/// #[test] and `x as fn()`\nconst S: &str = \"#[test] x as fn()\";\n\
                     #![cfg(test)]\nfn f() -> fn() -> u8 { g as fn() -> u8 }\n";
        assert_eq!(test_counts(prose), TestCounts::default());
        let in_a_macro = "proptest! { #[test] fn holds(x in 0..9u8) { check(x) } }";
        assert_eq!(test_counts(in_a_macro).tests, 1);
        let conditional = "#[cfg_attr(not(target_arch = \"wasm32\"), test)]\n\
                           #[cfg_attr(target_arch = \"wasm32\", wasm_bindgen_test::wasm_bindgen_test)]\n\
                           #[cfg_attr(miri, ignore = \"slow\", allow(unused))]\nfn a() {}\n";
        assert_eq!(
            test_counts(conditional),
            TestCounts {
                tests: 2,
                ignored: 1
            }
        );
        assert_eq!(test_counts("#[test] fn a( {"), TestCounts::default());
    }

    fn an_added_ignore_counts() {
        let path = "crates/x/src/a.rs";
        let found = edits(&modified(
            path,
            "#[test]\nfn a() {}\n",
            "#[test]\n#[ignore = \"flaky\"]\nfn a() {}\n",
        ));
        assert_eq!(kinds(&found), [(path, Kind::TestIgnored)]);
        assert_eq!(found[0].detail, "`#[ignore]` 0 -> 1");
    }

    fn an_exemption_counts_by_its_values_and_not_by_its_comments_or_reasons() {
        let old = "[advisories]\nignore = [\n  { id = \"RUSTSEC-1\", reason = \"old words\" },\n]\n\
                   [bans]\nmultiple-versions = \"deny\"\n";
        let reworded = "# a comment\n[advisories]\nignore = [\n  { id = \"RUSTSEC-1\", reason = \"new words\" },\n]\n\
                        [bans]\nmultiple-versions = \"deny\"\n";
        assert_eq!(edits(&modified("deny.toml", old, reworded)), []);
        let grown = "[advisories]\nignore = [\n  { id = \"RUSTSEC-2\", reason = \"r\" },\n  \
                     { id = \"RUSTSEC-1\", reason = \"old words\" },\n]\n\
                     [bans]\nmultiple-versions = \"warn\"\n";
        let found = edits(&modified("deny.toml", old, grown));
        assert_eq!(kinds(&found), [("deny.toml", Kind::Exemption)]);
        assert_eq!(
            found[0].detail,
            "2 value(s) added: advisories.ignore[].id = \"RUSTSEC-2\", \
             bans.multiple-versions = \"warn\""
        );
        let shrunk = "[bans]\nmultiple-versions = \"deny\"\n";
        assert_eq!(edits(&modified("typos.toml", old, shrunk)), []);
        assert_eq!(
            edits(&modified("typos.toml", old, "[bans"))[0].detail,
            "does not parse as TOML"
        );
    }

    fn a_perf_baseline_counts_when_a_count_rises_or_leaves_and_not_when_it_falls() {
        let path = "crates/flui-widgets/perf/baseline/label_change_small.toml";
        let old = "[label_change_small]\nallocations = 51\nbuilds_run = 12\nnodes_painted = 3\n";
        let cheaper = "[label_change_small]\nallocations = 40\nbuilds_run = 12\nnodes_painted = 3\nnew_count = 9\n";
        assert_eq!(edits(&modified(path, old, cheaper)), []);
        let dearer = "[label_change_small]\nallocations = 60\nbuilds_run = 12\n";
        let found = edits(&modified(path, old, dearer));
        let details: Vec<&str> = found.iter().map(|e| e.detail.as_str()).collect();
        assert_eq!(
            details,
            [
                "label_change_small.allocations 51 -> 60",
                "label_change_small.nodes_painted removed"
            ]
        );
        assert!(found.iter().all(|e| e.kind == Kind::PerfBaseline));
        let size = edits(&modified(
            SIZE_BASELINE,
            "[size]\ncrates_linux = 272\n",
            "[size]\ncrates_linux = 273\n",
        ));
        assert_eq!(size[0].detail, "size.crates_linux 272 -> 273");
        let deleted = [change(Status::Deleted, path, Some(old), None)];
        assert_eq!(edits(&deleted)[0].detail, "deleted");
        let added = [change(Status::Added, path, None, Some(old))];
        assert_eq!(edits(&added), []);
    }

    fn an_allowlist_file_counts_by_its_growth() {
        let path = "tools/xtask/allowlists/try-new.toml";
        let old = "[[allow]]\npath = \"a.rs\"\ncount = 2\n";
        let found = edits(&modified(
            path,
            old,
            "[[allow]]\npath = \"a.rs\"\ncount = 3\n",
        ));
        assert_eq!(kinds(&found), [(path, Kind::Allowlist)]);
        let lowered = "[[allow]]\npath = \"a.rs\"\ncount = 1\n";
        assert_eq!(edits(&modified(path, old, lowered)), []);
        assert_eq!(edits(&[change(Status::Deleted, path, Some(old), None)]), []);
    }

    fn an_allowlist_grows_by_a_new_entry_a_higher_count_or_a_new_name_and_not_by_shrinking() {
        let old = "reason = \"r\"\n[[allow]]\npath = \"a.rs\"\ncount = 2\n\
                   [[allow]]\npath = \"b.rs\"\nkind = \"pump\"\nnames = [\"advance\"]\n";
        let grown = "reason = \"r\"\n[[allow]]\npath = \"a.rs\"\ncount = 3\n\
                     [[allow]]\npath = \"b.rs\"\nkind = \"pump\"\nnames = [\"advance\", \"tick\"]\n\
                     [[allow]]\npath = \"c.rs\"\ncount = 1\nreason = \"new\"\n";
        assert_eq!(
            allowlist_growth(Some(old), grown),
            [
                "allow { path = \"a.rs\" } count 2 -> 3",
                "allow { kind = \"pump\", path = \"b.rs\" } names gained tick",
                "new entry allow { path = \"c.rs\" }",
            ]
        );
        let shrunk =
            "reason = \"r\"\n[[allow]]\npath = \"a.rs\"\ncount = 1\nreason = \"reworded\"\n";
        assert_eq!(allowlist_growth(Some(old), shrunk), Vec::<String>::new());
        assert_eq!(
            allowlist_growth(None, shrunk),
            ["new entry allow { path = \"a.rs\" }"]
        );
        assert_eq!(
            allowlist_growth(Some(old), "[[allow"),
            ["does not parse as TOML"]
        );
    }

    fn a_body_names_a_path_only_inside_the_section_and_with_a_reason() {
        let found = vec![
            Edit {
                path: "api/flui.txt".to_owned(),
                kind: Kind::ApiSnapshot,
                detail: String::new(),
            },
            Edit {
                path: "crates/x/tests/a.rs".to_owned(),
                kind: Kind::TestsRemoved,
                detail: String::new(),
            },
        ];
        let body = "## Summary\n\napi/flui.txt is mentioned here, outside the section.\n\n\
                    ## Expectations\n\n- `crates/x/tests/a.rs`: moved to crates/x/tests/b.rs with its cases\n\
                    - `api/flui.txt`\n\n## Notes\n- api/flui.txt removes Foo::bar, deprecated since 0.3\n";
        assert_eq!(unnamed(&found, body), ["api/flui.txt"]);
        let named = body.replace(
            "- `api/flui.txt`\n",
            "- `api/flui.txt`: drops Foo::bar, which ADR-0330 removed\n",
        );
        assert_eq!(unnamed(&found, &named), Vec::<String>::new());
        let by_directory = body.replace(
            "- `api/flui.txt`\n",
            "- api/: the snapshot drops Foo::bar\n- crates/: too wide to name a test file\n",
        );
        assert_eq!(unnamed(&found, &by_directory), Vec::<String>::new());
        let too_wide = body.replace("`crates/x/tests/a.rs`: moved", "crates/x/: moved");
        assert_eq!(
            unnamed(&found, &too_wide),
            ["api/flui.txt", "crates/x/tests/a.rs"]
        );
        assert_eq!(unnamed(&found, "no section").len(), 2);
        assert_eq!(unnamed(&[], ""), Vec::<String>::new());
    }

    fn name_status_reads_nul_separated_pairs() {
        assert_eq!(
            parse_name_status("M\0a.rs\0D\0b.snap\0T\0c\0A\0d.stderr\0"),
            [
                (Status::Modified, "a.rs".to_owned()),
                (Status::Deleted, "b.snap".to_owned()),
                (Status::Added, "d.stderr".to_owned()),
            ]
        );
    }

    #[test]
    fn expectations_contract() {
        crate::table_test::run_table(
            "expectations_contract",
            &[
                (
                    "golden_files_count_when_changed_or_deleted_and_a_schema_also_when_added",
                    golden_files_count_when_changed_or_deleted_and_a_schema_also_when_added as fn(),
                ),
                (
                    "api_lines_removed_count_and_lines_added_or_moved_do_not",
                    api_lines_removed_count_and_lines_added_or_moved_do_not as fn(),
                ),
                (
                    "a_file_that_loses_tests_counts_and_a_conversion_to_a_table_does_not",
                    a_file_that_loses_tests_counts_and_a_conversion_to_a_table_does_not as fn(),
                ),
                (
                    "tests_are_counted_in_tokens_and_not_in_text",
                    tests_are_counted_in_tokens_and_not_in_text as fn(),
                ),
                ("an_added_ignore_counts", an_added_ignore_counts as fn()),
                (
                    "an_exemption_counts_by_its_values_and_not_by_its_comments_or_reasons",
                    an_exemption_counts_by_its_values_and_not_by_its_comments_or_reasons as fn(),
                ),
                (
                    "a_perf_baseline_counts_when_a_count_rises_or_leaves_and_not_when_it_falls",
                    a_perf_baseline_counts_when_a_count_rises_or_leaves_and_not_when_it_falls
                        as fn(),
                ),
                (
                    "an_allowlist_file_counts_by_its_growth",
                    an_allowlist_file_counts_by_its_growth as fn(),
                ),
                (
                    "an_allowlist_grows_by_a_new_entry_a_higher_count_or_a_new_name_and_not_by_shrinking",
                    an_allowlist_grows_by_a_new_entry_a_higher_count_or_a_new_name_and_not_by_shrinking
                        as fn(),
                ),
                (
                    "a_body_names_a_path_only_inside_the_section_and_with_a_reason",
                    a_body_names_a_path_only_inside_the_section_and_with_a_reason as fn(),
                ),
                (
                    "name_status_reads_nul_separated_pairs",
                    name_status_reads_nul_separated_pairs as fn(),
                ),
            ],
        );
    }
}
