//! Where builds write their deliverables, and how `flui clean` removes them.
//!
//! Cargo decides where compiled code goes; what FLUI packages from it (an
//! APK, an `.app`, a web page with its `pkg/`) goes beside that, in
//! [`project_output_root`]: `<cargo target-dir>/flui-out/<project>`. It
//! follows `CARGO_TARGET_DIR`, `build.target-dir` and an enclosing
//! workspace the way cargo does, so `cargo clean` removes it too.
//!
//! A build writes to [`default_output_dir`] unless `--output` names another
//! directory. `flui clean` removes the default output directories; an
//! `--output` directory is the user's, like the rest of the file system,
//! and no clean touches it.

use std::io;
use std::path::{Path, PathBuf};

/// Where the project at `workspace_root` keeps what its builds package:
/// `<target-dir>/flui-out/<project>`, with the target-dir `cargo metadata`
/// reports and the project named after the package whose manifest is in
/// `workspace_root` (the directory's name for a virtual workspace), not
/// after an enclosing workspace, so members of one workspace stay apart.
/// Two projects of one name sharing a target-dir share this directory, as
/// their same-named binaries share `<target-dir>/<profile>`. Where cargo
/// cannot read the project, the target-dir is `<workspace_root>/target`,
/// cargo's own default.
#[must_use]
pub(crate) fn project_output_root(workspace_root: &Path) -> PathBuf {
    let (target_dir, project) = match crate::build::util::cargo::cargo_project(workspace_root) {
        Ok(cargo) => (cargo.target_dir, cargo.package),
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

/// What a clean removed, and the first removal that failed. The clean went
/// on past that failure.
#[derive(Debug, Default)]
pub(crate) struct Cleaned {
    /// The directories removed.
    pub(crate) removed: Vec<PathBuf>,
    /// The first removal that failed, naming the path.
    pub(crate) failure: Option<io::Error>,
}

impl Cleaned {
    /// Remove `dir` if it exists: record it on success, keep the first
    /// failure otherwise. A directory that cannot be inspected (anything but
    /// "not found") is a failure too, so an incomplete clean never reports
    /// success.
    pub(crate) fn remove(&mut self, dir: PathBuf, remove: RemoveDir) {
        match std::fs::symlink_metadata(&dir) {
            Ok(_) => match remove(&dir) {
                Ok(()) => self.removed.push(dir),
                Err(error) => self.fail(io::Error::new(
                    error.kind(),
                    format!("could not remove {}: {error}", dir.display()),
                )),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => self.fail(io::Error::new(
                error.kind(),
                format!("could not inspect {}: {error}", dir.display()),
            )),
        }
    }

    /// Keep `error` unless an earlier failure is already kept.
    pub(crate) fn fail(&mut self, error: io::Error) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    /// Add what another clean removed; the earlier failure stays first.
    pub(crate) fn merge(&mut self, other: Cleaned) {
        self.removed.extend(other.removed);
        if let Some(failure) = other.failure {
            self.fail(failure);
        }
    }
}

/// How a clean removes a directory: [`remove_dir_all`], or a stand-in
/// that fails on cue in the tests.
pub(crate) type RemoveDir = fn(&Path) -> io::Result<()>;

/// The [`RemoveDir`] every clean outside the tests uses.
pub(crate) fn remove_dir_all(dir: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members_of_one_workspace_keep_apart() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let workspace = tmp.path().join("workspace");
        let write = |path: &str, contents: &str| {
            let path = workspace.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(path, contents).expect("file");
        };
        write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"apps/*\"]\nresolver = \"3\"\n",
        );
        for app in ["first", "second"] {
            write(
                &format!("apps/{app}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{app}-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                ),
            );
            write(&format!("apps/{app}/src/lib.rs"), "");
        }
        let first = project_output_root(&workspace.join("apps/first"));
        let second = project_output_root(&workspace.join("apps/second"));
        assert_ne!(first, second, "two members share one output root");
        assert!(first.ends_with("flui-out/first-app"), "{}", first.display());
    }

    /// Two directories that cannot be removed and one that can: the clean
    /// removes what it can and keeps the first failure.
    fn a_failed_removal_does_not_stop_the_clean() {
        fn remove_unless_locked(dir: &Path) -> io::Result<()> {
            let locked = dir
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("locked-"));
            if locked {
                return Err(io::Error::other("locked"));
            }
            std::fs::remove_dir_all(dir)
        }
        let tmp = tempfile::tempdir().expect("temp dir");
        let dirs = ["locked-a", "locked-b", "open"].map(|name| tmp.path().join(name));
        for dir in &dirs {
            std::fs::create_dir_all(dir).expect("dir");
        }
        let mut cleaned = Cleaned::default();
        for dir in &dirs {
            cleaned.remove(dir.clone(), remove_unless_locked);
        }
        let failure = cleaned.failure.expect("the locked dirs failed");
        assert!(
            failure.to_string().contains("locked-a"),
            "the first failure is not the first attempted: {failure}"
        );
        assert_eq!(
            cleaned.removed,
            [dirs[2].clone()],
            "the clean stopped before the open dir"
        );
    }

    /// A directory whose parent cannot be searched: the clean reports it
    /// instead of skipping it as missing, and removes it once the parent
    /// opens. Unix only: a Windows ACL needs more than the standard library
    /// to set.
    #[cfg(unix)]
    fn an_unreachable_directory_is_reported() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().expect("temp dir");
        let parent = tmp.path().join("locked");
        let dir = parent.join("web");
        std::fs::create_dir_all(&dir).expect("output dir");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).expect("lock");
        let unreachable = std::fs::symlink_metadata(&dir).is_err();
        let mut first = Cleaned::default();
        first.remove(dir.clone(), remove_dir_all);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).expect("unlock");
        if !unreachable {
            // Running as root: permissions do not apply, nothing to observe.
            return;
        }
        assert!(
            first.failure.is_some(),
            "an unreachable directory was skipped as missing"
        );
        let mut retry = Cleaned::default();
        retry.remove(dir.clone(), remove_dir_all);
        assert!(
            retry.failure.is_none() && !dir.exists(),
            "the retry did not remove it"
        );
    }

    #[test]
    fn output_roots_and_cleaning() {
        #[cfg(unix)]
        crate::test_cases::run_cases(&[(
            "an_unreachable_directory_is_reported",
            an_unreachable_directory_is_reported,
        )]);
        crate::test_cases::run_cases(&[
            (
                "members_of_one_workspace_keep_apart",
                members_of_one_workspace_keep_apart,
            ),
            (
                "a_failed_removal_does_not_stop_the_clean",
                a_failed_removal_does_not_stop_the_clean,
            ),
        ]);
    }
}
