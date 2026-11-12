//! Which changed files can change what `cargo xtask send-positions` reports.
//!
//! The gate (`api-contracts`) lists every place the facade's public API names
//! `Send` or `Sync` ([`crate::send_positions`]). Its report changes when a
//! change writes or removes such a marker, changes what the facade re-exports,
//! or renames or moves an item the list names. A pull request runs the gate
//! only when its diff does one of those ([`changed_position`]); the list and
//! the reached crates are read on each run, so the rule follows them with no
//! copy to keep in step (ADR-0303 §9).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::classify::{Repo, Workspace};
use super::diagnostics::{diff_lines, in_tests, is_rust};
use crate::send_positions::{ALLOWLIST, ROOTS, listed_items};

/// The first of `files` that can change what the gate reports, or `None`:
///
/// - the gate's list, rule or rustdoc JSON loader ([`is_the_gate`]);
/// - a library source of a crate the facade reaches whose changed lines
///   (`changed_lines`, a unified diff; without one, its whole text) name
///   `Send` or `Sync`, or declare a `pub use` or `pub mod`, which decides what
///   the facade re-exports, or sit inside a multi-line `pub use`
///   ([`in_a_public_use`]);
/// - a changed line, in the file that defines an item the list names, that
///   holds a name of the item's path there (`AppConfig`,
///   `with_memory_reclaim`): a rename or a move takes the path off its entry,
///   on whichever line the bound sits. Without a diff, any change to that
///   file.
///
/// A bound that becomes public through a change elsewhere (a private item
/// made `pub` on a line that names no marker) is left to the gate on `main`.
pub(super) fn changed_position<'f>(
    repo: &Repo,
    files: &'f [String],
    changed_lines: Option<&str>,
) -> anyhow::Result<Option<&'f str>> {
    if let Some(found) = files.iter().map(String::as_str).find(|f| is_the_gate(f)) {
        return Ok(Some(found));
    }
    if !files.iter().any(|f| is_rust(f)) {
        return Ok(None);
    }
    let ws = repo.workspace()?;
    let mut reaches_the_facade = BTreeMap::new();
    let mut in_the_api = |f: &str| -> bool {
        if !is_rust(f) || in_tests(f) {
            return false;
        }
        let Some(pkg) = ws.owning_package(f) else {
            return false;
        };
        let Some(package) = ws.packages.get(pkg) else {
            return false;
        };
        let dir = package.dir(&repo.root);
        let src = if dir.is_empty() {
            "src/".to_owned()
        } else {
            format!("{dir}/src/")
        };
        f.starts_with(&src)
            && *reaches_the_facade.entry(pkg.to_owned()).or_insert_with(|| {
                ws.affected(&BTreeSet::from([pkg.to_owned()]))
                    .iter()
                    .any(|dependent| ROOTS.contains(&dependent.as_str()))
            })
    };
    let api: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|f| in_the_api(f))
        .collect();
    if api.is_empty() {
        return Ok(None);
    }
    let defining = defining_files(repo, ws)?;
    let found = match changed_lines {
        Some(diff) => diff_lines(diff)
            .filter(|(file, line)| {
                names_a_position(line)
                    || defining
                        .get(*file)
                        .is_some_and(|names| words(line).any(|word| names.contains(word)))
            })
            .find_map(|(file, _)| api.iter().copied().find(|f| *f == file))
            .or_else(|| in_a_public_use(repo, diff, &api)),
        None => api.iter().copied().find(|f| {
            defining.contains_key(*f)
                || std::fs::read_to_string(repo.root.join(f))
                    .is_ok_and(|text| text.lines().any(names_a_position))
        }),
    };
    Ok(found)
}

/// The first file of `api` whose diff changes a line inside a `pub use`
/// statement that spans several lines (`pub use a::{\n    B,\n};`): an
/// interior line names no `pub use` itself. The statement is read from the
/// file on the side the line belongs to, by the blob the diff's `index` line
/// names; a diff without one (a synthetic one) has no sides to read.
fn in_a_public_use<'a>(repo: &Repo, diff: &str, api: &[&'a str]) -> Option<&'a str> {
    let mut file: Option<&'a str> = None;
    let (mut old, mut new) = (BTreeSet::new(), BTreeSet::new());
    let (mut old_line, mut new_line) = (0_usize, 0_usize);
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            let path = rest.split_once(" b/").map_or(rest, |(a, _)| a);
            file = api.iter().copied().find(|f| *f == path);
            old.clear();
            new.clear();
        } else if let Some(blobs) = line.strip_prefix("index ") {
            let blobs = blobs.split(' ').next().unwrap_or(blobs);
            if let (Some(_), Some((a, b))) = (file, blobs.split_once("..")) {
                old = public_use_lines(repo, a);
                new = public_use_lines(repo, b);
            }
        } else if line.starts_with("+++ ") || line.starts_with("--- ") {
            // the file names, read from `diff --git`
        } else if let Some(header) = line.strip_prefix("@@ -") {
            let mut ranges = header.split(' ');
            let start = |range: Option<&str>| {
                range
                    .and_then(|r| r.trim_start_matches('+').split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0)
            };
            old_line = start(ranges.next());
            new_line = start(ranges.next());
        } else if line.starts_with('-') {
            if file.is_some() && old.contains(&old_line) {
                return file;
            }
            old_line += 1;
        } else if line.starts_with('+') {
            if file.is_some() && new.contains(&new_line) {
                return file;
            }
            new_line += 1;
        }
    }
    None
}

/// The 1-based lines of the blob `id` that sit inside a `pub use` statement,
/// from its first line to the one that ends it with `;`; none for a missing
/// side (`0000000`) or an unreadable blob.
fn public_use_lines(repo: &Repo, id: &str) -> BTreeSet<usize> {
    if id.bytes().all(|b| b == b'0') {
        return BTreeSet::new();
    }
    let Ok(text) = repo.git(&["cat-file", "blob", id]) else {
        return BTreeSet::new();
    };
    let mut lines = BTreeSet::new();
    let mut inside = false;
    for (n, line) in text.lines().enumerate() {
        inside |= line.trim_start().starts_with("pub use ");
        if inside {
            lines.insert(n + 1);
            inside = !line.contains(';');
        }
    }
    lines
}

/// Whether `path` is the gate itself: its list, its rule, or the rustdoc JSON
/// loader it shares with `api-closure`. Their self-tests read built-in
/// documents, so only a run over the workspace sees what a change to them
/// does to the report.
fn is_the_gate(path: &str) -> bool {
    path == ALLOWLIST
        || path == "tools/xtask/src/send_positions.rs"
        || path == "tools/xtask/src/api_closure.rs"
        || path.starts_with("tools/xtask/src/api_closure/")
}

/// Whether a line of code names a marker (`Send`, `Sync` as a whole word) or
/// declares a public re-export or module; a comment line does not.
fn names_a_position(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    words(code).any(|word| word == "Send" || word == "Sync")
        || code.contains("pub use ")
        || code.contains("pub mod ")
}

/// The identifiers and keywords of `line`.
fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// The repo-relative source file of each item the list names, with the names
/// its path takes inside that file (`AppConfig`, `with_memory_reclaim`): the
/// module file of its longest module path that exists in its crate (`a::b` is
/// `src/a/b.rs` or `src/a/b/mod.rs`, the crate root `src/lib.rs`).
fn defining_files(
    repo: &Repo,
    ws: &Workspace,
) -> anyhow::Result<BTreeMap<String, BTreeSet<String>>> {
    let text = std::fs::read_to_string(repo.root.join(ALLOWLIST))?;
    let crates: BTreeMap<String, String> = ws
        .packages
        .values()
        .map(|p| (p.name.replace('-', "_"), p.dir(&repo.root)))
        .collect();
    let mut files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in listed_items(&text)? {
        // `impl Trait for Type` and `impl Type` name their paths among other
        // words: each workspace path in the label resolves on its own
        for path in item
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
            .filter(|token| token.contains("::"))
        {
            let mut segments = path.split("::");
            let Some(dir) = segments.next().and_then(|ident| crates.get(ident)) else {
                continue;
            };
            let segments: Vec<&str> = segments.collect();
            if let Some((file, inside)) = (0..=segments.len()).rev().find_map(|k| {
                module_files(dir, &segments[..k])
                    .into_iter()
                    .find(|file| repo.root.join(file).is_file())
                    .map(|file| (file, &segments[k..]))
            }) {
                files
                    .entry(file)
                    .or_default()
                    .extend(inside.iter().map(|&name| name.to_owned()));
            }
        }
    }
    Ok(files)
}

/// The files that can hold the module `path` of the crate in `dir`.
fn module_files(dir: &str, path: &[&str]) -> Vec<String> {
    let src = Path::new(dir).join("src");
    let paths = if path.is_empty() {
        vec![src.join("lib.rs")]
    } else {
        let module = path.join("/");
        vec![
            src.join(format!("{module}.rs")),
            src.join(&module).join("mod.rs"),
        ]
    };
    paths
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::classify::tests::repo;
    use super::*;

    fn reaches(file: &str, line: Option<&str>) -> Option<String> {
        let diff = line.map(|line| {
            format!(
                "diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n{line}\n"
            )
        });
        changed_position(repo(), &[file.to_owned()], diff.as_deref())
            .expect("reading the list")
            .map(str::to_owned)
    }

    /// A changed line of a reached crate's library that writes or removes a
    /// marker, or a public re-export, reaches the gate; any other line of the
    /// same file, a comment, a test and tooling do not.
    fn a_changed_marker_reaches_the_gate() {
        let container = "crates/flui-widgets/src/container.rs";
        for line in [
            "+    pub fn on_tap(self, f: impl Fn() + Send + 'static) -> Self {",
            "-pub fn spawn<T: Sync>(value: T) {",
            "+pub use crate::basic::Padding;",
            "-pub mod decoration;",
        ] {
            assert_eq!(
                reaches(container, Some(line)).as_deref(),
                Some(container),
                "{line}"
            );
        }
        for line in [
            "+    let padding = EdgeInsets::all(8.0);",
            "+    /// The callback is `Send` when the closure is.",
            "+    let sender = Sendable::new();",
        ] {
            assert_eq!(reaches(container, Some(line)), None, "{line}");
        }
        let line = Some("+pub fn spawn<T: Send>(value: T) {}");
        for misses in [
            "crates/flui-widgets/tests/main.rs",
            "tools/xtask/src/perf.rs",
            "docs/testing.md",
        ] {
            assert_eq!(reaches(misses, line), None, "{misses}");
        }
        // the gate's list, rule and loader, whose self-tests read built-in
        // documents, on any change
        for gate in [
            "tools/xtask/allowlists/send-positions.toml",
            "tools/xtask/src/send_positions.rs",
            "tools/xtask/src/api_closure.rs",
        ] {
            assert_eq!(reaches(gate, None).as_deref(), Some(gate), "{gate}");
        }
    }

    /// A line that names a listed item, in the file that defines it, reaches
    /// the gate, so a rename on a line apart from its bound is seen; another
    /// line of that file and the same line in another file do not.
    fn a_listed_items_name_reaches_the_gate() {
        let config = "crates/flui-app/src/app/config.rs";
        let rename = Some("-    pub fn with_memory_reclaim(");
        assert_eq!(reaches(config, rename).as_deref(), Some(config));
        assert_eq!(reaches(config, Some("+    let frames = 3;")), None);
        // an implementation's entry names its paths after `impl` (and `for`)
        let lifecycle = "crates/flui-app/src/app/lifecycle.rs";
        let method = Some("-    pub fn spawn_io<T, F, Fut>(");
        assert_eq!(reaches(lifecycle, method).as_deref(), Some(lifecycle));
        assert_eq!(
            reaches("crates/flui-widgets/src/container.rs", rename),
            None
        );
    }

    /// An interior line of a multi-line `pub use` reaches the gate, on the
    /// side of the diff it belongs to; a line outside the statement does not.
    fn a_line_inside_a_public_use_reaches_the_gate() {
        let facade = "src/rendering.rs";
        let blob = repo()
            .git(&["rev-parse", &format!("HEAD:{facade}")])
            .expect("the facade's blob");
        let text = std::fs::read_to_string(repo().root.join(facade)).expect("the facade");
        let lines: Vec<&str> = text.lines().collect();
        let start = lines
            .iter()
            .position(|l| l.starts_with("pub use ") && !l.contains(';'))
            .expect("a multi-line `pub use` in the facade");
        let interior = lines[start + 1];
        assert!(!names_a_position(interior), "{interior}");
        let diff = |sign: char, at: usize, line: &str| {
            let blob = blob.trim();
            let side = if sign == '-' {
                format!("-{at},1 +{},0", at - 1)
            } else {
                format!("-{},0 +{at},1", at - 1)
            };
            format!(
                "diff --git a/{facade} b/{facade}\nindex {blob}..{blob} 100644\n--- a/{facade}\n+++ b/{facade}\n@@ {side} @@\n{sign}{line}\n"
            )
        };
        let reaches = |diff: String| {
            changed_position(repo(), &[facade.to_owned()], Some(&diff))
                .expect("reading the list")
                .map(str::to_owned)
        };
        for sign in ['-', '+'] {
            assert_eq!(
                reaches(diff(sign, start + 2, interior)).as_deref(),
                Some(facade),
                "{sign}{interior}"
            );
        }
        // the first line of the file is the crate's documentation
        assert_eq!(reaches(diff('-', 1, lines[0])), None, "{}", lines[0]);
    }

    /// Without a diff (`--files`), the whole text of a changed file decides.
    fn a_file_list_reads_the_whole_file() {
        let config = "crates/flui-app/src/app/config.rs";
        assert_eq!(reaches(config, None).as_deref(), Some(config));
        assert_eq!(reaches("crates/flui-widgets/src/container.rs", None), None);
    }

    #[test]
    fn send_positions_inputs() {
        crate::table_test::run_table(
            "send_positions_inputs",
            &[
                (
                    "a_changed_marker_reaches_the_gate",
                    a_changed_marker_reaches_the_gate as fn(),
                ),
                (
                    "a_listed_items_name_reaches_the_gate",
                    a_listed_items_name_reaches_the_gate as fn(),
                ),
                (
                    "a_line_inside_a_public_use_reaches_the_gate",
                    a_line_inside_a_public_use_reaches_the_gate as fn(),
                ),
                (
                    "a_file_list_reads_the_whole_file",
                    a_file_list_reads_the_whole_file as fn(),
                ),
            ],
        );
    }
}
