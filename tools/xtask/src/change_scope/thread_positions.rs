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
/// - a library source of a crate the facade reaches whose changed lines
///   (`changed_lines`, a unified diff; without one, its whole text) name
///   `Send` or `Sync`, or declare a `pub use` or `pub mod`, which decides what
///   the facade re-exports;
/// - the file that defines an item the list names: a rename there moves the
///   item's path off its entry, on whichever line the bound sits.
///
/// A bound that becomes public through a change elsewhere (a private item
/// made `pub` on a line that names no marker) is left to the gate on `main`.
pub(super) fn changed_position<'f>(
    repo: &Repo,
    files: &'f [String],
    changed_lines: Option<&str>,
) -> anyhow::Result<Option<&'f str>> {
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
    if let Some(found) = api.iter().find(|f| defining.contains(**f)) {
        return Ok(Some(found));
    }
    let found = match changed_lines {
        Some(diff) => diff_lines(diff)
            .filter(|(_, line)| names_a_position(line))
            .find_map(|(file, _)| api.iter().find(|f| **f == file)),
        None => api.iter().find(|f| {
            std::fs::read_to_string(repo.root.join(f))
                .is_ok_and(|text| text.lines().any(names_a_position))
        }),
    };
    Ok(found.copied())
}

/// Whether a line of code names a marker (`Send`, `Sync` as a whole word) or
/// declares a public re-export or module; a comment line does not.
fn names_a_position(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| word == "Send" || word == "Sync")
        || code.contains("pub use ")
        || code.contains("pub mod ")
}

/// The repo-relative source file of each item the list names: the module file
/// of its longest module path that exists in its crate (`a::b` is
/// `src/a/b.rs` or `src/a/b/mod.rs`, the crate root `src/lib.rs`).
fn defining_files(repo: &Repo, ws: &Workspace) -> anyhow::Result<BTreeSet<String>> {
    let text = std::fs::read_to_string(repo.root.join(ALLOWLIST))?;
    let crates: BTreeMap<String, String> = ws
        .packages
        .values()
        .map(|p| (p.name.replace('-', "_"), p.dir(&repo.root)))
        .collect();
    let mut files = BTreeSet::new();
    for item in listed_items(&text)? {
        let mut segments = item.split("::");
        let Some(dir) = segments.next().and_then(|ident| crates.get(ident)) else {
            continue;
        };
        let segments: Vec<&str> = segments.collect();
        if let Some(file) = (0..=segments.len())
            .rev()
            .flat_map(|k| module_files(dir, &segments[..k]))
            .find(|file| repo.root.join(file).is_file())
        {
            files.insert(file);
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
            "tools/xtask/src/send_positions.rs",
            "docs/testing.md",
        ] {
            assert_eq!(reaches(misses, line), None, "{misses}");
        }
    }

    /// The file that defines a listed item reaches the gate on any change, so
    /// a rename on a line apart from its bound is seen; another file of the
    /// same crate is not.
    fn a_listed_items_file_reaches_the_gate() {
        let config = "crates/flui-app/src/app/config.rs";
        let rename = Some("+    pub fn with_reclaim(");
        assert_eq!(reaches(config, rename).as_deref(), Some(config));
        assert_eq!(
            reaches("crates/flui-widgets/src/container.rs", rename),
            None
        );
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
                    "a_listed_items_file_reaches_the_gate",
                    a_listed_items_file_reaches_the_gate as fn(),
                ),
                (
                    "a_file_list_reads_the_whole_file",
                    a_file_list_reads_the_whole_file as fn(),
                ),
            ],
        );
    }
}
