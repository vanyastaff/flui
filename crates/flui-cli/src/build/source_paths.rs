//! The source paths a release build leaves in the binary.
//!
//! rustc writes the path of every source file a panic can name (each
//! `core::panic::Location`) into the binary, and a path outside the package
//! being compiled is absolute: the cargo registry under `CARGO_HOME`, a path
//! dependency, the target directory's generated files. Shipped, those name the
//! build machine's user and folders, and cost bytes (in a `flui create`
//! counter on Linux, about 45 KB of 126 KB of paths are those prefixes).
//!
//! A release build of `flui build` rewrites each prefix with rustc's
//! `--remap-path-prefix`, so a registry crate's file reads
//! `wgpu-30.0.0/src/lib.rs` and a path dependency's `flui-view-0.2.0/src/…`.
//! Cargo's own `trim-paths` profile setting does this, but it is unstable in
//! the toolchain the framework pins. A debug build keeps the full paths, which
//! debuggers and editors open.
//!
//! The flags join the rustflags the author already gives cargo, from whichever
//! one source cargo reads them (its order: `CARGO_ENCODED_RUSTFLAGS`, then
//! `RUSTFLAGS`, then every `target.<triple>` and matching `target.<cfg>`
//! table's `rustflags`, then `build.rustflags`), so a build keeps its linker
//! and CPU flags. A configuration file that gives them as a string, which
//! cargo refuses to merge with a `--config` array, is read here and its flags
//! passed whole in `CARGO_ENCODED_RUSTFLAGS`. The rewrites are a function of
//! the directories alone, so two builds of one project pass the same flags and
//! cargo's cache holds.

use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::str::FromStr as _;

use cargo_metadata::Metadata;
use cargo_platform::{Cfg, Platform};

use crate::build::error::{BuildError, BuildResult};
use crate::build::util::environment;

/// The separator cargo puts between the flags of `CARGO_ENCODED_RUSTFLAGS`.
const ENCODED_SEPARATOR: char = '\x1f';

/// One rewrite: a directory of the build machine and the relative prefix its
/// files are named by in the binary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Remap {
    from: PathBuf,
    to: String,
}

impl Remap {
    /// rustc's flag. rustc splits the value at its last `=`, so `from` may
    /// hold one and `to`, a package name and version or a fixed word, holds
    /// none. Both end in a separator, so a prefix never matches half of a
    /// directory name.
    fn flag(&self) -> String {
        let mut from = self.from.to_string_lossy().into_owned();
        if !from.ends_with(MAIN_SEPARATOR) {
            from.push(MAIN_SEPARATOR);
        }
        format!("--remap-path-prefix={from}={}", self.to)
    }
}

/// The rewrites of one project's build: rustc applies the last one that
/// matches a path, so a directory inside another (a target directory in the
/// workspace) comes after it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SourcePathRemap {
    remaps: Vec<Remap>,
}

impl SourcePathRemap {
    /// The rewrites of the project in `dir`: each registry source directory
    /// of `cargo_home` (its crates are named `<name>-<version>/`), its git
    /// checkouts, every package of the resolved graph that is a path outside
    /// the workspace, the workspace root and the target directory.
    ///
    /// # Errors
    ///
    /// When `cargo metadata` fails for the project.
    pub(crate) fn of_project(dir: &Path, cargo_home: Option<&Path>) -> BuildResult<Self> {
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(dir)
            .exec()
            .map_err(|error| BuildError::CommandFailed {
                command: "cargo metadata".into(),
                exit_code: -1,
                stderr: error.to_string(),
            })?;
        let registries =
            cargo_home.map_or_default(|home| registry_sources(&home.join("registry").join("src")));
        Ok(Self::from_parts(
            &metadata,
            &registries,
            cargo_home.map(|home| home.join("git").join("checkouts")),
        ))
    }

    fn from_parts(
        metadata: &Metadata,
        registries: &[PathBuf],
        git_checkouts: Option<PathBuf>,
    ) -> Self {
        let mut remaps: Vec<Remap> = registries
            .iter()
            .map(|index| Remap {
                from: index.clone(),
                to: String::new(),
            })
            .collect();
        remaps.extend(git_checkouts.map(|from| Remap {
            from,
            to: format!("git{MAIN_SEPARATOR}"),
        }));
        let root = metadata.workspace_root.as_std_path();
        remaps.push(Remap {
            from: root.to_path_buf(),
            to: String::new(),
        });
        let mut outside: Vec<Remap> = metadata
            .packages
            .iter()
            .filter(|package| package.source.is_none())
            .filter_map(|package| {
                let dir = package.manifest_path.parent()?.as_std_path();
                (!dir.starts_with(root)).then(|| Remap {
                    from: dir.to_path_buf(),
                    to: format!("{}-{}{MAIN_SEPARATOR}", package.name, package.version),
                })
            })
            .collect();
        outside.sort_by(|a, b| a.from.cmp(&b.from));
        remaps.extend(outside);
        remaps.push(Remap {
            from: metadata.target_directory.as_std_path().to_path_buf(),
            to: format!("target{MAIN_SEPARATOR}"),
        });
        Self { remaps }
    }

    /// rustc's flags, in the order rustc reads them.
    fn flags(&self) -> Vec<String> {
        self.remaps.iter().map(Remap::flag).collect()
    }

    /// What cargo is given so that the flags join the author's rustflags from
    /// `source` (see [`RustflagsSource::of_build`]).
    pub(crate) fn onto(&self, source: &RustflagsSource) -> CargoRustflags {
        let flags = self.flags();
        if flags.is_empty() {
            return CargoRustflags::default();
        }
        let encoded = |author: Vec<String>| {
            let all: Vec<String> = author.into_iter().chain(flags.iter().cloned()).collect();
            CargoRustflags {
                args: Vec::new(),
                env: vec![(
                    "CARGO_ENCODED_RUSTFLAGS",
                    all.join(&ENCODED_SEPARATOR.to_string()),
                )],
            }
        };
        match source {
            RustflagsSource::Encoded(author) => encoded(
                author
                    .split(ENCODED_SEPARATOR)
                    .filter(|flag| !flag.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
            // `CARGO_ENCODED_RUSTFLAGS` comes before `RUSTFLAGS`, which cargo
            // splits at whitespace: the same flags, then these, whose paths
            // may hold a space.
            RustflagsSource::Plain(author) => {
                encoded(author.split_whitespace().map(str::to_owned).collect())
            }
            // A `--config` array joins the arrays of the same key from every
            // configuration file and variable.
            RustflagsSource::Target(triple) => {
                CargoRustflags::config(&format!("target.{}.rustflags", toml_key(triple)), &flags)
            }
            RustflagsSource::Build => CargoRustflags::config("build.rustflags", &flags),
            // `CARGO_ENCODED_RUSTFLAGS` comes before every configuration, so
            // it carries the configured flags themselves.
            RustflagsSource::Configured(author) => encoded(author.clone()),
        }
    }
}

/// What a release build of the project in `dir` for `triple` gives cargo on
/// top of its own arguments: the rewrites, joined to the rustflags this
/// process's environment and the project's cargo configuration give.
///
/// # Errors
///
/// When `cargo metadata` fails for the project, or `rustc` cannot list the
/// target's cfgs that a `target.'cfg(…)'` table needs.
pub(crate) fn release_rustflags(dir: &Path, triple: &str) -> BuildResult<CargoRustflags> {
    let cargo_home = environment::cargo_home(dir);
    let remap = SourcePathRemap::of_project(dir, cargo_home.as_deref())?;
    let configs = environment::cargo_configs(dir, cargo_home);
    let source = RustflagsSource::of_build(
        triple,
        |name| std::env::var(name).ok(),
        &configs,
        || environment::target_cfg(dir, triple),
    )?;
    Ok(remap.onto(&source))
}

/// The source cargo reads a build's rustflags from, of the four it tries in
/// turn (the module documentation lists them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RustflagsSource {
    /// `CARGO_ENCODED_RUSTFLAGS`, as given.
    Encoded(String),
    /// `RUSTFLAGS`, as given.
    Plain(String),
    /// A `target.<triple>` or matching `target.<cfg>` table, in a file or
    /// `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, each file's value an array: the
    /// build's triple.
    Target(String),
    /// `build.rustflags` or `CARGO_BUILD_RUSTFLAGS`, each file's value an
    /// array, or none at all.
    Build,
    /// A configuration whose source gives the flags as a string in a file,
    /// which cargo refuses to merge with a `--config` array: the flags it
    /// gives, in cargo's order.
    Configured(Vec<String>),
}

impl RustflagsSource {
    /// The source of a build for `triple` with the variables `var` answers,
    /// the cargo configuration files `configs` (each parsed, nearest first),
    /// and the cfgs of the target, asked of `target_cfg` only when a file has
    /// a `target.'cfg(…)'.rustflags` table.
    ///
    /// # Errors
    ///
    /// What `target_cfg` answers.
    pub(crate) fn of_build(
        triple: &str,
        var: impl Fn(&str) -> Option<String>,
        configs: &[toml::Table],
        target_cfg: impl FnOnce() -> BuildResult<Vec<Cfg>>,
    ) -> BuildResult<Self> {
        if let Some(flags) = var("CARGO_ENCODED_RUSTFLAGS") {
            return Ok(Self::Encoded(flags));
        }
        if let Some(flags) = var("RUSTFLAGS") {
            return Ok(Self::Plain(flags));
        }
        let target_tables = || {
            configs
                .iter()
                .filter_map(|config| config.get("target")?.as_table())
                .flat_map(|targets| targets.iter())
                .filter(|(_, table)| table.get("rustflags").is_some())
                .map(|(key, _)| key.as_str())
        };
        let mut keys: Vec<&str> = target_tables()
            .filter(|key| *key == triple)
            .take(1)
            .collect();
        let cfg_keys: Vec<&str> = target_tables()
            .filter(|key| key.starts_with("cfg("))
            .collect();
        if !cfg_keys.is_empty() {
            let cfgs = target_cfg()?;
            for key in cfg_keys {
                let matches =
                    Platform::from_str(key).is_ok_and(|platform| platform.matches(triple, &cfgs));
                if matches && !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let target_variable = format!(
            "CARGO_TARGET_{}_RUSTFLAGS",
            triple.to_ascii_uppercase().replace(['-', '.'], "_")
        );
        let target_variable = var(&target_variable);
        // Each key's values, nearest file first.
        let (per_key, variable, source): (Vec<Vec<&toml::Value>>, Option<String>, Self) =
            if target_variable.is_some() || !keys.is_empty() {
                let per_key = keys
                    .iter()
                    .map(|key| {
                        configs
                            .iter()
                            .filter_map(|config| config.get("target")?.get(*key)?.get("rustflags"))
                            .collect()
                    })
                    .collect();
                (per_key, target_variable, Self::Target(triple.to_owned()))
            } else {
                let values = configs
                    .iter()
                    .filter_map(|config| config.get("build")?.get("rustflags"))
                    .collect();
                (vec![values], var("CARGO_BUILD_RUSTFLAGS"), Self::Build)
            };
        if !per_key.iter().flatten().any(|value| value.is_str()) {
            return Ok(source);
        }
        let mut flags: Vec<String> = per_key
            .iter()
            .flat_map(|values| key_flags(values))
            .collect();
        flags.extend(
            variable
                .iter()
                .flat_map(|value| value.split_whitespace().map(str::to_owned)),
        );
        Ok(Self::Configured(flags))
    }
}

/// The flags cargo reads from one key's values, nearest file first: the
/// nearest string, split at whitespace, where one gives a string (a nearer
/// string replaces a farther one, and cargo refuses a string and an array
/// together), else every array, farthest first.
fn key_flags(values: &[&toml::Value]) -> Vec<String> {
    if let Some(flags) = values.iter().find_map(|value| value.as_str()) {
        return flags.split_whitespace().map(str::to_owned).collect();
    }
    values
        .iter()
        .rev()
        .filter_map(|value| value.as_array())
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The arguments and variables a cargo command takes on top of its own so
/// that rustc gets the author's flags and the rewrites.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CargoRustflags {
    /// Cargo arguments (`--config <key>=<array>`).
    pub(crate) args: Vec<String>,
    /// Variables to set on the command.
    pub(crate) env: Vec<(&'static str, String)>,
}

impl CargoRustflags {
    fn config(key: &str, flags: &[String]) -> Self {
        let array = toml::Value::Array(flags.iter().cloned().map(toml::Value::String).collect());
        Self {
            args: vec!["--config".into(), format!("{key}={array}")],
            env: Vec::new(),
        }
    }
}

/// A target triple as a TOML key: quoted, since a triple such as
/// `thumbv7em-none-eabihf` is bare-safe but one with a `.` (a custom target's
/// file name) is not.
fn toml_key(triple: &str) -> String {
    toml::Value::String(triple.to_owned()).to_string()
}

/// Each index directory of a registry's `src` (`index.crates.io-<hash>`),
/// sorted so that two builds pass the same flags; none when it is absent.
fn registry_sources(src: &Path) -> Vec<PathBuf> {
    let mut sources: Vec<PathBuf> = std::fs::read_dir(src)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    sources.sort();
    sources
}

#[cfg(test)]
mod tests;
