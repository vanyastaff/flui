//! Helpers shared by the commands: the repository root, files relative to it,
//! and the workspace metadata.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, ensure};
use regex::Regex;

/// The xtask manifest directory this binary was built from.
const BUILT_FROM: &str = env!("CARGO_MANIFEST_DIR");

/// The repository root: the directory holding the workspace `Cargo.toml`.
///
/// Two levels above the xtask manifest, whichever directory `cargo xtask` was
/// started from; [`built_from_this_checkout`] makes sure that manifest is the
/// one cargo ran.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(BUILT_FROM)
        .ancestors()
        .nth(2)
        .expect("BUG: tools/xtask sits two levels below the repository root")
        .to_path_buf()
}

/// Refuses to run a binary another checkout built.
///
/// `cargo run`, and so `cargo xtask`, tells the program which manifest it ran
/// in `CARGO_MANIFEST_DIR`. A target directory shared between worktrees can
/// hand this checkout the xtask another checkout built: cargo judges freshness
/// by modification times, and on Windows the executable's name carries no
/// per-checkout hash. That binary would run the other checkout's code against
/// the other checkout's tree. Started directly, not through cargo, there is
/// nothing to compare.
pub(crate) fn built_from_this_checkout() -> anyhow::Result<()> {
    let Some(ran) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return Ok(());
    };
    let ran = Path::new(&ran);
    ensure!(
        same_dir(ran, Path::new(BUILT_FROM)),
        "this xtask was built from {BUILT_FROM}, but cargo ran it for {}: a target directory \
         shared between checkouts served another checkout's build. Run `cargo clean -p xtask` \
         and retry, or give each checkout its own CARGO_TARGET_DIR",
        ran.display()
    );
    Ok(())
}

/// Whether `a` and `b` name the same directory, however each is spelled.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Reads a file relative to the repository root, with the path in the error.
pub(crate) fn read(rel: impl AsRef<Path>) -> anyhow::Result<String> {
    let path = repo_root().join(rel.as_ref());
    std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

/// `cargo metadata --no-deps` for the workspace at `root`: only the workspace's
/// own manifests are read, so it is fast and needs no network.
pub(crate) fn metadata(root: &Path) -> anyhow::Result<cargo_metadata::Metadata> {
    cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .exec()
        .context("running `cargo metadata`")
}

/// Whether `exit` is spelled `ADR-NNNN`, as an allowlist entry's exit is.
pub(crate) fn is_adr_number(exit: &str) -> bool {
    static ADR_NUMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ADR-\d{4}$").expect("BUG: static regex is valid"));
    ADR_NUMBER.is_match(exit)
}

/// The file names under `docs/adr` of the repository at `root`; none when the
/// directory is missing.
pub(crate) fn adr_files(root: &Path) -> Vec<String> {
    std::fs::read_dir(root.join("docs").join("adr"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// Whether one of `files` (from [`adr_files`]) is the ADR `number`.
pub(crate) fn adr_exists(files: &[String], number: &str) -> bool {
    let prefix = format!("{number}-");
    let exact = format!("{number}.md");
    files
        .iter()
        .any(|file| file.starts_with(&prefix) || *file == exact)
}

/// A uniquely named directory under the system temp dir, removed on drop.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(crate) fn new(label: &str) -> anyhow::Result<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "xtask-{label}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best effort: a leftover scratch directory is harmless.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_is_itself_however_it_is_spelled() {
        let dir = ScratchDir::new("same-dir").expect("scratch dir");
        std::fs::create_dir_all(dir.path().join("a")).expect("subdirectory");
        assert!(same_dir(dir.path(), &dir.path().join("a").join("..")));
        assert!(!same_dir(dir.path(), &dir.path().join("a")));
    }

    #[test]
    fn cargo_ran_the_binary_it_built() {
        // `cargo test` sets CARGO_MANIFEST_DIR to this crate, as `cargo run` does
        built_from_this_checkout().expect("the test binary is this checkout's");
    }
}
