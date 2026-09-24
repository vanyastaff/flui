//! The guard that keeps the lane's docs-only allowlist honest against real
//! `include_str!` targets.

use std::path::{Path, PathBuf};

use anyhow::Context;
use regex::Regex;
use walkdir::WalkDir;

use super::classify::is_docs_only;

/// A `.rs` file and the docs-only path it `include_str!`s.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Offender {
    pub(super) rs_file: String,
    pub(super) target: String,
}

/// `path` relative to `root` with `/` separators.
fn slash_relative(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Every `include_str!("...")` in the tree's `.rs` files whose target exists,
/// lies inside the repository and is classified docs-only.
///
/// The docs-only allowlist decides which changed files let a `pull_request`
/// run skip every compiling job. A file under it that is pulled into compiled
/// or tested content via `include_str!` (a `#[doc = include_str!(...)]`
/// doctest, or a plain `include_str!(...)` a test asserts against) would let a
/// PR touching only that file skip the very jobs that would catch it breaking
/// something.
pub(super) fn docs_only_include_targets(root: &Path) -> anyhow::Result<Vec<Offender>> {
    let include = Regex::new(r#"include_str!\(\s*"([^"]+)"\s*\)"#).expect("BUG: valid regex");
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let mut offenders = Vec::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        // build output, the optional reference clones, git's own store; a
        // top-level `target-*` is a per-agent CARGO_TARGET_DIR, never source
        let top_level_target = e.depth() == 1 && name.starts_with("target-");
        !(e.file_type().is_dir()
            && (matches!(&*name, "target" | ".flutter" | ".gpui" | ".git") || top_level_target))
    });
    for entry in walker {
        let entry = entry.context("walking the repository")?;
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        for m in include.captures_iter(&text) {
            let Some(dir) = path.parent() else { continue };
            // a missing target (e.g. a build-script-generated path) or one
            // outside the repository is not this guard's concern
            let Ok(target): Result<PathBuf, _> = dir.join(&m[1]).canonicalize() else {
                continue;
            };
            let Some(target) = slash_relative(&target, &canonical_root) else {
                continue;
            };
            if is_docs_only(&target) {
                let rs_file =
                    slash_relative(path, root).context("a walked file lies under the root")?;
                offenders.push(Offender { rs_file, target });
            }
        }
    }
    offenders.sort();
    offenders.dedup();
    Ok(offenders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_tree_includes_no_docs_only_file() {
        let offenders = docs_only_include_targets(&crate::util::repo_root()).expect("walk");
        assert_eq!(offenders, Vec::<Offender>::new());
    }

    #[test]
    fn a_docs_only_include_target_is_reported() {
        let dir = std::env::temp_dir().join(format!("xtask-paths-filter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["docs", "crates/a/src", "crates/a/target"] {
            std::fs::create_dir_all(dir.join(sub)).expect("mkdir");
        }
        std::fs::write(dir.join("docs/guide.md"), "# guide\n").expect("write");
        std::fs::write(dir.join("crates/a/README.md"), "# a\n").expect("write");
        let src = "#![doc = include_str!(\"../README.md\")]\n\
                   const GUIDE: &str = include_str!( \"../../../docs/guide.md\" );\n\
                   const GONE: &str = include_str!(\"../../../docs/missing.md\");\n";
        std::fs::write(dir.join("crates/a/src/lib.rs"), src).expect("write");
        // build output is not source
        std::fs::write(
            dir.join("crates/a/target/gen.rs"),
            "include_str!(\"../../../docs/guide.md\");",
        )
        .expect("write");
        let offenders = docs_only_include_targets(&dir).expect("walk");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            offenders,
            [Offender {
                rs_file: "crates/a/src/lib.rs".to_owned(),
                target: "docs/guide.md".to_owned()
            }]
        );
    }
}
