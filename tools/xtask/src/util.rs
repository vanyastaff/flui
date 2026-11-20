//! Helpers shared by the commands: the repository root, files relative to it,
//! and the workspace metadata.

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

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
pub(crate) fn same_dir(a: &Path, b: &Path) -> bool {
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

/// An exclusively created temporary directory, removed on drop.
///
/// `tempfile` owns name generation, atomic admission and cleanup, so an
/// existing path is never adopted and subsequently removed as our scratch.
pub(crate) struct ScratchDir(tempfile::TempDir);

impl ScratchDir {
    pub(crate) fn try_new(label: &str) -> anyhow::Result<Self> {
        tempfile::Builder::new()
            .prefix(&format!("xtask-{label}-"))
            .tempdir()
            .map(Self)
            .context("creating an exclusive xtask scratch directory")
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_ran_the_binary_it_built() {
        // `cargo test` sets CARGO_MANIFEST_DIR to this crate, as `cargo run` does
        built_from_this_checkout().expect("the test binary is this checkout's");
    }
}
