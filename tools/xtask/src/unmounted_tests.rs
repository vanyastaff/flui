//! Every `tests/*.rs` file of a crate that sets `autotests = false` is compiled.
//!
//! Such a crate names its test targets itself, and its other test files are
//! modules of one. A new `tests/*.rs` that no `[[test]]` declares and no
//! target mounts is never compiled: its tests are green because they do not
//! run. No test run shows that, so it is checked here.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use toml::{Table, Value as Toml};

use crate::util::{metadata, repo_root};

/// Arguments for `cargo xtask unmounted-tests`.
#[derive(Debug, clap::Args)]
pub(crate) struct UnmountedTestsArgs {}

/// `cargo xtask unmounted-tests`: check that every `tests/*.rs` file of an
/// `autotests = false` crate is a test target or a module of one.
pub(crate) fn unmounted_tests(_args: &UnmountedTestsArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let workspace = metadata(&root)?;
    let mut findings: Vec<String> = Vec::new();
    let mut crates = 0_usize;
    for package in workspace.workspace_packages() {
        let manifest_path = package.manifest_path.as_std_path();
        let text = std::fs::read_to_string(manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: Table = toml::from_str(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        let autotests = manifest
            .get("package")
            .and_then(Toml::as_table)
            .and_then(|package| package.get("autotests"));
        if autotests != Some(&Toml::Boolean(false)) {
            continue;
        }
        crates += 1;
        let crate_dir = manifest_path
            .parent()
            .expect("BUG: a manifest path has a parent directory");
        let declared: BTreeSet<PathBuf> = package
            .targets
            .iter()
            .filter(|target| target.is_test())
            .map(|target| normalize(target.src_path.as_std_path()))
            .collect();
        for file in unmounted(crate_dir, &declared)? {
            findings.push(format!(
                "{} never runs: `{}` sets `autotests = false`; give it a `[[test]]` target or \
                 mount it from one",
                file.strip_prefix(&root).unwrap_or(&file).display(),
                package.name
            ));
        }
    }
    if findings.is_empty() {
        println!(
            "unmounted-tests: {crates} crate(s) with `autotests = false`, every test file compiled"
        );
        return Ok(ExitCode::SUCCESS);
    }
    for finding in &findings {
        eprintln!("unmounted-tests: {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// The top-level `tests/*.rs` files of `crate_dir` that are neither one of the
/// `declared` test targets nor mounted (`mod` or `#[path] mod`) from one.
///
/// A `tests/main.rs` no target declares is unmounted itself: Cargo never
/// compiles it, so its `mod` lines mount nothing. Only the direct modules of a
/// declared target count, read from its syntax, so a comment or a string
/// literal mounts no file.
fn unmounted(crate_dir: &Path, declared: &BTreeSet<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let tests_dir = crate_dir.join("tests");
    if !tests_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut mounted: BTreeSet<PathBuf> = BTreeSet::new();
    for source in declared.iter().filter(|path| path.is_file()) {
        let text = std::fs::read_to_string(source)
            .with_context(|| format!("reading {}", source.display()))?;
        let dir = source
            .parent()
            .expect("BUG: a file path has a parent directory");
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parsing test target {}", source.display()))?;
        for item in parsed.items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            if module.content.is_some() {
                continue;
            }
            let path: Option<String> = module
                .attrs
                .iter()
                .find(|attribute| attribute.path().is_ident("path"))
                .map(|attribute| {
                    let syn::Meta::NameValue(value) = &attribute.meta else {
                        bail!("module path in {} is not a string", source.display());
                    };
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(path),
                        ..
                    }) = &value.value
                    else {
                        bail!("module path in {} is not a string", source.display());
                    };
                    Ok(path.value())
                })
                .transpose()?;
            let file = path.map_or_else(
                || dir.join(format!("{}.rs", module.ident)),
                |path| dir.join(path),
            );
            mounted.insert(normalize(&file));
        }
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&tests_dir)
        .with_context(|| format!("listing {}", tests_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| normalize(&entry.path()))
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    Ok(files
        .into_iter()
        .filter(|file| !declared.contains(file) && !mounted.contains(file))
        .collect())
}

/// `path` with `.` and `..` resolved lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ScratchDir;

    /// A crate directory whose `tests/` holds `files`, and the names left unmounted when
    /// `targets` are its declared test targets.
    fn left_unmounted(files: &[(&str, &str)], targets: &[&str]) -> Vec<String> {
        let scratch = ScratchDir::try_new("unmounted-tests").expect("a scratch directory");
        let tests = scratch.path().join("tests");
        std::fs::create_dir_all(&tests).expect("mkdir");
        for (name, text) in files {
            std::fs::write(tests.join(name), text).expect("write");
        }
        let declared: BTreeSet<PathBuf> = targets
            .iter()
            .map(|name| normalize(&tests.join(name)))
            .collect();
        unmounted(scratch.path(), &declared)
            .expect("the walk")
            .iter()
            .map(|path| {
                path.file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn a_file_no_target_mounts_is_reported() {
        let files = [
            (
                "main.rs",
                "mod mounted;\n#[path = \"elsewhere.rs\"]\nmod renamed;\n",
            ),
            ("mounted.rs", ""),
            ("elsewhere.rs", ""),
            ("orphan.rs", ""),
        ];
        assert_eq!(left_unmounted(&files, &["main.rs"]), ["orphan.rs"]);
    }

    fn a_target_of_its_own_is_compiled() {
        let files = [("main.rs", ""), ("standalone.rs", "")];
        assert_eq!(
            left_unmounted(&files, &["main.rs", "standalone.rs"]),
            [] as [&str; 0]
        );
    }

    fn an_undeclared_main_mounts_nothing() {
        let files = [("main.rs", "mod mounted;\n"), ("mounted.rs", "")];
        assert_eq!(left_unmounted(&files, &[]), ["main.rs", "mounted.rs"]);
    }

    fn a_comment_or_a_string_mounts_nothing() {
        let files = [
            (
                "main.rs",
                "// mod commented;\n/* mod blocked; */\nconst S: &str = \"mod quoted;\";\n",
            ),
            ("commented.rs", ""),
            ("blocked.rs", ""),
            ("quoted.rs", ""),
        ];
        assert_eq!(
            left_unmounted(&files, &["main.rs"]),
            ["blocked.rs", "commented.rs", "quoted.rs"]
        );
    }

    #[test]
    fn unmounted_tests_contract() {
        crate::table_test::run_table(
            "unmounted_tests_contract",
            &[
                (
                    "a_file_no_target_mounts_is_reported",
                    a_file_no_target_mounts_is_reported as fn(),
                ),
                (
                    "a_target_of_its_own_is_compiled",
                    a_target_of_its_own_is_compiled as fn(),
                ),
                (
                    "an_undeclared_main_mounts_nothing",
                    an_undeclared_main_mounts_nothing as fn(),
                ),
                (
                    "a_comment_or_a_string_mounts_nothing",
                    a_comment_or_a_string_mounts_nothing as fn(),
                ),
            ],
        );
    }
}
