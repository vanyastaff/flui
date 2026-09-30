//! Where builds write their deliverables, and which of those directories
//! `flui clean --platform` may remove.
//!
//! A build writes to [`default_output_dir`] unless `--out` names another
//! directory. The default is always the CLI's to remove. An `--out`
//! directory is the CLI's only if the build found it missing or empty: the
//! build then leaves an [`OWNER_MARKER`] in it and records its path under
//! `target/flui-out/`, and `flui clean --platform` removes a recorded
//! directory only while the marker is still there. A directory that held
//! anything before the first build into it (`--out .`, `--out src`, a shared
//! folder) is never claimed, so it is never removed.

use std::io;
use std::path::{Path, PathBuf};

use crate::build::platform::BuilderContext;

/// The file a build leaves in an `--out` directory it created, the proof
/// `flui clean` looks for before removing that directory.
pub(crate) const OWNER_MARKER: &str = ".flui-out";

/// Where a build for `platform` (a `Platform::name`) writes its deliverables
/// when no `--out` is given: `target/flui-out/<platform>` under the project.
#[must_use]
pub(crate) fn default_output_dir(workspace_root: &Path, platform: &str) -> PathBuf {
    workspace_root
        .join("target")
        .join("flui-out")
        .join(platform)
}

/// The file listing the `--out` directories builds for `platform` claimed,
/// one absolute path per line. It sits beside the default output directory,
/// not in it, so cleaning that directory keeps the list until the recorded
/// directories are gone too.
fn claims_file(workspace_root: &Path, platform: &str) -> PathBuf {
    workspace_root
        .join("target")
        .join("flui-out")
        .join(format!("{platform}.out-dirs"))
}

/// Create `ctx.output_dir`. An `--out` directory the build finds missing or
/// empty, or one an earlier build already claimed, gets the owner marker and
/// is recorded for `flui clean --platform`.
///
/// # Errors
///
/// Returns the I/O error of creating the directory, writing the marker or
/// recording the claim.
pub(crate) fn prepare_output_dir(ctx: &BuilderContext) -> io::Result<()> {
    let platform = ctx.platform.name();
    let dir = &ctx.output_dir;
    if *dir == default_output_dir(&ctx.workspace_root, platform) {
        return std::fs::create_dir_all(dir);
    }

    let claimable = match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none() || dir.join(OWNER_MARKER).is_file(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    std::fs::create_dir_all(dir)?;
    if !claimable {
        return Ok(());
    }
    std::fs::write(dir.join(OWNER_MARKER), "")?;

    let claimed = std::fs::canonicalize(dir)?;
    let claims = claims_file(&ctx.workspace_root, platform);
    let mut recorded = read_claims(&claims)?;
    if !recorded.contains(&claimed) {
        recorded.push(claimed);
        if let Some(parent) = claims.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&claims, join_claims(&recorded))?;
    }
    Ok(())
}

/// Remove what builds for `platform` wrote: every recorded `--out` directory
/// that still carries the owner marker and is neither the project nor one of
/// its ancestors, then the default output directory and the record itself.
/// Returns the directories removed.
///
/// # Errors
///
/// Returns the I/O error of reading the record or removing a directory.
pub(crate) fn clean_output_dirs(workspace_root: &Path, platform: &str) -> io::Result<Vec<PathBuf>> {
    let claims = claims_file(workspace_root, platform);
    let project = std::fs::canonicalize(workspace_root)?;

    let mut removed = Vec::new();
    for dir in read_claims(&claims)? {
        let owned = dir.join(OWNER_MARKER).is_file() && !project.starts_with(&dir);
        if owned {
            std::fs::remove_dir_all(&dir)?;
            removed.push(dir);
        }
    }
    let default = default_output_dir(workspace_root, platform);
    if default.exists() {
        std::fs::remove_dir_all(&default)?;
        removed.push(default);
    }
    if let Err(error) = std::fs::remove_file(&claims)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }
    Ok(removed)
}

fn read_claims(claims: &Path) -> io::Result<Vec<PathBuf>> {
    match std::fs::read_to_string(claims) {
        Ok(text) => Ok(text
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn join_claims(dirs: &[PathBuf]) -> String {
    let mut text = String::new();
    for dir in dirs {
        text.push_str(&dir.to_string_lossy());
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{BuilderContextBuilder, Platform, Profile};

    /// A web build context for a project at `root` writing to `out`.
    fn web_ctx(root: &Path, out: Option<PathBuf>) -> BuilderContext {
        let builder = BuilderContextBuilder::new(root.to_path_buf())
            .with_platform(Platform::Web {
                target: "web".to_string(),
            })
            .with_profile(Profile::Debug);
        match out {
            Some(out) => builder.with_output_dir(out).build(),
            None => builder.build(),
        }
    }

    /// A project directory with a file in it, and a scratch area beside it.
    fn project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).expect("project dir");
        std::fs::write(root.join("flui.toml"), "").expect("flui.toml");
        (tmp, root)
    }

    fn missing_out_dir_is_claimed_and_cleaned() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        std::fs::write(out.join("index.html"), "").expect("deliverable");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(!out.exists(), "a claimed --out dir survived the clean");
    }

    fn empty_out_dir_is_claimed_and_cleaned() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        std::fs::create_dir_all(&out).expect("empty dir");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(!out.exists(), "a claimed --out dir survived the clean");
    }

    fn a_rebuild_keeps_the_claim() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        let ctx = web_ctx(&root, Some(out.clone()));
        prepare_output_dir(&ctx).expect("first build");
        std::fs::write(out.join("index.html"), "").expect("deliverable");
        prepare_output_dir(&ctx).expect("second build");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(!out.exists(), "a rebuilt --out dir survived the clean");
    }

    fn a_non_empty_out_dir_is_never_removed() {
        let (tmp, root) = project();
        let out = tmp.path().join("shared");
        std::fs::create_dir_all(&out).expect("dir");
        std::fs::write(out.join("notes.txt"), "mine").expect("user file");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        assert!(!out.join(OWNER_MARKER).exists(), "claimed a non-empty dir");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(
            out.join("notes.txt").is_file(),
            "removed a dir it never owned"
        );
    }

    fn the_project_as_out_dir_is_never_removed() {
        let (_tmp, root) = project();
        prepare_output_dir(&web_ctx(&root, Some(root.clone()))).expect("prepare");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(root.join("flui.toml").is_file(), "removed the project");
    }

    fn a_dir_whose_marker_is_gone_is_kept() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        std::fs::remove_file(out.join(OWNER_MARKER)).expect("drop marker");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(out.is_dir(), "removed a dir without the owner marker");
    }

    fn the_default_dir_is_cleaned_without_a_marker() {
        let (_tmp, root) = project();
        let ctx = web_ctx(&root, None);
        prepare_output_dir(&ctx).expect("prepare");
        assert!(!ctx.output_dir.join(OWNER_MARKER).exists());
        std::fs::write(ctx.output_dir.join("index.html"), "").expect("deliverable");
        clean_output_dirs(&root, "web").expect("clean");
        assert!(!ctx.output_dir.exists(), "the default output dir survived");
    }

    fn clean_forgets_the_claims() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        clean_output_dirs(&root, "web").expect("clean");
        // A directory the user makes later at the same path, marker and all,
        // is no longer on record.
        std::fs::create_dir_all(&out).expect("recreate");
        std::fs::write(out.join(OWNER_MARKER), "").expect("marker");
        clean_output_dirs(&root, "web").expect("second clean");
        assert!(out.is_dir(), "a forgotten claim was removed again");
    }

    #[test]
    fn clean_removes_only_output_dirs_a_build_claimed() {
        crate::test_cases::run_cases(&[
            (
                "missing_out_dir_is_claimed_and_cleaned",
                missing_out_dir_is_claimed_and_cleaned,
            ),
            (
                "empty_out_dir_is_claimed_and_cleaned",
                empty_out_dir_is_claimed_and_cleaned,
            ),
            ("a_rebuild_keeps_the_claim", a_rebuild_keeps_the_claim),
            (
                "a_non_empty_out_dir_is_never_removed",
                a_non_empty_out_dir_is_never_removed,
            ),
            (
                "the_project_as_out_dir_is_never_removed",
                the_project_as_out_dir_is_never_removed,
            ),
            (
                "a_dir_whose_marker_is_gone_is_kept",
                a_dir_whose_marker_is_gone_is_kept,
            ),
            (
                "the_default_dir_is_cleaned_without_a_marker",
                the_default_dir_is_cleaned_without_a_marker,
            ),
            ("clean_forgets_the_claims", clean_forgets_the_claims),
        ]);
    }
}
