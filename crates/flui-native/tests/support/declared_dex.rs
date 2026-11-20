//! Whether each dex a crate declares is the one compiled from its Java sources
//! (ADR-0247 item 1, ADR-0377 §2).
//!
//! A crate declares its Java sources and their compiled dex in
//! `[package.metadata.flui.declares.android]` (`java`, `dex`; ADR-0307), at
//! the top or under `features.<name>`. The dex is checked in, so building FLUI
//! for Android needs no JDK, and the record beside it
//! (`classes.dex` → `classes.dex.toml`) holds the SHA-256 of the sources and
//! of the dex. The `Android dex` workflow (`.github/workflows/android-dex.yml`,
//! `mode: write`) compiles the dex and writes the record; this module reads
//! both digests back without a JDK, so a source edited without rebuilding the
//! dex, or a dex replaced by hand, fails.
//!
//! One file for every crate that declares a dex: `flui-native`'s and
//! `flui-system`'s `android_dex` test modules mount it with `#[path]`, so the
//! digest the workflow's shell computes has one Rust reading.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One declared dex, by paths relative to the crate's directory and
/// `/`-separated, as the manifest names them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DeclaredDex {
    /// The directory of the Java sources.
    pub(crate) sources: String,
    /// The checked-in dex.
    pub(crate) dex: String,
    /// Its record: the dex's path with `.toml` added.
    pub(crate) record: String,
}

/// The dex a manifest declares: the `java` and `dex` of
/// `package.metadata.flui.declares.android` and of each
/// `package.metadata.flui.declares.features.<name>.android`, each once and in
/// path order.
///
/// # Errors
///
/// A table that names one of `java` and `dex` without the other, or either as
/// anything but a string.
pub(crate) fn declared_dex(manifest: &toml::Table) -> Result<Vec<DeclaredDex>, String> {
    let Some(declares) = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("flui"))
        .and_then(|flui| flui.get("declares"))
    else {
        return Ok(Vec::new());
    };
    let features = declares
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::values);
    let mut declared = Vec::new();
    for android in std::iter::once(declares)
        .chain(features)
        .filter_map(|table| table.get("android"))
    {
        let path = |key: &str| -> Result<Option<String>, String> {
            android
                .get(key)
                .map(|value| {
                    value
                        .as_str()
                        .map(|path| path.trim_start_matches("./").to_owned())
                        .ok_or_else(|| format!("android.{key} is not a path"))
                })
                .transpose()
        };
        match (path("java")?, path("dex")?) {
            (Some(sources), Some(dex)) => declared.push(DeclaredDex {
                record: format!("{dex}.toml"),
                sources,
                dex,
            }),
            (None, None) => {}
            _ => return Err("android.java and android.dex are declared together".to_owned()),
        }
    }
    declared.sort();
    declared.dedup();
    Ok(declared)
}

/// The two digests a `classes.dex.toml` records, and that it names the tools
/// the workflow's `rebuild` mode compiles with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    /// [`sources_digest`] of the Java sources the dex was compiled from.
    pub(crate) sources: String,
    /// The SHA-256 of the dex, in lowercase hex.
    pub(crate) dex: String,
}

impl Record {
    /// # Errors
    ///
    /// Text that is not TOML, or a record without one of `sources`, `dex`,
    /// `javac`, `build-tools` and `platform` as a string.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table = text
            .parse()
            .map_err(|error| format!("the record is not TOML: {error}"))?;
        let field = |name: &str| -> Result<String, String> {
            table
                .get(name)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("the record has no `{name}`"))
        };
        for tool in ["javac", "build-tools", "platform"] {
            field(tool)?;
        }
        Ok(Self {
            sources: field("sources")?,
            dex: field("dex")?,
        })
    }
}

fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").expect("writing to a String does not fail");
        out
    })
}

/// The digest of the sources: each file's path relative to the sources'
/// directory and its bytes, in path order, each followed by a NUL, so renaming
/// a file changes it too. The workflow computes the same bytes with
/// `printf '%s\0'` and `sha256sum`.
pub(crate) fn sources_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

/// The `.java` files under `dir`, by path relative to it, `/`-separated and
/// sorted as strings. Symbolic links are not followed.
fn java_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = std::fs::read_dir(&next)
            .unwrap_or_else(|error| panic!("reading {}: {error}", next.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| panic!("reading {}: {error}", next.display()));
            let path = entry.path();
            let file_type = entry
                .file_type()
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "java") {
                let relative = path
                    .strip_prefix(dir)
                    .expect("a walked path sits under its root")
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                files.push((relative, path));
            }
        }
    }
    files.sort();
    files
}

/// What is wrong with the checked-in dex `declared` of the crate at
/// `crate_dir`, named `crate_name`: one line per mismatch.
fn mismatches(crate_name: &str, crate_dir: &Path, declared: &DeclaredDex) -> Vec<String> {
    const REBUILD: &str = "rebuild it by running `.github/workflows/android-dex.yml` with \
                           `mode: write` on the branch (`gh workflow run android-dex.yml --ref \
                           <branch>`) and commit its `android-dex` artifact";
    let DeclaredDex {
        sources: sources_dir,
        dex: dex_path,
        record: record_path,
    } = declared;
    let sources: Vec<(String, Vec<u8>)> = java_files(&crate_dir.join(sources_dir))
        .into_iter()
        .map(|(relative, path)| {
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            (relative, bytes)
        })
        .collect();
    if sources.is_empty() {
        return vec![format!(
            "{crate_name}: no Java sources under {sources_dir}, which {dex_path} is declared to be compiled from"
        )];
    }
    let record_text = std::fs::read_to_string(crate_dir.join(record_path))
        .unwrap_or_else(|error| panic!("{crate_name}: reading {record_path}: {error}"));
    let record = Record::parse(&record_text)
        .unwrap_or_else(|error| panic!("{crate_name}: {record_path}: {error}"));
    let dex = std::fs::read(crate_dir.join(dex_path))
        .unwrap_or_else(|error| panic!("{crate_name}: reading {dex_path}: {error}"));
    let mut found = Vec::new();
    if record.sources != sources_digest(&sources) {
        found.push(format!(
            "{crate_name}: the Java sources under {sources_dir} changed since {dex_path} was \
             built; {REBUILD}"
        ));
    }
    if record.dex != hex(&Sha256::digest(&dex)) {
        found.push(format!(
            "{crate_name}: {dex_path} is not the dex {record_path} records; {REBUILD}"
        ));
    }
    found
}

/// Panics unless every dex the crate at `crate_dir` declares in its manifest
/// is the one its record names, compiled from its Java sources, and the crate
/// declares at least one.
pub(crate) fn assert_declared_dex_is_built_from_its_sources(crate_dir: &Path) {
    let manifest_path = crate_dir.join("Cargo.toml");
    let manifest: toml::Table = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", manifest_path.display()))
        .parse()
        .unwrap_or_else(|error| panic!("parsing {}: {error}", manifest_path.display()));
    let crate_name = manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{} names no package", manifest_path.display()));
    let declared = declared_dex(&manifest)
        .unwrap_or_else(|error| panic!("{}: {error}", manifest_path.display()));
    assert!(
        !declared.is_empty(),
        "{crate_name} declares no `android.dex` and `android.java`"
    );
    let found: Vec<String> = declared
        .iter()
        .flat_map(|declared| mismatches(crate_name, crate_dir, declared))
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}
