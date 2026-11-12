//! Which changed files can change what a compile-fail case prints.
//!
//! The trybuild suites (`test-trybuild`) hold each case's compiler output in a
//! `*.stderr` expectation. The output quotes, in its `--> path` and `::: path` lines, every
//! source of the error chain: the fixture, and the definition of each type
//! the compiler names (`note: required because it appears within the type
//! … --> crates/<crate>/src/<file>.rs`). A change that leaves all of those
//! files alone leaves the expectation as it is, so a pull request runs the
//! suites only when its diff reaches one of them ([`changed_input`]).
//!
//! The cited files are read from the tracked expectations on each run, so the
//! set follows them with no list to keep in step (ADR-0303 §9).

use std::collections::BTreeSet;
use std::path::Path;

use super::classify::Repo;

/// Inputs of every expectation: the compiler and its standard library (the
/// toolchain file), the locked dependency versions (trybuild's among them),
/// and the test groups that select the suites.
const EVERY_EXPECTATION: &[&str] = &[
    "rust-toolchain.toml",
    "rust-toolchain",
    "Cargo.lock",
    ".config/nextest.toml",
];

/// What a runner of the suites names: a test target that builds cases.
const RUNNER_MARK: &str = "trybuild::TestCases";

/// The repo-relative files the `--> path` lines (the error's location) and
/// `::: path` lines (a secondary location, such as the declaration of a private
/// item) of `expectation` cite.
///
/// trybuild prints a path inside the package that runs the case relative to
/// that package (`src/x.rs`, `tests/ui/x.rs`), a path elsewhere in the
/// workspace under `$WORKSPACE/`, and the toolchain's and registry's under
/// `$RUST/` and `$CARGO/`, which no diff reaches. The running package is not
/// always the one whose directory holds the case (the facade runs
/// `flui-animation`'s), so a relative path is taken both ways: under the
/// directory of the package holding the expectation, and under the root.
pub(super) fn cited_paths(expectation: &str, package_dir: &str) -> BTreeSet<String> {
    let mut cited = BTreeSet::new();
    for line in expectation.lines() {
        let line = line.trim_start();
        let Some(rest) = line
            .strip_prefix("--> ")
            .or_else(|| line.strip_prefix("::: "))
        else {
            continue;
        };
        let path = rest.split(':').next().unwrap_or(rest).trim();
        if let Some(path) = path.strip_prefix("$WORKSPACE/") {
            cited.insert(path.to_owned());
        } else if !path.is_empty() && !path.starts_with('$') {
            if !package_dir.is_empty() {
                cited.insert(format!("{package_dir}/{path}"));
            }
            cited.insert(path.to_owned());
        }
    }
    cited
}

/// The directory of the package that holds `path` (repo-relative): the
/// nearest ancestor with a `Cargo.toml`, `""` for the root package.
fn package_dir(root: &Path, path: &str) -> String {
    let mut dir = Path::new(path).parent();
    while let Some(d) = dir {
        if d.as_os_str().is_empty() {
            break;
        }
        if root.join(d).join("Cargo.toml").is_file() {
            return d.to_string_lossy().replace('\\', "/");
        }
        dir = d.parent();
    }
    String::new()
}

/// The first of `files` that can change what a trybuild suite reports, or
/// `None`:
///
/// - a file an expectation cites;
/// - a file in a case directory: beside an expectation, or named by a
///   runner's `pass` or `compile_fail` path (a passing case has no
///   expectation, and runs its assertions);
/// - a runner of the suites;
/// - the source of a workspace procedural macro, whose diagnostics point at
///   the invocation and are copied into the expectations;
/// - an input of every expectation ([`EVERY_EXPECTATION`]).
pub(super) fn changed_input<'f>(
    repo: &Repo,
    files: &'f [String],
) -> anyhow::Result<Option<&'f str>> {
    let listing = repo.git(&["ls-files", "-z", "--", "*.stderr"])?;
    let mut cited = BTreeSet::new();
    let mut case_dirs = BTreeSet::new();
    for expectation in listing.split('\0').filter(|f| !f.is_empty()) {
        let Ok(text) = std::fs::read_to_string(repo.root.join(expectation)) else {
            continue;
        };
        cited.extend(cited_paths(&text, &package_dir(&repo.root, expectation)));
        case_dirs.insert(parent(expectation).to_owned());
    }
    let runners = repo.git(&["grep", "-l", "-z", "-F", RUNNER_MARK, "--", "*.rs"])?;
    for runner in runners.split('\0').filter(|f| in_tests(f)) {
        let Ok(text) = std::fs::read_to_string(repo.root.join(runner)) else {
            continue;
        };
        let package = package_dir(&repo.root, runner);
        case_dirs.extend(case_paths(&text).map(|case| join(&package, parent(case))));
    }
    if let Some(found) = files.iter().map(String::as_str).find(|&f| {
        EVERY_EXPECTATION.contains(&f)
            || cited.contains(f)
            || case_dirs.contains(parent(f))
            || is_runner(&repo.root, f)
    }) {
        return Ok(Some(found));
    }
    // only a file a package owns can be a macro's source: documentation and
    // tooling alone never read the workspace's metadata
    if !files.iter().any(|f| is_rust(f)) {
        return Ok(None);
    }
    let ws = repo.workspace()?;
    let macro_crates: BTreeSet<&str> = ws
        .packages
        .values()
        .filter(|pkg| {
            pkg.targets
                .iter()
                .any(|t| t.kind.iter().any(|k| k == "proc-macro"))
        })
        .map(|pkg| pkg.name.as_str())
        .collect();
    Ok(files.iter().map(String::as_str).find(|f| {
        is_rust(f)
            && !in_tests(f)
            && ws
                .owning_package(f)
                .is_some_and(|pkg| macro_crates.contains(pkg))
    }))
}

/// The case paths a runner hands trybuild: the string literal of each
/// `.pass("…")` and `.compile_fail("…")` call, a file or a glob, relative to
/// the runner's package.
fn case_paths(runner: &str) -> impl Iterator<Item = &str> {
    [".pass(\"", ".compile_fail(\""]
        .into_iter()
        .flat_map(move |call| {
            runner
                .match_indices(call)
                .filter_map(move |(at, _)| runner[at + call.len()..].split('"').next())
        })
}

/// `dir/path`, or `path` when `dir` is the root.
fn join(dir: &str, path: &str) -> String {
    match (dir.is_empty(), path.is_empty()) {
        (true, _) => path.to_owned(),
        (false, true) => dir.to_owned(),
        (false, false) => format!("{dir}/{path}"),
    }
}

/// Whether `path` is a Rust source file.
fn is_rust(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs")
}

/// Whether `path` sits under a `tests/` directory.
fn in_tests(path: &str) -> bool {
    path.starts_with("tests/") || path.contains("/tests/")
}

/// `path`'s directory, `""` at the root.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Whether `path` is a test target that builds trybuild cases: a Rust file
/// under a `tests/` directory that names [`RUNNER_MARK`]; `false` when it is
/// gone (a deleted runner changes no expectation it no longer runs).
fn is_runner(root: &Path, path: &str) -> bool {
    in_tests(path)
        && is_rust(path)
        && std::fs::read_to_string(root.join(path)).is_ok_and(|text| text.contains(RUNNER_MARK))
}

#[cfg(test)]
mod tests {
    use super::super::classify::tests::repo;
    use super::*;

    fn input(files: &[&str]) -> Option<String> {
        let files: Vec<String> = files.iter().map(|&f| f.to_owned()).collect();
        changed_input(repo(), &files)
            .expect("reading the expectations")
            .map(str::to_owned)
    }

    fn a_cited_path_resolves_by_its_prefix() {
        let text = "\
error[E0277]: `Rc<X>` cannot be sent between threads safely
  --> tests/ui/case.rs:13:30
note: required because it appears within the type `X`
  --> $WORKSPACE/crates/flui-foundation/src/notifier.rs
   |
   | pub struct X {
  --> $RUST/core/src/marker.rs
  --> $CARGO/registry/src/some-crate/src/lib.rs
  ::: src/owned.rs
";
        let cited = cited_paths(text, "crates/flui-view");
        let expected: BTreeSet<String> = [
            "crates/flui-foundation/src/notifier.rs",
            "crates/flui-view/tests/ui/case.rs",
            "tests/ui/case.rs",
            "crates/flui-view/src/owned.rs",
            "src/owned.rs",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(cited, expected);
        // the root package's paths are already repo-relative
        assert_eq!(
            cited_paths("  --> crates/a/src/b.rs:1:2\n", ""),
            ["crates/a/src/b.rs".to_owned()].into()
        );
    }

    /// The type an expectation names, the expectation, its case, a runner and
    /// the toolchain each reach a suite; a source of the same crate that no
    /// expectation cites, and documentation, do not.
    fn a_diff_reaches_the_suites_through_what_they_cite() {
        for reaches in [
            // `WidgetStatesController`, quoted by the facade's thread-boundary case
            "crates/flui-widgets/src/widget_state.rs",
            // the declaration flui-runtime's privacy case cites only as `::: src/…`
            "crates/flui-runtime/src/ui_runtime/frame.rs",
            // a passing case, which no expectation sits beside, and a new one
            "crates/flui-widgets/tests/routable_ui/pass/basic.rs",
            "crates/flui-widgets/tests/routable_ui/pass/a_new_case.rs",
            // the derive whose error text `routable_ui/fail/*.stderr` copies
            "crates/flui-macros/src/derive_routable.rs",
            "tests/ui/thread_boundary/owner_values_stay_on_their_thread.stderr",
            "tests/ui/thread_boundary/owner_values_stay_on_their_thread.rs",
            "tests/ui/thread_boundary/a_new_case.rs",
            "tests/thread_boundary_ui.rs",
            "crates/flui-painting/tests/compile_fail.rs",
            "rust-toolchain.toml",
            "Cargo.lock",
        ] {
            assert_eq!(input(&["README.md", reaches]).as_deref(), Some(reaches));
        }
        for misses in [
            "crates/flui-widgets/src/container.rs",
            "crates/flui-view/src/element/behavior.rs",
            "docs/testing.md",
            "tools/xtask/src/change_scope/diagnostics.rs",
        ] {
            assert_eq!(input(&[misses]), None, "{misses}");
        }
    }

    #[test]
    fn trybuild_inputs() {
        crate::table_test::run_table(
            "trybuild_inputs",
            &[
                (
                    "a_cited_path_resolves_by_its_prefix",
                    a_cited_path_resolves_by_its_prefix as fn(),
                ),
                (
                    "a_diff_reaches_the_suites_through_what_they_cite",
                    a_diff_reaches_the_suites_through_what_they_cite as fn(),
                ),
            ],
        );
    }
}
