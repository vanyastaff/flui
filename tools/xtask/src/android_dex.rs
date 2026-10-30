//! `cargo xtask android-dex`: the dex of the Android backend's `FluiView`
//! (ADR-0247).
//!
//! The view's Java sources live under `crates/flui-native/android/java`.
//! Their compiled dex, `crates/flui-native/android/classes.dex`, is checked
//! in and `include_bytes!`'d into the backend, so building FLUI for Android
//! needs no JDK and no build script, and every packager (`flui build`, Gradle,
//! cargo-apk) gets the dex the library was built with. `classes.dex.toml`
//! next to it records the SHA-256 of the sources and of the dex, and the tools
//! that compiled it.
//!
//! - Without arguments (the `cargo xtask checks` step) both digests are
//!   checked, which needs no JDK: a source edited without regenerating the
//!   dex, or a dex replaced by hand, fails. So does a backend whose one
//!   `include_bytes!` (in `src/owner/android/java_view.rs`) no longer names
//!   the checked-in dex.
//! - `--rebuild` also compiles the sources with `javac` and `d8` and requires
//!   the checked-in bytes. It needs a JDK and the Android SDK's build tools
//!   (`ANDROID_HOME`); `javac` output can differ between JDK releases, so a
//!   mismatch names the recorded tools.
//! - `--write` compiles and writes the dex and its record.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::util::{ScratchDir, repo_root};

/// The Java sources, relative to the repository root.
const SOURCES: &str = "crates/flui-native/android/java";
/// The checked-in dex.
const DEX: &str = "crates/flui-native/android/classes.dex";
/// Its record.
const RECORD: &str = "crates/flui-native/android/classes.dex.toml";
/// The backend file that `include_bytes!`s the dex: the Android owner thread's
/// Java view.
const EMBEDDER: &str = "crates/flui-native/src/owner/android/java_view.rs";
/// The oldest API level the dex runs on: FLUI's `minSdk`.
const MIN_API: u32 = 23;
/// The Java language level: what `d8` accepts for that API level without desugaring.
const JAVA_RELEASE: &str = "8";

/// Arguments for `cargo xtask android-dex`.
#[derive(Debug, clap::Args)]
pub(crate) struct AndroidDexArgs {
    /// Compile the sources and write the dex and its record.
    #[arg(long, conflicts_with = "rebuild")]
    write: bool,
    /// Also compile the sources and require the checked-in bytes (needs a JDK and the SDK).
    #[arg(long)]
    rebuild: bool,
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

/// The repository-relative path the one `include_bytes!` in `source`, the text
/// of the repository-relative `file`, names; `None` when `source` has none or
/// more than one, or its path climbs out of the repository.
fn embedded_path(file: &str, source: &str) -> Option<String> {
    const OPEN: &str = "include_bytes!(\"";
    let mut includes = source.match_indices(OPEN);
    let (start, _) = includes.next()?;
    if includes.next().is_some() {
        return None;
    }
    let rest = &source[start + OPEN.len()..];
    let literal = &rest[..rest.find('"')?];
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in literal.split('/') {
        match part {
            ".." => {
                parts.pop()?;
            }
            "" | "." => {}
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Why `source`, the text of the backend file `embedder`, does not ship the
/// checked-in dex: its one `include_bytes!` names another file, or it has
/// none or more than one. `None` when it embeds [`DEX`].
fn embedding_failure(embedder: &str, source: &str) -> Option<String> {
    let embedded = embedded_path(embedder, source);
    (embedded.as_deref() != Some(DEX)).then(|| match embedded {
        Some(other) => format!(
            "{embedder} embeds {other}, not {DEX}; the backend would ship another dex than the \
             one checked here"
        ),
        None => format!("{embedder} does not embed {DEX} with one `include_bytes!`"),
    })
}

fn read_sources(root: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    java_files(&root.join(SOURCES))?
        .into_iter()
        .map(|(relative, path)| {
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            Ok((relative, bytes))
        })
        .collect()
}

/// `cargo xtask android-dex`: check (or `--write`) the `FluiView` dex.
pub(crate) fn android_dex(args: &AndroidDexArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let sources = read_sources(&root)?;
    if sources.is_empty() {
        bail!("no Java sources under {SOURCES}");
    }
    let digest = sources_digest(&sources);
    if args.write {
        let built = compile(&root, None)?;
        let record = Record {
            sources: digest,
            dex: hex(&Sha256::digest(&built.dex)),
            javac: built.javac,
            build_tools: built.build_tools,
            platform: built.platform,
        };
        std::fs::write(root.join(DEX), &built.dex).with_context(|| format!("writing {DEX}"))?;
        std::fs::write(root.join(RECORD), record.render())
            .with_context(|| format!("writing {RECORD}"))?;
        println!(
            "android-dex: wrote {DEX} ({} bytes) and {RECORD}",
            built.dex.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let record = Record::parse(
        &std::fs::read_to_string(root.join(RECORD)).with_context(|| format!("reading {RECORD}"))?,
    )?;
    let dex = std::fs::read(root.join(DEX)).with_context(|| format!("reading {DEX}"))?;
    let embedder = std::fs::read_to_string(root.join(EMBEDDER))
        .with_context(|| format!("reading {EMBEDDER}"))?;
    let mut failures: Vec<String> = embedding_failure(EMBEDDER, &embedder).into_iter().collect();
    if record.sources != digest {
        failures.push(format!(
            "the Java sources under {SOURCES} changed since the dex was built; \
             run `cargo xtask android-dex --write`"
        ));
    }
    if record.dex != hex(&Sha256::digest(&dex)) {
        failures.push(format!(
            "{DEX} is not the dex {RECORD} records; run `cargo xtask android-dex --write`"
        ));
    }
    if failures.is_empty() && args.rebuild {
        let built = compile(&root, Some(&record))?;
        if built.dex != dex {
            failures.push(format!(
                "compiling the sources with javac {} and build-tools {} does not reproduce {DEX} \
                 (recorded: javac {}, build-tools {})",
                built.javac, built.build_tools, record.javac, record.build_tools
            ));
        }
    }
    for failure in &failures {
        eprintln!("android-dex: {failure}");
    }
    if !failures.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "android-dex: {DEX} is built from the sources under {SOURCES}{}",
        if args.rebuild {
            " and reproduces byte for byte"
        } else {
            " (digests)"
        }
    );
    Ok(ExitCode::SUCCESS)
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

/// Compile the sources: `javac` against the platform's `android.jar`, then
/// `d8` from the build tools, run through `java` so every host runs the same
/// jar. `recorded` picks the recorded build tools and platform when installed.
fn compile(root: &Path, recorded: Option<&Record>) -> anyhow::Result<Built> {
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
    let sources: Vec<PathBuf> = java_files(&root.join(SOURCES))?
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

    fn the_embedded_path_resolves_against_the_embedding_file() {
        let include = r#"const DEX: &[u8] = include_bytes!("../../../android/classes.dex");"#;
        assert_eq!(
            embedded_path(EMBEDDER, include).as_deref(),
            Some(DEX),
            "the path, relative to the embedding file"
        );
        assert_eq!(
            embedded_path("crates/flui-native/src/owner/android.rs", include).as_deref(),
            Some("crates/android/classes.dex"),
            "a file one directory up names another file"
        );
        assert_eq!(embedded_path(EMBEDDER, "const DEX: &[u8] = &[];"), None);
        assert_eq!(
            embedded_path(EMBEDDER, &format!("{include}\n{include}")),
            None,
            "two embeddings"
        );
        assert_eq!(
            embedded_path("a.rs", r#"include_bytes!("../../x.dex")"#),
            None,
            "a path that climbs out of the repository"
        );
    }

    fn a_backend_that_embeds_another_dex_fails() {
        let embedding = |path: &str| format!("const DEX: &[u8] = include_bytes!(\"{path}\");");
        assert_eq!(
            embedding_failure(EMBEDDER, &embedding("../../../android/classes.dex")),
            None,
            "the checked-in dex"
        );
        let other = embedding_failure(EMBEDDER, &embedding("../../../android/stale/classes.dex"))
            .expect("another dex fails");
        assert!(
            other.contains("crates/flui-native/android/stale/classes.dex"),
            "the failure names what the backend embeds: {other}"
        );
        assert!(
            embedding_failure(EMBEDDER, "const DEX: &[u8] = &[];").is_some(),
            "no embedding fails"
        );
        let real = std::fs::read_to_string(repo_root().join(EMBEDDER)).expect("the embedder");
        assert_eq!(
            embedding_failure(EMBEDDER, &real),
            None,
            "the backend embeds the checked-in dex"
        );
    }

    fn the_checked_in_dex_matches_its_sources() {
        let args = AndroidDexArgs {
            write: false,
            rebuild: false,
        };
        assert_eq!(android_dex(&args).expect("runs"), ExitCode::SUCCESS);
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
                    "the_embedded_path_resolves_against_the_embedding_file",
                    the_embedded_path_resolves_against_the_embedding_file as fn(),
                ),
                (
                    "a_backend_that_embeds_another_dex_fails",
                    a_backend_that_embeds_another_dex_fails as fn(),
                ),
                (
                    "the_checked_in_dex_matches_its_sources",
                    the_checked_in_dex_matches_its_sources as fn(),
                ),
            ],
        );
    }
}
