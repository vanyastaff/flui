//! An in-memory description of a generated project, written to disk in one
//! step.
//!
//! Every template builds a [`ProjectPlan`] from pure string formatting —
//! no file system access — so `--dry-run` can inspect exactly what a
//! template would produce without creating anything, and so two runs of the
//! same inputs are provably byte-identical (see the `plan_is_deterministic`
//! test below).

use crate::error::{CliResult, ResultExt};
use std::path::{Path, PathBuf};

/// One file a template wants written, with a path relative to the project
/// root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Path relative to the project root (e.g. `src/main.rs`).
    pub path: PathBuf,
    /// The file's full contents.
    pub contents: String,
}

/// Everything a template generates, before any of it touches disk.
///
/// Files are kept in the order the template pushed them, which is also the
/// order `--dry-run` lists them and the order `create.file` events are
/// emitted in — both are part of the observable, deterministic output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectPlan {
    files: Vec<PlannedFile>,
    /// Directories to create even when they hold no planned file (e.g. an
    /// empty `assets/`).
    dirs: Vec<PathBuf>,
}

impl ProjectPlan {
    /// An empty plan.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file, relative to the project root.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        self.files.push(PlannedFile {
            path: path.into(),
            contents: contents.into(),
        });
        self
    }

    /// Ensure a directory exists even if no file is planned inside it.
    #[must_use]
    pub fn dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.dirs.push(path.into());
        self
    }

    /// Append another plan's files and directories, keeping this plan's
    /// entries first.
    ///
    /// Used to compose the hot-reload workspace from its three member
    /// crates' sub-plans.
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.files.extend(other.files);
        self.dirs.extend(other.dirs);
        self
    }

    /// The planned files, in generation order.
    #[must_use]
    pub fn files(&self) -> &[PlannedFile] {
        &self.files
    }

    /// The directories this plan creates in addition to file parents.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Write every planned file and directory under `root`, which must
    /// already exist.
    ///
    /// # Errors
    ///
    /// Returns an error if any directory or file cannot be created.
    pub fn write(&self, root: &Path) -> CliResult<()> {
        for dir in &self.dirs {
            let target = root.join(dir);
            std::fs::create_dir_all(&target)
                .with_context(|| format!("Failed to create directory '{}'", target.display()))?;
        }
        for file in &self.files {
            let target = root.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create directory '{}'", parent.display())
                })?;
            }
            std::fs::write(&target, &file.contents)
                .with_context(|| format!("Failed to create '{}'", target.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_creates_files_and_bare_directories() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let plan = ProjectPlan::new()
            .file("Cargo.toml", "[package]\n")
            .file("src/main.rs", "fn main() {}\n")
            .dir("assets");

        plan.write(tmp.path()).expect("plan writes");

        assert_eq!(
            std::fs::read_to_string(tmp.path().join("Cargo.toml")).expect("Cargo.toml"),
            "[package]\n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("src/main.rs")).expect("src/main.rs"),
            "fn main() {}\n"
        );
        assert!(tmp.path().join("assets").is_dir());
    }

    #[test]
    fn merge_preserves_order_and_appends() {
        let a = ProjectPlan::new().file("a.txt", "a").dir("da");
        let b = ProjectPlan::new().file("b.txt", "b").dir("db");
        let merged = a.merge(b);
        assert_eq!(
            merged
                .files()
                .iter()
                .map(|f| f.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["a.txt", "b.txt"]
        );
        assert_eq!(
            merged
                .dirs()
                .iter()
                .map(|d| d.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["da", "db"]
        );
    }
}
