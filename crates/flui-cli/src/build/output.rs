//! Where builds write their deliverables, and which of those directories
//! `flui clean --platform` may remove.
//!
//! Cargo decides where compiled code goes; what FLUI packages from it (an
//! APK, an `.app`, a web page with its `pkg/`) goes beside that, in
//! [`project_output_root`]: `<cargo target-dir>/flui-out/<project>`. It
//! follows `CARGO_TARGET_DIR`, `build.target-dir` and an enclosing
//! workspace the way cargo does, so `cargo clean` removes it too, and the
//! project level keeps projects that share one target-dir apart.
//!
//! A build writes to [`default_output_dir`] unless `--output` names another
//! directory. The default is always the CLI's to remove. An `--output`
//! directory is a build's only if the build found it missing or empty: the
//! build then leaves an [`OWNER_MARKER`] in it naming the owner (the
//! platform and the project), and records the claim in the project's output
//! root. `flui clean` removes a recorded directory only while the marker
//! still names the same owner. A directory that held anything before the
//! first build into it (`--output .`, `--output src`, a shared folder, the
//! output of another platform or project) is never claimed, so it is never
//! removed.
//!
//! The marker is the proof of ownership; the record is only an index of
//! where to look. Each claim is a file of its own holding the directory's
//! path byte for byte, so concurrent builds never overwrite each other's
//! claims and any path the OS accepts survives the round trip. Losing or
//! damaging the record (`cargo clean`, deleting `target/` by hand, a
//! truncated write) never fails a build or a clean: an unreadable claim is
//! skipped, a claim that cannot be written is skipped, and the next build
//! into a claimed directory records it again.

use std::ffi::{OsStr, OsString};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};

use crate::build::platform::BuilderContext;

/// The file a build leaves in an `--output` directory it created, naming
/// the owner `flui clean` checks before removing that directory.
pub(crate) const OWNER_MARKER: &str = ".flui-out";

/// Where the project at `workspace_root` keeps what its builds package:
/// `<target-dir>/flui-out/<project>`, with the target-dir `cargo metadata`
/// reports and the project named after the root package (the workspace
/// directory for a virtual workspace). Two projects of one name sharing a
/// target-dir share this directory, as their same-named binaries share
/// `<target-dir>/<profile>`. Where cargo cannot read the project, the
/// target-dir is `<workspace_root>/target`, cargo's own default.
#[must_use]
pub(crate) fn project_output_root(workspace_root: &Path) -> PathBuf {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(workspace_root)
        .no_deps()
        .exec();
    let (target_dir, project) = match metadata {
        Ok(metadata) => {
            let project = metadata
                .root_package()
                .map(|package| package.name.to_string())
                .or_else(|| metadata.workspace_root.file_name().map(str::to_owned));
            (metadata.target_directory.into_std_path_buf(), project)
        }
        Err(error) => {
            crate::ui::debug(format!(
                "cargo metadata failed in {}: {error}; using its target/",
                workspace_root.display()
            ));
            (workspace_root.join("target"), None)
        }
    };
    let project = project
        .or_else(|| {
            workspace_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "app".to_owned());
    target_dir.join("flui-out").join(project)
}

/// Where a build for `platform` (a `Platform::name`) writes its deliverables
/// when no `--output` is given: `<project output root>/<platform>`.
#[must_use]
pub(crate) fn default_output_dir(workspace_root: &Path, platform: &str) -> PathBuf {
    project_output_root(workspace_root).join(platform)
}

/// The directory of claims builds for `platform` recorded, one file per
/// claimed `--output` directory. It sits beside the default output
/// directory, not in it, so cleaning that directory keeps the claims until
/// the claimed directories are gone too.
fn claims_dir(output_root: &Path, platform: &str) -> PathBuf {
    output_root.join(format!("{platform}.out-dirs"))
}

/// What the owner marker holds: the platform, a NUL, then the canonical
/// project path's OS bytes. Builds for another platform, or from another
/// project, write a different owner.
fn owner(project: &Path, platform: &str) -> Vec<u8> {
    let mut bytes = platform.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(os_bytes(project.as_os_str()));
    bytes
}

/// Whether `dir` carries an owner marker naming `owner`.
fn owned_by(dir: &Path, owner: &[u8]) -> bool {
    std::fs::read(dir.join(OWNER_MARKER)).is_ok_and(|marker| marker == owner)
}

/// Create `ctx.output_dir`. An `--output` directory the build finds missing
/// or empty, or one the same platform of the same project already claimed,
/// gets the owner marker and is recorded for `flui clean`.
///
/// # Errors
///
/// Returns the I/O error of creating the directory, resolving the project or
/// writing the marker. Recording the claim is best effort and never fails the
/// build.
pub(crate) fn prepare_output_dir(ctx: &BuilderContext) -> io::Result<()> {
    let platform = ctx.platform.name();
    let dir = &ctx.output_dir;
    let output_root = project_output_root(&ctx.workspace_root);
    if *dir == output_root.join(platform) {
        return std::fs::create_dir_all(dir);
    }

    let owner = owner(&std::fs::canonicalize(&ctx.workspace_root)?, platform);
    let claimable = match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none() || owned_by(dir, &owner),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    std::fs::create_dir_all(dir)?;
    if !claimable {
        return Ok(());
    }
    std::fs::write(dir.join(OWNER_MARKER), &owner)?;

    let claims = claims_dir(&output_root, platform);
    if let Err(error) = record_claim(&claims, dir) {
        crate::ui::debug(format!(
            "could not record {} in {}: {error}; `flui clean` will find it after the next build",
            dir.display(),
            claims.display()
        ));
    }
    Ok(())
}

/// Record `dir` as a file of its own in `claims`, named after a hash of its
/// path. Rewriting the same claim writes the same bytes, and two claims
/// never share a file, so builds running at once cannot lose one another's.
fn record_claim(claims: &Path, dir: &Path) -> io::Result<()> {
    let bytes = os_bytes(std::fs::canonicalize(dir)?.as_os_str());
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    std::fs::create_dir_all(claims)?;
    std::fs::write(claims.join(format!("{:016x}", hasher.finish())), bytes)
}

/// Remove what builds for `platform` of the project at `workspace_root`
/// wrote under `output_root` (its [`project_output_root`]): every recorded
/// `--output` directory whose marker still names this platform of this
/// project and that is neither the project nor one of its ancestors, then
/// the default output directory and the claims. Returns the directories
/// removed.
///
/// # Errors
///
/// Returns the I/O error of resolving the project or removing a directory.
/// Unreadable or unremovable claims are not an error.
pub(crate) fn clean_output_dirs(
    workspace_root: &Path,
    output_root: &Path,
    platform: &str,
) -> io::Result<Vec<PathBuf>> {
    let claims = claims_dir(output_root, platform);
    let project = std::fs::canonicalize(workspace_root)?;
    let owner = owner(&project, platform);

    let mut removed = Vec::new();
    for dir in read_claims(&claims) {
        if owned_by(&dir, &owner) && !project.starts_with(&dir) {
            std::fs::remove_dir_all(&dir)?;
            removed.push(dir);
        }
    }
    let default = output_root.join(platform);
    if default.exists() {
        std::fs::remove_dir_all(&default)?;
        removed.push(default);
    }
    if let Err(error) = std::fs::remove_dir_all(&claims)
        && error.kind() != io::ErrorKind::NotFound
    {
        crate::ui::debug(format!("could not remove {}: {error}", claims.display()));
    }
    Ok(removed)
}

/// The recorded directories. Missing or unreadable claims are skipped, and a
/// claim whose bytes are not the path of an owned directory fails the
/// marker check in the clean.
fn read_claims(claims: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(claims) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| os_from_bytes(&bytes))
        .map(PathBuf::from)
        .collect()
}

/// A path's OS string, byte for byte: the raw bytes on Unix, the UTF-16
/// code units (little-endian) on Windows, so paths that are not UTF-8 or
/// hold a newline round-trip exactly.
#[cfg(unix)]
fn os_bytes(os: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    os.as_bytes().to_vec()
}

#[cfg(unix)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "one signature with the Windows decoder, which rejects an odd byte count"
)]
fn os_from_bytes(bytes: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt as _;
    Some(OsStr::from_bytes(bytes).to_owned())
}

#[cfg(windows)]
fn os_bytes(os: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    os.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(windows)]
fn os_from_bytes(bytes: &[u8]) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let (units, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let wide: Vec<u16> = units.iter().map(|unit| u16::from_le_bytes(*unit)).collect();
    Some(OsString::from_wide(&wide))
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

    /// A desktop build context for a project at `root` writing to `out`.
    fn desktop_ctx(root: &Path, out: PathBuf) -> BuilderContext {
        BuilderContextBuilder::new(root.to_path_buf())
            .with_platform(Platform::Desktop { target: None })
            .with_profile(Profile::Debug)
            .with_output_dir(out)
            .build()
    }

    fn clean(root: &Path) -> io::Result<Vec<PathBuf>> {
        clean_output_dirs(root, &project_output_root(root), "web")
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
        clean(&root).expect("clean");
        assert!(!out.exists(), "a claimed --output dir survived the clean");
    }

    fn empty_out_dir_is_claimed_and_cleaned() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        std::fs::create_dir_all(&out).expect("empty dir");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        clean(&root).expect("clean");
        assert!(!out.exists(), "a claimed --output dir survived the clean");
    }

    fn a_rebuild_keeps_the_claim() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        let ctx = web_ctx(&root, Some(out.clone()));
        prepare_output_dir(&ctx).expect("first build");
        std::fs::write(out.join("index.html"), "").expect("deliverable");
        prepare_output_dir(&ctx).expect("second build");
        clean(&root).expect("clean");
        assert!(!out.exists(), "a rebuilt --output dir survived the clean");
    }

    fn a_non_empty_out_dir_is_never_removed() {
        let (tmp, root) = project();
        let out = tmp.path().join("shared");
        std::fs::create_dir_all(&out).expect("dir");
        std::fs::write(out.join("notes.txt"), "mine").expect("user file");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        assert!(!out.join(OWNER_MARKER).exists(), "claimed a non-empty dir");
        clean(&root).expect("clean");
        assert!(
            out.join("notes.txt").is_file(),
            "removed a dir it never owned"
        );
    }

    fn the_project_as_out_dir_is_never_removed() {
        let (_tmp, root) = project();
        prepare_output_dir(&web_ctx(&root, Some(root.clone()))).expect("prepare");
        clean(&root).expect("clean");
        assert!(root.join("flui.toml").is_file(), "removed the project");
    }

    fn a_dir_whose_marker_is_gone_is_kept() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        std::fs::remove_file(out.join(OWNER_MARKER)).expect("drop marker");
        clean(&root).expect("clean");
        assert!(out.is_dir(), "removed a dir without the owner marker");
    }

    fn the_default_dir_is_cleaned_without_a_marker() {
        let (_tmp, root) = project();
        let ctx = web_ctx(&root, None);
        prepare_output_dir(&ctx).expect("prepare");
        assert!(!ctx.output_dir.join(OWNER_MARKER).exists());
        std::fs::write(ctx.output_dir.join("index.html"), "").expect("deliverable");
        clean(&root).expect("clean");
        assert!(!ctx.output_dir.exists(), "the default output dir survived");
    }

    fn clean_forgets_the_claims() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        clean(&root).expect("clean");
        // A directory the user makes later at the same path, marker and all,
        // is no longer on record.
        std::fs::create_dir_all(&out).expect("recreate");
        let project = std::fs::canonicalize(&root).expect("project");
        std::fs::write(out.join(OWNER_MARKER), owner(&project, "web")).expect("marker");
        clean(&root).expect("second clean");
        assert!(out.is_dir(), "a forgotten claim was removed again");
    }

    fn a_lost_record_is_rebuilt_by_the_next_build() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        let ctx = web_ctx(&root, Some(out.clone()));
        prepare_output_dir(&ctx).expect("first build");
        std::fs::remove_dir_all(root.join("target")).expect("delete target by hand");
        prepare_output_dir(&ctx).expect("build after target is gone");
        clean(&root).expect("clean");
        assert!(!out.exists(), "the rebuilt claim was not cleaned");
    }

    fn a_damaged_record_breaks_neither_build_nor_clean() {
        let (tmp, root) = project();
        let claims = claims_dir(&project_output_root(&root), "web");
        std::fs::create_dir_all(&claims).expect("claims dir");
        std::fs::write(claims.join("garbage"), [0xff, 0xfe, b'\n', 0x00]).expect("garbage claim");
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("build over garbage");
        clean(&root).expect("clean over garbage");
        assert!(
            !out.exists(),
            "the claim recorded after garbage was not cleaned"
        );
    }

    fn an_unwritable_record_breaks_neither_build_nor_clean() {
        let (tmp, root) = project();
        // A file where the claims directory belongs: reading and writing
        // claims both fail.
        let claims = claims_dir(&project_output_root(&root), "web");
        std::fs::create_dir_all(claims.parent().expect("parent")).expect("output root");
        std::fs::write(&claims, "").expect("claims path taken");
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("build");
        assert!(out.join(OWNER_MARKER).is_file(), "the claim was not marked");
        clean(&root).expect("clean");
    }

    fn another_platforms_output_is_not_claimed() {
        let (tmp, root) = project();
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("web build");
        std::fs::write(out.join("index.html"), "").expect("web deliverable");
        prepare_output_dir(&desktop_ctx(&root, out.clone())).expect("desktop build");
        clean_output_dirs(&root, &project_output_root(&root), "desktop").expect("clean desktop");
        assert!(
            out.join("index.html").is_file(),
            "a desktop clean removed the web build's output"
        );
        clean(&root).expect("clean web");
        assert!(!out.exists(), "the web build's own output survived");
    }

    fn another_projects_output_is_not_claimed() {
        let (tmp, root) = project();
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("other project");
        let out = tmp.path().join("dist");
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("our build");
        std::fs::write(out.join("index.html"), "").expect("our deliverable");
        prepare_output_dir(&web_ctx(&other, Some(out.clone()))).expect("their build");
        clean(&other).expect("their clean");
        assert!(
            out.join("index.html").is_file(),
            "another project's clean removed our output"
        );
    }

    fn concurrent_claims_are_all_recorded() {
        const BUILDS: usize = 16;
        let (tmp, root) = project();
        let claims = claims_dir(&project_output_root(&root), "web");
        let outs: Vec<PathBuf> = (0..BUILDS)
            .map(|build| {
                let out = tmp.path().join(format!("dist-{build}"));
                std::fs::create_dir_all(&out).expect("output dir");
                out
            })
            .collect();
        let barrier = std::sync::Barrier::new(BUILDS);
        std::thread::scope(|scope| {
            for out in &outs {
                let (claims, barrier) = (&claims, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    record_claim(claims, out).expect("record claim");
                });
            }
        });
        assert_eq!(
            read_claims(&claims).len(),
            BUILDS,
            "claims lost to builds recording at once"
        );
    }

    /// A directory name that is not UTF-8: raw bytes with a newline on Unix,
    /// an unpaired surrogate on Windows.
    fn an_out_dir_whose_name_is_not_utf8_is_cleaned() {
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStrExt as _;
            OsStr::from_bytes(b"dist-\xff\nx").to_owned()
        };
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt as _;
            OsString::from_wide(&[0x64, 0x69, 0x73, 0x74, 0xD800])
        };
        assert!(name.to_str().is_none(), "the fixture name is UTF-8");
        let (tmp, root) = project();
        let out = tmp.path().join(name);
        prepare_output_dir(&web_ctx(&root, Some(out.clone()))).expect("prepare");
        clean(&root).expect("clean");
        assert!(
            !out.exists(),
            "a claimed dir with a non-UTF-8 name survived"
        );
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
            (
                "a_lost_record_is_rebuilt_by_the_next_build",
                a_lost_record_is_rebuilt_by_the_next_build,
            ),
            (
                "a_damaged_record_breaks_neither_build_nor_clean",
                a_damaged_record_breaks_neither_build_nor_clean,
            ),
            (
                "an_unwritable_record_breaks_neither_build_nor_clean",
                an_unwritable_record_breaks_neither_build_nor_clean,
            ),
            (
                "another_platforms_output_is_not_claimed",
                another_platforms_output_is_not_claimed,
            ),
            (
                "another_projects_output_is_not_claimed",
                another_projects_output_is_not_claimed,
            ),
            (
                "concurrent_claims_are_all_recorded",
                concurrent_claims_are_all_recorded,
            ),
            (
                "an_out_dir_whose_name_is_not_utf8_is_cleaned",
                an_out_dir_whose_name_is_not_utf8_is_cleaned,
            ),
        ]);
    }
}
