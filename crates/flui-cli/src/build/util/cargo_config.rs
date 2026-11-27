//! The cargo configuration files a cargo command reads, found and layered as
//! cargo finds and layers them, for the commands of the CLI that must know
//! what cargo will be configured with before it runs (the registries a
//! declaration names, `build-std`, the rustflags a release build joins).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// How deep `include`s nest before the rest is not read.
const INCLUDE_DEPTH: usize = 16;

/// The cargo home a cargo command run in `dir` reads, as cargo's
/// `home::cargo_home_with_cwd`, from the values `CARGO_HOME` and the user's
/// home directory have: `CARGO_HOME` read against `dir` when relative and
/// unset when empty, else `.cargo` in the home directory.
pub(crate) fn cargo_home_with(
    dir: &Path,
    cargo_home: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    cargo_home
        .filter(|home| !home.is_empty())
        .map(|home| dir.join(home))
        .or_else(|| home.map(|home| home.join(".cargo")))
}

/// [`cargo_home_with`] this process's environment, which the cargo it starts
/// inherits.
pub(crate) fn cargo_home(dir: &Path) -> Option<PathBuf> {
    cargo_home_with(dir, std::env::var_os("CARGO_HOME"), std::env::home_dir())
}

/// The configuration files a cargo command run in `dir` reads, nearest first:
/// in each `.cargo` from `dir` up, then in `cargo_home`, the file `config`,
/// which cargo reads before `config.toml`, or else `config.toml`.
pub(crate) fn files(dir: &Path, cargo_home: Option<PathBuf>) -> Vec<PathBuf> {
    dir.ancestors()
        .map(|ancestor| ancestor.join(".cargo"))
        .chain(cargo_home)
        .filter_map(|cargo_dir| {
            ["config", "config.toml"]
                .iter()
                .map(|file| cargo_dir.join(file))
                .find(|file| file.is_file())
        })
        .collect()
}

/// The tables one configuration file contributes, in the order cargo merges
/// them, a later one over an earlier (stable since cargo 1.93; the `include`
/// key of cargo's configuration reference and `load_includes` in cargo's
/// `src/context/mod.rs`): each included file's own layers first, left to
/// right, then the file's own table. An include is a path ending in `.toml`,
/// or a table with its `path` and `optional`, relative to the including
/// file's directory. A file already being read (a cycle, which cargo refuses)
/// or deeper than [`INCLUDE_DEPTH`] adds nothing; so does a file that cannot
/// be read or is not TOML, which cargo refuses before it runs.
pub(crate) fn layers(file: &Path) -> Vec<toml::Table> {
    let mut layers = Vec::new();
    layers_into(file, &mut Vec::new(), &mut layers);
    layers
}

fn layers_into(file: &Path, reading: &mut Vec<PathBuf>, layers: &mut Vec<toml::Table>) {
    if reading.len() > INCLUDE_DEPTH || reading.iter().any(|open| open == file) {
        return;
    }
    let Some(table) = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
    else {
        return;
    };
    reading.push(file.to_path_buf());
    let base = file.parent().unwrap_or(Path::new(""));
    let includes: Vec<PathBuf> = table
        .get("include")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|include| match include {
            toml::Value::String(path) => Some(path.as_str()),
            toml::Value::Table(include) => include.get("path").and_then(toml::Value::as_str),
            _ => None,
        })
        // As cargo checks it: the extension exactly `toml`.
        .filter(|path| Path::new(path).extension() == Some(std::ffi::OsStr::new("toml")))
        .map(|path| base.join(path))
        .collect();
    for include in includes {
        layers_into(&include, reading, layers);
    }
    layers.push(table);
    reading.pop();
}

/// Every table a cargo command run in `dir` reads, highest precedence first:
/// the nearest file's own table, then its includes from the last, then the
/// next file's.
pub(crate) fn tables_by_precedence(dir: &Path, cargo_home: Option<PathBuf>) -> Vec<toml::Table> {
    files(dir, cargo_home)
        .iter()
        .flat_map(|file| layers(file).into_iter().rev())
        .collect()
}
