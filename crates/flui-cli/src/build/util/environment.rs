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

/// Refuse a build for `triple` when the Rust toolchain the project in `dir`
/// selects has no standard library for it.
///
/// Asks `rustc` (the one `RUSTC` names, as cargo does) where the target's
/// libraries are, under a deadline, so it works wherever the toolchain came
/// from (rustup, a distribution, Nix) and a hung tool cannot stop the build.
///
/// # Errors
///
/// [`BuildError::TargetNotInstalled`] when the target has no library
/// directory, [`BuildError::CommandFailed`] when `rustc` cannot answer.
pub(crate) fn require_rust_target(dir: &Path, triple: &str) -> BuildResult<()> {
    if rust_target_installed(dir, triple)? {
        Ok(())
    } else {
        Err(BuildError::TargetNotInstalled {
            target: triple.to_string(),
            install_cmd: format!("rustup target add {triple}"),
        })
    }
}

/// Whether the Rust toolchain the project in `dir` selects has a standard
/// library for `triple` (see [`require_rust_target`]).
///
/// # Errors
///
/// [`BuildError::CommandFailed`] when `rustc` cannot be started, fails, or
/// does not answer before [`crate::proc::PROBE_TIMEOUT`].
pub(crate) fn rust_target_installed(dir: &Path, triple: &str) -> BuildResult<bool> {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let rendered = format!("rustc --print target-libdir --target {triple}");
    let failed = |reason: String| BuildError::CommandFailed {
        command: rendered.clone(),
        exit_code: -1,
        stderr: reason,
    };
    let output = crate::proc::output_with_timeout(
        std::process::Command::new(rustc)
            .args(["--print", "target-libdir", "--target", triple])
            .current_dir(dir),
        crate::proc::PROBE_TIMEOUT,
    )
    .map_err(|error| failed(error.to_string()))?;
    if !output.status.success() {
        // rustc refuses a triple it does not know at all; that is a target
        // this toolchain cannot build, not a failed probe.
        return Ok(false);
    }
    let libdir = String::from_utf8_lossy(&output.stdout);
    let libdir = Path::new(libdir.trim());
    Ok(!libdir.as_os_str().is_empty() && libdir.is_dir())
}

/// The host triple of the Rust toolchain the project in `dir` selects, from
/// `rustc -vV`, under a deadline.
///
/// # Errors
///
/// [`BuildError::CommandFailed`] when `rustc` cannot be started, fails, does
/// not answer in time or prints no `host:` line.
pub(crate) fn rust_host_triple(dir: &Path) -> BuildResult<String> {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let failed = |reason: String| BuildError::CommandFailed {
        command: "rustc -vV".to_string(),
        exit_code: -1,
        stderr: reason,
    };
    let output = crate::proc::output_with_timeout(
        std::process::Command::new(rustc)
            .arg("-vV")
            .current_dir(dir),
        crate::proc::PROBE_TIMEOUT,
    )
    .map_err(|error| failed(error.to_string()))?;
    if !output.status.success() {
        return Err(failed(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(|host| host.trim().to_string())
        .ok_or_else(|| failed("no `host:` line".to_string()))
}

#[cfg(test)]
mod tests {
    use super::newest_version_dir;

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
