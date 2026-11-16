use std::env;
use std::path::{Path, PathBuf};

use crate::build::error::{BuildError, BuildResult};

/// Check if a command exists in PATH
pub(crate) fn check_command_exists(command: &str) -> BuildResult<PathBuf> {
    which::which(command).map_err(|_| BuildError::ToolNotFound {
        tool: command.to_string(),
        install_hint: format!("Ensure {command} is installed and in PATH"),
    })
}

/// Get environment variable with error context
pub(crate) fn get_env_var(name: &str) -> BuildResult<String> {
    env::var(name).map_err(|_| BuildError::EnvVarError {
        var: name.to_string(),
        reason: "not set".to_string(),
    })
}

/// Resolve `ANDROID_HOME` from environment or common paths
pub(crate) fn resolve_android_home() -> BuildResult<PathBuf> {
    // Try environment variable first
    if let Ok(android_home) = env::var("ANDROID_HOME") {
        let path = PathBuf::from(android_home);
        if path.exists() {
            crate::ui::debug(format!(
                "Found ANDROID_HOME from environment: {}",
                path.display()
            ));
            return Ok(path);
        }
    }

    // Try common default locations
    let common_paths = if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(env::var("LOCALAPPDATA").unwrap_or_default()).join("Android\\Sdk"),
            PathBuf::from("C:\\Android\\Sdk"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(env::var("HOME").unwrap_or_default()).join("Library/Android/sdk")]
    } else {
        vec![PathBuf::from(env::var("HOME").unwrap_or_default()).join("Android/Sdk")]
    };

    for path in common_paths {
        if path.exists() {
            crate::ui::debug(format!(
                "Found Android SDK at default location: {}",
                path.display()
            ));
            return Ok(path);
        }
    }

    Err(BuildError::EnvVarError {
        var: "ANDROID_HOME".to_string(),
        reason: "not set and Android SDK not found in default locations".to_string(),
    })
}

/// Resolve `JAVA_HOME` from environment
pub(crate) fn resolve_java_home() -> BuildResult<PathBuf> {
    let java_home = get_env_var("JAVA_HOME")?;
    let path = PathBuf::from(java_home);

    if !path.exists() {
        return Err(BuildError::PathNotFound {
            path,
            context: "JAVA_HOME path does not exist".to_string(),
        });
    }

    Ok(path)
}

/// Resolve Android NDK home (from `ANDROID_HOME/ndk`)
pub(crate) fn resolve_ndk_home(android_home: &Path) -> BuildResult<PathBuf> {
    // Try ANDROID_NDK_HOME first
    if let Ok(ndk_home) = env::var("ANDROID_NDK_HOME") {
        let path = PathBuf::from(ndk_home);
        if path.exists() {
            crate::ui::debug(format!(
                "Found NDK from ANDROID_NDK_HOME: {}",
                path.display()
            ));
            return Ok(path);
        }
    }

    // ANDROID_HOME/ndk/<version>: the highest version, compared by number.
    if let Some(path) = newest_version_dir(&android_home.join("ndk")) {
        crate::ui::debug(format!("Found NDK at: {}", path.display()));
        return Ok(path);
    }

    Err(BuildError::EnvVarError {
        var: "ANDROID_NDK_HOME".to_string(),
        reason: "not found. Set ANDROID_NDK_HOME or install NDK via Android Studio".to_string(),
    })
}

/// The subdirectory of `parent` whose name is the highest version
/// (`29.0.14206865` above `9.0.0`), its dot- or dash-separated components
/// compared as numbers, a component that is not a number counting as 0
/// (`android-36` is `[0, 36]`). `None` when `parent` has no subdirectory.
pub(crate) fn newest_version_dir(parent: &Path) -> Option<PathBuf> {
    newest_version_dir_where(parent, |_| true)
}

/// [`newest_version_dir`] among the names `accept` takes.
pub(crate) fn newest_version_dir_where(
    parent: &Path,
    accept: impl Fn(&str) -> bool,
) -> Option<PathBuf> {
    std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            accept(name).then(|| (version_key(name), entry.path()))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, path)| path)
}

/// The numeric components of a version-like name, 0 for one that is not a
/// number.
fn version_key(name: &str) -> Vec<u64> {
    name.split(['.', '-'])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// The environment every `cargo ndk` call runs with: the SDK and the NDK
/// this build resolved, so cargo-ndk links with that NDK instead of one it
/// finds on its own (it reads `ANDROID_NDK_HOME` first).
pub(crate) fn ndk_env<'a>(
    android_home: &'a Path,
    ndk_home: &'a Path,
) -> [(&'static str, &'a Path); 2] {
    [
        ("ANDROID_HOME", android_home),
        ("ANDROID_NDK_HOME", ndk_home),
    ]
}

/// How the Rust toolchain a project selects can build the standard library
/// of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetSupport {
    /// The toolchain ships the target's standard library.
    Installed,
    /// Cargo builds it from source: the project's cargo configuration asks
    /// for `build-std` and the toolchain has the `rust-src` component.
    FromSource,
    /// Neither: cargo cannot build for the target.
    Missing,
}

impl TargetSupport {
    /// What the toolchain offers, from whether the target's library directory
    /// exists, whether cargo is configured to build `std`, and whether the
    /// toolchain has the standard library's sources.
    #[must_use]
    pub(crate) fn of(libraries_installed: bool, build_std: bool, rust_src: bool) -> Self {
        if libraries_installed {
            Self::Installed
        } else if build_std && rust_src {
            Self::FromSource
        } else {
            Self::Missing
        }
    }
}

/// Refuse a build for `triple` that cargo cannot make with the Rust
/// toolchain the project in `dir` selects.
///
/// Asks that toolchain's `rustc` (the one `RUSTC` names, as cargo does)
/// where the target's libraries are, under a deadline, so it works wherever
/// the toolchain came from (rustup, a distribution, Nix) and a hung tool
/// cannot stop the build; a target without them is still buildable when the
/// project's cargo configuration builds `std` from the `rust-src` sources
/// ([`TargetSupport`]). Nothing is ever installed.
///
/// # Errors
///
/// [`BuildError::TargetNotInstalled`] naming the toolchain looked at when the
/// target is [`TargetSupport::Missing`]; [`BuildError::CommandFailed`] when
/// `rustc` cannot answer.
pub(crate) fn require_rust_target(dir: &Path, triple: &str) -> BuildResult<()> {
    match rust_target_support(dir, triple)? {
        TargetSupport::Installed => Ok(()),
        TargetSupport::FromSource => {
            crate::ui::debug(format!(
                "{triple}: the standard library is built from source (`build-std`)"
            ));
            Ok(())
        }
        TargetSupport::Missing => Err(BuildError::TargetNotInstalled {
            target: triple.to_string(),
            toolchain: toolchain_description(dir),
            install_cmd: format!("rustup target add {triple}"),
        }),
    }
}

/// What the Rust toolchain the project in `dir` selects offers for `triple`
/// (see [`require_rust_target`]).
///
/// # Errors
///
/// [`BuildError::CommandFailed`] when `rustc` cannot be started, fails, or
/// does not answer before [`crate::proc::PROBE_TIMEOUT`].
pub(crate) fn rust_target_support(dir: &Path, triple: &str) -> BuildResult<TargetSupport> {
    // rustc refuses a triple it does not know at all; that is a target this
    // toolchain has no libraries for, not a failed probe.
    let libraries_installed = rustc_answer(dir, &["--print", "target-libdir", "--target", triple])?
        .is_some_and(|libdir| !libdir.is_empty() && Path::new(&libdir).is_dir());
    if libraries_installed {
        return Ok(TargetSupport::Installed);
    }
    let cargo_home = env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
        env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(|home| PathBuf::from(home).join(".cargo"))
    });
    let build_std = build_std_configured(dir, env::var_os("CARGO_UNSTABLE_BUILD_STD"), cargo_home);
    let rust_src = build_std
        && rustc_answer(dir, &["--print", "sysroot"])?.is_some_and(|sysroot| {
            Path::new(&sysroot)
                .join("lib/rustlib/src/rust/library/core")
                .is_dir()
        });
    Ok(TargetSupport::of(false, build_std, rust_src))
}

/// Whether cargo builds `std` for a project in `dir`: `CARGO_UNSTABLE_BUILD_STD`
/// (`build_std_env`) is set, or a cargo configuration file it reads (each
/// `.cargo/config.toml` or `.cargo/config` from `dir` up, then the one in
/// `cargo_home`) states `[unstable] build-std`.
pub(crate) fn build_std_configured(
    dir: &Path,
    build_std_env: Option<std::ffi::OsString>,
    cargo_home: Option<PathBuf>,
) -> bool {
    if build_std_env.is_some_and(|value| !value.is_empty()) {
        return true;
    }
    dir.ancestors()
        .map(|ancestor| ancestor.join(".cargo"))
        .chain(cargo_home)
        .flat_map(|cargo_dir| [cargo_dir.join("config.toml"), cargo_dir.join("config")])
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .any(|text| states_build_std(&text))
}

/// Whether the cargo configuration `text` states a non-empty
/// `[unstable] build-std`.
fn states_build_std(text: &str) -> bool {
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    table
        .get("unstable")
        .and_then(|unstable| unstable.get("build-std"))
        .is_some_and(|crates| match crates {
            toml::Value::Array(crates) => !crates.is_empty(),
            toml::Value::Boolean(on) => *on,
            _ => false,
        })
}

/// The toolchain the project in `dir` selects, as a refusal names it: its
/// release and its sysroot, or what could be learned of them.
fn toolchain_description(dir: &Path) -> String {
    let release = rustc_answer(dir, &["-V"]).ok().flatten();
    let sysroot = rustc_answer(dir, &["--print", "sysroot"]).ok().flatten();
    match (release, sysroot) {
        (Some(release), Some(sysroot)) => format!("{release} at {sysroot}"),
        (Some(release), None) => release,
        (None, Some(sysroot)) => format!("the toolchain at {sysroot}"),
        (None, None) => "the toolchain `rustc` runs".to_owned(),
    }
}

/// The trimmed standard output of the project's `rustc` with `args`, run in
/// `dir` under [`crate::proc::PROBE_TIMEOUT`]; `None` when it exits with a
/// failure.
///
/// # Errors
///
/// [`BuildError::CommandFailed`] when `rustc` cannot be started or does not
/// answer in time.
fn rustc_answer(dir: &Path, args: &[&str]) -> BuildResult<Option<String>> {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = crate::proc::output_with_timeout(
        std::process::Command::new(rustc)
            .args(args)
            .current_dir(dir),
        crate::proc::PROBE_TIMEOUT,
    )
    .map_err(|error| BuildError::CommandFailed {
        command: format!("rustc {}", args.join(" ")),
        exit_code: -1,
        stderr: error.to_string(),
    })?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

/// The host triple of the Rust toolchain the project in `dir` selects, from
/// `rustc -vV`, under a deadline.
///
/// # Errors
///
/// [`BuildError::CommandFailed`] when `rustc` cannot be started, fails, does
/// not answer in time or prints no `host:` line.
pub(crate) fn rust_host_triple(dir: &Path) -> BuildResult<String> {
    let failed = |reason: &str| BuildError::CommandFailed {
        command: "rustc -vV".to_string(),
        exit_code: -1,
        stderr: reason.to_owned(),
    };
    rustc_answer(dir, &["-vV"])?
        .ok_or_else(|| failed("rustc failed"))?
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(|host| host.trim().to_string())
        .ok_or_else(|| failed("no `host:` line"))
}

#[cfg(test)]
mod tests {
    use super::{TargetSupport, build_std_configured, newest_version_dir};

    /// A target is buildable when its libraries are installed, or when cargo
    /// builds `std` from the toolchain's sources; a `build-std` configuration
    /// is read from the project's directories, the cargo home and the
    /// environment, and never from a table that does not ask for it.
    #[test]
    fn a_target_is_buildable_from_its_libraries_or_from_source() {
        let mut failures = Vec::new();
        for (libraries, build_std, rust_src, expected) in [
            (true, false, false, TargetSupport::Installed),
            (false, true, true, TargetSupport::FromSource),
            (false, true, false, TargetSupport::Missing),
            (false, false, true, TargetSupport::Missing),
        ] {
            let found = TargetSupport::of(libraries, build_std, rust_src);
            if found != expected {
                failures.push(format!("{libraries} {build_std} {rust_src}: {found:?}"));
            }
        }

        let tree = tempfile::tempdir().expect("tree");
        let project = tree.path().join("workspace").join("app");
        std::fs::create_dir_all(&project).expect("project");
        let home = tree.path().join("cargo-home");
        std::fs::create_dir_all(&home).expect("cargo home");
        let config = |dir: &std::path::Path, text: &str| {
            std::fs::create_dir_all(dir).expect("config dir");
            std::fs::write(dir.join("config.toml"), text).expect("config");
        };
        let configured = |env: Option<&str>| {
            build_std_configured(&project, env.map(Into::into), Some(home.clone()))
        };
        if configured(None) {
            failures.push("no configuration".to_owned());
        }
        if !configured(Some("std,panic_abort")) {
            failures.push("CARGO_UNSTABLE_BUILD_STD".to_owned());
        }
        config(&home, "[unstable]\nbuild-std = []\n[build]\njobs = 4\n");
        if configured(None) {
            failures.push("an empty build-std".to_owned());
        }
        config(
            &tree.path().join("workspace").join(".cargo"),
            "[unstable]\nbuild-std = [\"std\", \"panic_abort\"]\n",
        );
        if !configured(None) {
            failures.push("a workspace's .cargo/config.toml".to_owned());
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The NDK and the build-tools are the highest version by number, so
    /// `29.0.14206865` wins over `9.0.0`, a name that is no version loses to
    /// every version, and a file is never taken.
    #[test]
    fn the_newest_version_is_compared_by_number() {
        let parent = tempfile::tempdir().expect("versions");
        for name in ["9.0.0", "29.0.14206865", "28.2.13676358", "latest"] {
            std::fs::create_dir(parent.path().join(name)).expect("version dir");
        }
        std::fs::write(parent.path().join("30.0.0"), b"a file").expect("file");
        assert_eq!(
            newest_version_dir(parent.path()),
            Some(parent.path().join("29.0.14206865"))
        );
        assert_eq!(newest_version_dir(&parent.path().join("missing")), None);
    }
}
