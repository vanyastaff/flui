//! Every link from the repository's markdown into the checkout resolves.
//!
//! lychee, offline, over every markdown file git knows outside the archival
//! roots: tracked, or untracked and not ignored, so a new doc is checked
//! before `git add`. A relative link must name a file or directory that
//! exists without climbing above the repository root, and an `#anchor` into a
//! markdown file one of its headings. External URLs are not fetched, so the
//! check cannot flake on the network; links to this repository's own `main`
//! on GitHub are the exception, remapped onto the checkout, so a doc cited by
//! URL is held to the same rule as one cited by path. A root-relative link
//! (`/docs/x.md`) resolves against the repository root, as GitHub renders it.
//!
//! The Windows file system matches names case-insensitively, so a link whose
//! case is wrong passes there and fails on Linux CI.
//!
//! lychee is optional locally, like typos and taplo: without it the check is
//! skipped with a message, unless `--strict` (CI installs it).

use std::fmt::Write as _;
use std::io::{ErrorKind, Write as _};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, bail};

use crate::util::{ScratchDir, repo_root};

/// The lychee CI installs (ci.yml, job `checks`); docs.yml's lychee-action
/// runs the same version over the rendered book.
pub(crate) const LYCHEE_VERSION: &str = "0.24.2";

/// Dated records AGENTS.md exempts from upkeep: a link there that rotted is
/// history, not a defect, and neither `docs-links` nor `markers` reads them.
pub(crate) const ARCHIVAL_ROOTS: [&str; 10] = [
    "docs/archive/",
    "docs/audits/",
    "docs/brainstorms/",
    "docs/ideation/",
    "docs/plans/",
    "docs/research/",
    "docs/superpowers/",
    ".rust-studio/specs/",
    "specs/",
    "openspec/",
];

/// A link to this repository's `main` on GitHub (a regex; the path after it
/// is the file in the checkout).
const SELF_MAIN: &str = r"https://github\.com/vanyastaff/flui/(?:blob|tree)/main/";

/// Arguments for `cargo xtask docs-links`.
#[derive(Debug, clap::Args)]
pub(crate) struct DocsLinksArgs {
    /// Fail when lychee is not installed instead of skipping the check (CI).
    #[arg(long)]
    strict: bool,
}

/// `cargo xtask docs-links`: check the links in the repository's markdown.
pub(crate) fn docs_links(args: &DocsLinksArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "*.md",
        ])
        .current_dir(&root)
        .output()
        .context("running `git ls-files`")?;
    if !listed.status.success() {
        bail!("`git ls-files` failed ({})", listed.status);
    }
    let listed =
        String::from_utf8(listed.stdout).context("`git ls-files` printed a non-UTF-8 path")?;
    let files: Vec<&str> = gated(listed.split_terminator('\0'))
        .into_iter()
        // still in the index, deleted in the working tree: nothing to read
        .filter(|path| root.join(path).is_file())
        .collect();
    check(|| Command::new("lychee"), &root, &files, args.strict)
}

/// The paths of `listed` (repo-relative, `/`-separated, as git prints them)
/// outside the archival roots.
fn gated<'a>(listed: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    listed
        .into_iter()
        .filter(|path| !ARCHIVAL_ROOTS.iter().any(|root| path.starts_with(root)))
        .collect()
}

/// Runs lychee (as `lychee` makes it, with its output wherever that sends
/// it) over `files`, relative to `root`, fed on stdin; then fails the links
/// that passed only because lychee followed them out of the checkout
/// ([`climbing_out`]).
fn check(
    lychee: impl Fn() -> Command,
    root: &Path,
    files: &[&str],
    strict: bool,
) -> anyhow::Result<ExitCode> {
    let root_dir = root
        .to_str()
        .context("the repository root is not UTF-8, so no file URL names it")?;
    let spawned = lychee()
        .args(lychee_args(root_dir))
        .current_dir(root)
        .stdin(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(missing(strict)),
        Err(error) => return Err(error).context("spawning `lychee`"),
    };
    let mut stdin = child.stdin.take().expect("BUG: lychee's stdin is piped");
    // A write fails only when lychee exited before reading its input, and then
    // its own message and exit status are what to report.
    let fed = stdin.write_all(files.join("\n").as_bytes());
    drop(stdin);
    let status = child.wait().context("waiting for `lychee`")?;
    if !status.success() {
        // Exit 2 is a broken link, and also an argument lychee rejects (clap's
        // usage error): what a lychee older than CI's does with these flags.
        let version = match installed_version(&lychee) {
            Some(version) if version != LYCHEE_VERSION => format!(
                "; this is lychee {version}, CI runs {LYCHEE_VERSION} ({})",
                install_hint()
            ),
            _ => String::new(),
        };
        eprintln!(
            "docs-links: lychee failed ({status}, {} markdown files), see above{version}",
            files.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    fed.context("writing the file list to `lychee`")?;
    let climbing = climbing_out(&lychee, root, files)?;
    for (file, url) in &climbing {
        eprintln!(
            "docs-links: {file}: a link climbs above the repository root \
             (from a scratch copy it resolves to {url})"
        );
    }
    if !climbing.is_empty() {
        eprintln!(
            "docs-links: {} links leave the checkout, which GitHub cannot follow",
            climbing.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "docs-links: {} markdown files, archival roots excluded",
        files.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// The links of `files` that climb above the repository root, each as the
/// file and the URL it resolves to from a scratch copy of the markdown.
///
/// lychee resolves `../` past the root like any other path, so [`check`]
/// passes such a link wherever the host has a file where it lands, and CI
/// always has one: from the root of its checkout, `.../flui/flui`,
/// `../flui/<any file>` is the checkout again. On GitHub it is a 404. So
/// lychee lists the links of a copy of `files` in a directory that no link
/// names, and a local one that resolves outside the copy climbed out.
fn climbing_out(
    lychee: impl Fn() -> Command,
    root: &Path,
    files: &[&str],
) -> anyhow::Result<Vec<(String, String)>> {
    let copy = ScratchDir::new("docs-links")?;
    for file in files {
        let to = copy.path().join(file);
        let dir = to.parent().expect("BUG: a copied file sits in the copy");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::copy(root.join(file), &to).with_context(|| format!("copying {file}"))?;
    }
    // A file, not stdin: lychee prints while it still reads its inputs, and
    // a stdout pipe nobody reads yet would stall both ends.
    std::fs::write(copy.path().join(INPUTS), files.join("\n"))
        .context("writing lychee's file list")?;
    let copy_dir = copy
        .path()
        .to_str()
        .context("the temp dir is not UTF-8, so no file URL names it")?;
    let output = lychee()
        .args(dump_args(copy_dir))
        .current_dir(copy.path())
        .stdout(Stdio::piped())
        .output()
        .context("running `lychee --dump`")?;
    if !output.status.success() {
        bail!(
            "`lychee --dump` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let name = copy
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("BUG: a scratch directory has a UTF-8 name");
    let listed = String::from_utf8(output.stdout).context("`lychee --dump` printed non-UTF-8")?;
    let mut climbing = Vec::new();
    for line in listed.lines().filter(|line| !line.is_empty()) {
        if let Some((file, url)) = outside(line, name)? {
            climbing.push((file.to_owned(), url.to_owned()));
        }
    }
    Ok(climbing)
}

/// The file list [`climbing_out`] hands lychee, in the copy's root.
const INPUTS: &str = ".lychee-inputs";

/// One line of `lychee --dump --verbose`, `<url> (<file>)`, with
/// ` [excluded]` after a URL the check skips: the file and the URL when the
/// URL is a local path outside the directory named `copy`.
fn outside<'a>(line: &'a str, copy: &str) -> anyhow::Result<Option<(&'a str, &'a str)>> {
    let parsed = line.split_once(" (").and_then(|(url, rest)| {
        let rest = rest.strip_suffix(" [excluded]").unwrap_or(rest);
        Some((url, rest.strip_suffix(')')?))
    });
    let Some((url, file)) = parsed else {
        bail!("`lychee --dump --verbose` printed an unexpected line: {line}");
    };
    let inside = url.split(['/', '#', '?']).any(|segment| segment == copy);
    Ok((url.starts_with("file://") && !inside).then_some((file, url)))
}

/// The version of the lychee `lychee` makes, when `--version` names one.
fn installed_version(lychee: impl Fn() -> Command) -> Option<String> {
    let output = lychee()
        .arg("--version")
        .stdout(Stdio::piped())
        .output()
        .ok()?;
    let printed = String::from_utf8(output.stdout).ok()?;
    Some(printed.trim().strip_prefix("lychee ")?.to_owned())
}

/// lychee's arguments for the checkout at `root`, reading the file list from
/// stdin. `anchor-only`: `#heading` fragments are checked, GitHub's `#L10`
/// line anchors into source files are not (they are not markdown).
fn lychee_args(root: &str) -> Vec<String> {
    let remap = format!("^{SELF_MAIN}(.*)$ {}/$1", file_url(root));
    [
        "--offline",
        "--no-progress",
        "--include-fragments=anchor-only",
        "--root-dir",
        root,
        "--remap",
        &remap,
        "--files-from",
        "-",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// lychee's arguments for listing, not checking, the links of the copy at
/// `copy`, each with the file it is in. No remap: only a relative link can
/// climb out.
fn dump_args(copy: &str) -> [&str; 7] {
    [
        "--dump",
        "--verbose",
        "--no-progress",
        "--root-dir",
        copy,
        "--files-from",
        INPUTS,
    ]
}

/// `dir` as a `file://` URL: `/`-separated, and every byte a URL (or the
/// remap's `$` replacement syntax) would read as syntax percent-encoded.
fn file_url(dir: &str) -> String {
    let path = dir.replace(std::path::MAIN_SEPARATOR, "/");
    // `/home/...` already has the slash that opens the path; `D:/...` does not
    let mut url = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

/// A missing lychee: a failure under `--strict`, otherwise a skip that says
/// how to install it.
fn missing(strict: bool) -> ExitCode {
    let install = install_hint();
    if strict {
        eprintln!(
            "docs-links: lychee is not installed ({install}), and --strict makes that a failure"
        );
        ExitCode::FAILURE
    } else {
        println!(
            "docs-links: lychee not installed, skipped ({install}; `cargo xtask doctor` lists it; CI runs it)"
        );
        ExitCode::SUCCESS
    }
}

/// How to install the lychee CI runs.
pub(crate) fn install_hint() -> String {
    format!("cargo install --locked lychee --version {LYCHEE_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archival_roots_are_left_out() {
        let listed = [
            "README.md",
            "docs/testing.md",
            "docs/designs/2026-06-30-rasterbackend-seam.md",
            "docs/archive/ROADMAP.md",
            "docs/plans/x.md",
            "docs/research/x.md",
            ".rust-studio/specs/001/spec.md",
            ".rust-studio/research/notes.md",
            "specs/004-view-element-core/spec.md",
            "openspec/x.md",
            // a root, not a prefix: these are live docs
            "docs/plans.md",
            "crates/flui-view/specs/x.md",
            "book/src/specs/x.md",
        ];
        assert_eq!(
            gated(listed),
            [
                "README.md",
                "docs/testing.md",
                "docs/designs/2026-06-30-rasterbackend-seam.md",
                ".rust-studio/research/notes.md",
                "docs/plans.md",
                "crates/flui-view/specs/x.md",
                "book/src/specs/x.md",
            ]
        );
    }

    #[test]
    fn the_checkout_root_is_a_file_url() {
        assert_eq!(
            file_url("/home/runner/work/flui/flui"),
            "file:///home/runner/work/flui/flui"
        );
        let windows = format!("D:{}flui-wt", std::path::MAIN_SEPARATOR);
        assert_eq!(file_url(&windows), "file:///D:/flui-wt");
        assert_eq!(file_url("/tmp/a b#1/$x%"), "file:///tmp/a%20b%231/%24x%25");
    }

    #[test]
    fn lychee_runs_offline_with_the_self_url_remapped() {
        assert_eq!(
            lychee_args("/repo").join(" "),
            "--offline --no-progress --include-fragments=anchor-only --root-dir /repo \
             --remap ^https://github\\.com/vanyastaff/flui/(?:blob|tree)/main/(.*)$ file:///repo/$1 \
             --files-from -"
        );
        assert_eq!(
            dump_args("/copy").join(" "),
            "--dump --verbose --no-progress --root-dir /copy --files-from .lychee-inputs"
        );
    }

    #[test]
    fn a_dumped_link_outside_the_copy_climbed_out() {
        let copy = "xtask-docs-links-7-0-9";
        let dumped = |line| outside(line, copy).expect("parses");
        let inside = [
            "file:///tmp/xtask-docs-links-7-0-9/docs/x.md#a-heading (README.md)",
            "file:///tmp/xtask-docs-links-7-0-9 (docs/x.md)",
            "file:///C:/Temp/xtask-docs-links-7-0-9/crates (docs/x.md)",
            // not a local path: nothing to climb out of
            "https://example.com/x (README.md) [excluded]",
            "https://github.com/vanyastaff/flui/blob/main/x.md (README.md)",
        ];
        for line in inside {
            assert_eq!(dumped(line), None, "{line}");
        }
        assert_eq!(
            dumped("file:///tmp/flui/README.md (README.md)"),
            Some(("README.md", "file:///tmp/flui/README.md"))
        );
        // a sibling whose name starts like the copy's is still outside it
        assert_eq!(
            dumped("file:///tmp/xtask-docs-links-7-0-99/x.md (docs/a b.md)"),
            Some(("docs/a b.md", "file:///tmp/xtask-docs-links-7-0-99/x.md"))
        );
        let error = outside("file:///tmp/x.md", copy).expect_err("no file named");
        assert!(error.to_string().contains("file:///tmp/x.md"), "{error}");
    }

    #[test]
    fn a_missing_lychee_fails_only_under_strict() {
        let root = repo_root();
        let absent = || Command::new("flui-xtask-no-such-lychee");
        assert_eq!(
            check(absent, &root, &[], true).expect("runs"),
            ExitCode::FAILURE
        );
        assert_eq!(
            check(absent, &root, &[], false).expect("runs"),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn ci_installs_the_version_doctor_names() {
        let ci = crate::util::read(".github/workflows/ci.yml").expect("ci.yml");
        let pinned = ci
            .lines()
            .find_map(|line| line.trim().strip_prefix("LYCHEE_VERSION:"))
            .map(str::trim);
        assert_eq!(pinned, Some(LYCHEE_VERSION));
    }

    /// The flags against a real lychee: each kind of broken link fails, and
    /// the good ones pass. Skipped without lychee; the `checks` CI job, which
    /// installs it before running these tests, is where it always runs.
    #[test]
    fn lychee_catches_each_kind_of_broken_link() {
        if Command::new("lychee").arg("--version").output().is_err() {
            eprintln!("lychee not installed: skipped");
            return;
        }
        // CI's layout: the checkout is `.../flui/flui`, with a file beside it
        let base =
            std::env::temp_dir().join(format!("xtask-docs-links-test-{}", std::process::id()));
        let root = base.join("flui");
        std::fs::create_dir_all(root.join("docs")).expect("scratch dir");
        std::fs::write(root.join("docs/target.md"), "# A heading\n").expect("target");
        std::fs::write(base.join("outside.md"), "# Outside\n").expect("outside");
        let quiet = || {
            let mut lychee = Command::new("lychee");
            lychee.stdout(Stdio::null()).stderr(Stdio::null());
            lychee
        };
        let run = |name: &str, body: &str| {
            let file = format!("docs/{name}.md");
            std::fs::write(root.join(&file), body).expect("input");
            check(quiet, &root, &[file.as_str()], true).expect("lychee runs")
        };
        let good = "[rel](target.md#a-heading) [dir](../docs) [abs](/docs/target.md) \
                    [self](https://github.com/vanyastaff/flui/blob/main/docs/target.md#a-heading) \
                    [ext](https://example.com/nowhere)";
        // on a failure the inputs stay in `root`, for a rerun by hand
        assert_eq!(run("good", good), ExitCode::SUCCESS, "{}", root.display());
        for (name, broken) in [
            ("file", "[x](missing.md)"),
            ("depth", "[x](../../docs/target.md)"),
            ("anchor", "[x](target.md#no-such-heading)"),
            (
                "self",
                "[x](https://github.com/vanyastaff/flui/blob/main/docs/missing.md)",
            ),
            ("absolute", "[x](/docs/missing.md)"),
            // files that exist, reached from above the repository root
            ("above", "[x](../../outside.md)"),
            ("reentry", "[x](../../flui/docs/target.md)"),
        ] {
            assert_eq!(
                run(name, broken),
                ExitCode::FAILURE,
                "{name}: {broken} in {}",
                root.display()
            );
        }
        std::fs::remove_dir_all(&base).expect("scratch dir removed");
    }
}
