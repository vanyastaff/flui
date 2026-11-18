//! `cargo xtask android-dex`: the dex of each workspace crate that declares
//! Java classes (ADR-0377 §2), such as the Android backend's
//! (`flui-native`: `FluiActivity`, `FluiHost`, `FluiView`) and `flui-system`'s
//! (`FluiCallback`, `FluiDocuments`).
//!
//! A crate declares its Java sources and their compiled dex in
//! `[package.metadata.flui.declares.android]` (`java`, `dex`; ADR-0307), at
//! the top or under a feature, and the task reads that list from
//! `cargo metadata`: no list here is kept by hand. The dex is checked in, so
//! `flui build` packs the dex of each crate an application links into its
//! APK, and building FLUI for Android needs no JDK and no build script. A
//! record beside the dex (`classes.dex` → `classes.dex.toml`) holds the
//! SHA-256 of the sources and of the dex, and the tools that compiled it.
//! Each crate's classes compile against the platform alone, so no dex
//! depends on another's.
//!
//! - Without arguments (the `cargo xtask checks` step) both digests of every
//!   crate are checked, which needs no JDK: a source edited without
//!   regenerating the dex, or a dex replaced by hand, fails.
//! - `--rebuild` also compiles the sources with `javac` and `d8` and requires
//!   the checked-in bytes. It needs a JDK and the Android SDK's build tools
//!   (`ANDROID_HOME`); `javac` output can differ between JDK releases, so a
//!   mismatch names the recorded tools.
//! - `--write` compiles and writes each dex and its record, with the build
//!   tools and platform the record names when they are installed (the newest
//!   installed otherwise, which it says).
//! - `--list` prints the path of each declared dex and its record, which the
//!   workflow below uploads.
//!
//! Without a local JDK or SDK, `.github/workflows/android-dex.yml` runs
//! `--write` on a GitHub runner for a pull request that changes any
//! Java source or dex (and `--write` or `--rebuild` by hand on any branch)
//! and uploads each dex and its record as an artifact to commit.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::util::{ScratchDir, repo_root};

/// The oldest API level the dex runs on: FLUI's `minSdk`.
const MIN_API: u32 = 23;
/// The Java language level: what `d8` accepts for that API level without desugaring.
const JAVA_RELEASE: &str = "8";

/// One crate's Java classes, by the paths the task reads and writes, each
/// relative to the repository root and `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Tree {
    /// The directory of the Java sources.
    sources: String,
    /// The checked-in dex.
    dex: String,
    /// Its record: the dex's path with `.toml` added.
    record: String,
}

/// The trees one crate declares in its `[package.metadata]` (`metadata`):
/// the `java` and `dex` of `flui.declares.android` and of each
/// `flui.declares.features.<name>.android`, each pair resolved against the
/// crate's directory `crate_dir`, given relative to the repository root.
///
/// # Errors
///
/// A table that names one of `java` and `dex` without the other, or either
/// as anything but a string.
fn declared_trees(crate_dir: &str, metadata: &serde_json::Value) -> anyhow::Result<Vec<Tree>> {
    let Some(declares) = metadata.pointer("/flui/declares") else {
        return Ok(Vec::new());
    };
    let features = declares
        .get("features")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|features| features.values());
    let mut trees = Vec::new();
    for android in std::iter::once(declares)
        .chain(features)
        .filter_map(|table| table.get("android"))
    {
        let path = |key: &str| -> anyhow::Result<Option<String>> {
            android
                .get(key)
                .map(|value| {
                    value
                        .as_str()
                        .map(|path| format!("{crate_dir}/{}", path.trim_start_matches("./")))
                        .with_context(|| format!("{crate_dir}: android.{key} is not a path"))
                })
                .transpose()
        };
        match (path("java")?, path("dex")?) {
            (Some(sources), Some(dex)) => trees.push(Tree {
                record: format!("{dex}.toml"),
                sources,
                dex,
            }),
            (None, None) => {}
            _ => bail!("{crate_dir}: android.java and android.dex are declared together"),
        }
    }
    Ok(trees)
}

/// The trees every workspace crate declares, from `cargo metadata`, each
/// once and in path order.
fn workspace_trees(root: &Path) -> anyhow::Result<Vec<Tree>> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("running `cargo metadata`")?;
    let mut trees = Vec::new();
    for package in &metadata.packages {
        let crate_dir = package
            .manifest_path
            .parent()
            .context("a manifest has a directory")?;
        let relative = crate_dir
            .strip_prefix(&metadata.workspace_root)
            .unwrap_or(crate_dir)
            .as_str()
            .replace('\\', "/");
        trees.extend(declared_trees(&relative, &package.metadata)?);
    }
    trees.sort();
    trees.dedup();
    Ok(trees)
}

/// Arguments for `cargo xtask android-dex`.
#[derive(Debug, clap::Args)]
pub(crate) struct AndroidDexArgs {
    /// Compile the sources and write the dex and its record.
    #[arg(long, conflicts_with = "rebuild")]
    write: bool,
    /// Also compile the sources and require the checked-in bytes (needs a JDK and the SDK).
    #[arg(long)]
    rebuild: bool,
    /// Print the path of each declared dex and its record, one per line, and check nothing.
    #[arg(long, conflicts_with_all = ["write", "rebuild"])]
    list: bool,
}

/// What `classes.dex.toml` records.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    sources: String,
    dex: String,
    javac: String,
    build_tools: String,
    platform: String,
}

impl Record {
    fn render(&self) -> String {
        format!(
            "# Written by `cargo xtask android-dex --write`; checked by `cargo xtask checks`.\n\
             # The dex is compiled from the sources under `java/` (ADR-0247).\n\
             sources = \"{}\"\n\
             dex = \"{}\"\n\
             javac = \"{}\"\n\
             build-tools = \"{}\"\n\
             platform = \"{}\"\n\
             min-api = {MIN_API}\n",
            self.sources, self.dex, self.javac, self.build_tools, self.platform
        )
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let table: toml::Table = text.parse().context("parsing the dex record")?;
        let field = |name: &str| -> anyhow::Result<String> {
            table
                .get(name)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .with_context(|| format!("the dex record has no `{name}`"))
        };
        Ok(Self {
            sources: field("sources")?,
            dex: field("dex")?,
            javac: field("javac")?,
            build_tools: field("build-tools")?,
            platform: field("platform")?,
        })
    }
}

fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The `.java` files under `dir`, by path relative to it, `/`-separated and sorted.
fn java_files(dir: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        let path = entry.path();
        if entry.file_type().is_file() && path.extension().is_some_and(|ext| ext == "java") {
            let relative = path
                .strip_prefix(dir)
                .expect("BUG: walked paths sit under their root")
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path.to_path_buf()));
        }
    }
    files.sort();
    Ok(files)
}

/// The digest of the sources: each file's relative path and bytes, in order,
/// each followed by a NUL, so renaming a file changes it too.
fn sources_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

fn read_sources(root: &Path, tree: &Tree) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    java_files(&root.join(&tree.sources))?
        .into_iter()
        .map(|(relative, path)| {
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            Ok((relative, bytes))
        })
        .collect()
}

/// `cargo xtask android-dex`: check (or `--write`) each crate's dex.
pub(crate) fn android_dex(args: &AndroidDexArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let trees = workspace_trees(&root)?;
    if trees.is_empty() {
        bail!("no workspace crate declares `android.dex` and `android.java`");
    }
    if args.list {
        for tree in &trees {
            println!("{}\n{}", tree.dex, tree.record);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut held = true;
    for tree in &trees {
        held &= one_tree(&root, tree, args)?;
    }
    Ok(if held {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Check (or `--write`) the dex of `tree`; whether it holds.
fn one_tree(root: &Path, tree: &Tree, args: &AndroidDexArgs) -> anyhow::Result<bool> {
    let Tree {
        sources: sources_dir,
        dex: dex_path,
        record: record_path,
    } = tree;
    let sources = read_sources(root, tree)?;
    if sources.is_empty() {
        bail!("no Java sources under {sources_dir}");
    }
    let digest = sources_digest(&sources);
    if args.write {
        // The recorded build tools and platform when installed, so a rewrite
        // changes them only on purpose (by editing the record) and never picks
        // up a preview platform the SDK happens to have.
        let recorded = std::fs::read_to_string(root.join(record_path))
            .ok()
            .and_then(|text| Record::parse(&text).ok());
        let built = compile(root, tree, recorded.as_ref())?;
        let record = Record {
            sources: digest,
            dex: hex(&Sha256::digest(&built.dex)),
            javac: built.javac,
            build_tools: built.build_tools,
            platform: built.platform,
        };
        std::fs::write(root.join(dex_path), &built.dex)
            .with_context(|| format!("writing {dex_path}"))?;
        std::fs::write(root.join(record_path), record.render())
            .with_context(|| format!("writing {record_path}"))?;
        println!(
            "android-dex: wrote {dex_path} ({} bytes) and {record_path}",
            built.dex.len()
        );
        return Ok(true);
    }
    let record = Record::parse(
        &std::fs::read_to_string(root.join(record_path))
            .with_context(|| format!("reading {record_path}"))?,
    )?;
    let dex = std::fs::read(root.join(dex_path)).with_context(|| format!("reading {dex_path}"))?;
    let mut failures: Vec<String> = Vec::new();
    if record.sources != digest {
        failures.push(format!(
            "the Java sources under {sources_dir} changed since the dex was built; \
             run `cargo xtask android-dex --write`, or the `Android dex` workflow on a runner"
        ));
    }
    if record.dex != hex(&Sha256::digest(&dex)) {
        failures.push(format!(
            "{dex_path} is not the dex {record_path} records; run `cargo xtask android-dex --write`"
        ));
    }
    if failures.is_empty() && args.rebuild {
        let built = compile(root, tree, Some(&record))?;
        if built.dex != dex {
            failures.push(format!(
                "compiling the sources with javac {} and build-tools {} does not reproduce \
                 {dex_path} (recorded: javac {}, build-tools {})",
                built.javac, built.build_tools, record.javac, record.build_tools
            ));
        }
    }
    for failure in &failures {
        eprintln!("android-dex: {failure}");
    }
    if !failures.is_empty() {
        return Ok(false);
    }
    println!(
        "android-dex: {dex_path} is built from the sources under {sources_dir}{}",
        if args.rebuild {
            " and reproduces byte for byte"
        } else {
            " (digests)"
        }
    );
    Ok(true)
}

/// A dex compiled here, and the tools that compiled it.
struct Built {
    dex: Vec<u8>,
    javac: String,
    build_tools: String,
    platform: String,
}

/// The SDK at `ANDROID_HOME` (or `ANDROID_SDK_ROOT`).
fn sdk_root() -> anyhow::Result<PathBuf> {
    std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .context("compiling the dex needs the Android SDK: set ANDROID_HOME")
}

/// The subdirectory of `parent` named `wanted` when it has `file`, else the
/// one with the highest version-like name that has it.
fn pick(parent: &Path, wanted: Option<&str>, file: &str) -> anyhow::Result<(String, PathBuf)> {
    if let Some(wanted) = wanted {
        let dir = parent.join(wanted);
        if dir.join(file).is_file() {
            return Ok((wanted.to_owned(), dir));
        }
        eprintln!(
            "android-dex: {wanted} is not installed under {}; using the newest",
            parent.display()
        );
    }
    let mut found: Vec<(Vec<u32>, String, PathBuf)> = std::fs::read_dir(parent)
        .with_context(|| format!("reading {}", parent.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(file).is_file())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let version = name
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().unwrap_or(0))
                .collect();
            Some((version, name, path))
        })
        .collect();
    found.sort();
    found
        .pop()
        .map(|(_, name, path)| (name, path))
        .with_context(|| format!("no directory under {} has {file}", parent.display()))
}

/// `name` from `JAVA_HOME`'s `bin`, else from `PATH`.
fn jdk_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(name))
        .filter(|path| path.with_extension("exe").is_file() || path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn run(command: &mut Command) -> anyhow::Result<String> {
    let output = command
        .output()
        .with_context(|| format!("running {command:?} (is a JDK installed?)"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Compile `tree`'s sources: `javac` against the platform's `android.jar`,
/// then `d8` from the build tools, run through `java` so every host runs the
/// same jar. `recorded` picks the recorded build tools and platform when
/// installed.
fn compile(root: &Path, tree: &Tree, recorded: Option<&Record>) -> anyhow::Result<Built> {
    let sdk = sdk_root()?;
    let (build_tools, tools_dir) = pick(
        &sdk.join("build-tools"),
        recorded.map(|record| record.build_tools.as_str()),
        "lib/d8.jar",
    )?;
    let (platform, platform_dir) = pick(
        &sdk.join("platforms"),
        recorded.map(|record| record.platform.as_str()),
        "android.jar",
    )?;
    let android_jar = platform_dir.join("android.jar");
    let javac_version = run(Command::new(jdk_tool("javac")).arg("-version"))?;
    let javac = javac_version
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .to_owned();
    let scratch = ScratchDir::try_new("android-dex")?;
    let classes = scratch.path().join("classes");
    let out = scratch.path().join("out");
    std::fs::create_dir_all(&classes)?;
    std::fs::create_dir_all(&out)?;
    let sources: Vec<PathBuf> = java_files(&root.join(&tree.sources))?
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    run(Command::new(jdk_tool("javac"))
        .args([
            "--release",
            JAVA_RELEASE,
            "-encoding",
            "UTF-8",
            "-Xlint:all,-options",
        ])
        .arg("-Werror")
        .arg("-classpath")
        .arg(&android_jar)
        .arg("-d")
        .arg(&classes)
        .args(&sources))?;
    let mut class_files: Vec<PathBuf> = walkdir::WalkDir::new(&classes)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
        .collect();
    class_files.sort();
    run(Command::new(jdk_tool("java"))
        .arg("-cp")
        .arg(tools_dir.join("lib/d8.jar"))
        .arg("com.android.tools.r8.D8")
        .arg("--release")
        .args(["--min-api", &MIN_API.to_string()])
        .arg("--lib")
        .arg(&android_jar)
        .arg("--output")
        .arg(&out)
        .args(&class_files))?;
    let dex = std::fs::read(out.join("classes.dex")).context("d8 wrote no classes.dex")?;
    Ok(Built {
        dex,
        javac,
        build_tools,
        platform,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_record_round_trips() {
        let record = Record {
            sources: "aa".into(),
            dex: "bb".into(),
            javac: "21.0.12".into(),
            build_tools: "35.0.0".into(),
            platform: "android-35".into(),
        };
        assert_eq!(Record::parse(&record.render()).expect("parses"), record);
    }

    fn the_source_digest_covers_paths_and_bytes() {
        let one = vec![("a/A.java".to_owned(), b"class A {}".to_vec())];
        let renamed = vec![("a/B.java".to_owned(), b"class A {}".to_vec())];
        let edited = vec![("a/A.java".to_owned(), b"class A { }".to_vec())];
        let split = vec![
            ("a/A.java".to_owned(), b"class".to_vec()),
            ("a/A.java".to_owned(), b" A {}".to_vec()),
        ];
        let digest = sources_digest(&one);
        assert_ne!(digest, sources_digest(&renamed));
        assert_ne!(digest, sources_digest(&edited));
        assert_ne!(digest, sources_digest(&split));
    }

    fn the_checked_in_dex_matches_its_sources() {
        let args = AndroidDexArgs {
            write: false,
            rebuild: false,
            list: false,
        };
        assert_eq!(android_dex(&args).expect("runs"), ExitCode::SUCCESS);
    }

    /// The trees come from what each crate declares, at the top and under
    /// its features; a crate that declares neither key has none, and one
    /// that declares one key of the pair is refused.
    fn the_trees_are_what_the_crates_declare() {
        let declared = serde_json::json!({
            "flui": { "declares": {
                "schema": 4,
                "android": { "dex": "android/classes.dex", "java": "android/java" },
                "features": {
                    "camera": { "android": { "dex": "./camera/classes.dex", "java": "camera/java" } },
                    "http": { "android": { "permissions": ["android.permission.INTERNET"] } },
                },
            } }
        });
        let mut trees = declared_trees("crates/flui-x", &declared).expect("declared");
        trees.sort();
        assert_eq!(
            trees,
            [
                Tree {
                    sources: "crates/flui-x/android/java".to_owned(),
                    dex: "crates/flui-x/android/classes.dex".to_owned(),
                    record: "crates/flui-x/android/classes.dex.toml".to_owned(),
                },
                Tree {
                    sources: "crates/flui-x/camera/java".to_owned(),
                    dex: "crates/flui-x/camera/classes.dex".to_owned(),
                    record: "crates/flui-x/camera/classes.dex.toml".to_owned(),
                },
            ]
        );
        let none = serde_json::json!({ "flui": { "tier": "pkg" } });
        assert_eq!(
            declared_trees("crates/flui-y", &none).expect("none"),
            Vec::<Tree>::new()
        );
        let half = serde_json::json!({ "flui": { "declares": { "android": { "dex": "a.dex" } } } });
        assert!(declared_trees("crates/flui-z", &half).is_err());

        let workspace = workspace_trees(&repo_root()).expect("cargo metadata");
        for declaring in ["crates/flui-native/", "crates/flui-system/"] {
            assert!(
                workspace.iter().any(|tree| tree.dex.starts_with(declaring)),
                "{declaring} declares its dex: {workspace:?}"
            );
        }
    }

    #[test]
    fn android_dex_contract() {
        crate::table_test::run_table(
            "android_dex_contract",
            &[
                ("the_record_round_trips", the_record_round_trips as fn()),
                (
                    "the_source_digest_covers_paths_and_bytes",
                    the_source_digest_covers_paths_and_bytes as fn(),
                ),
                (
                    "the_checked_in_dex_matches_its_sources",
                    the_checked_in_dex_matches_its_sources as fn(),
                ),
                (
                    "the_trees_are_what_the_crates_declare",
                    the_trees_are_what_the_crates_declare as fn(),
                ),
            ],
        );
    }
}
