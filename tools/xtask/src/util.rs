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

/// `cargo metadata --no-deps` for the workspace at `root`: only the workspace's
/// own manifests are read, so it is fast and needs no network.
pub(crate) fn metadata(root: &Path) -> anyhow::Result<cargo_metadata::Metadata> {
    cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .exec()
        .context("running `cargo metadata`")
}

/// `cargo metadata --locked --all-features` for the workspace at `root`, with
/// no platform filter: every edge any root build can activate, which
/// `cargo xtask reach` resolves per root. Needs the registry sources of every
/// package in the lock file.
pub(crate) fn resolved_metadata(root: &Path) -> anyhow::Result<cargo_metadata::Metadata> {
    cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .features(cargo_metadata::CargoOpt::AllFeatures)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("running `cargo metadata --locked --all-features`")
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

/// `bytes` the way `du -sh` prints a size: one decimal below 10 of a unit.
pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else if value < 10.0 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

/// The total size of the files under `dir`.
pub(crate) fn tree_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len())
        .sum()
}

/// An exclusively created temporary directory, removed on drop.
///
/// `tempfile` owns name generation, atomic admission and cleanup, so an
/// existing path is never adopted and subsequently removed as our scratch.
pub(crate) struct ScratchDir(tempfile::TempDir);

impl ScratchDir {
    pub(crate) fn new(label: &str) -> anyhow::Result<Self> {
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
