use std::env;
use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};

/// Check if a command exists in PATH
pub fn check_command_exists(command: &str) -> BuildResult<PathBuf> {
    which::which(command).map_err(|_| BuildError::ToolNotFound {
        tool: command.to_string(),
        install_hint: format!("Ensure {command} is installed and in PATH"),
    })
}

/// Get environment variable with error context
pub fn get_env_var(name: &str) -> BuildResult<String> {
    env::var(name).map_err(|_| BuildError::EnvVarError {
        var: name.to_string(),
        reason: "not set".to_string(),
    })
}

/// Resolve `ANDROID_HOME` from environment or common paths
pub fn resolve_android_home() -> BuildResult<PathBuf> {
    // Try environment variable first
    if let Ok(android_home) = env::var("ANDROID_HOME") {
        let path = PathBuf::from(android_home);
        if path.exists() {
            tracing::debug!("Found ANDROID_HOME from environment: {:?}", path);
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
            tracing::debug!("Found Android SDK at default location: {:?}", path);
            return Ok(path);
        }
    }

    Err(BuildError::EnvVarError {
        var: "ANDROID_HOME".to_string(),
        reason: "not set and Android SDK not found in default locations".to_string(),
    })
}

/// Resolve `JAVA_HOME` from environment
pub fn resolve_java_home() -> BuildResult<PathBuf> {
    let java_home = get_env_var("JAVA_HOME")?;
    let path = PathBuf::from(java_home);

    if !path.exists() {
        return Err(BuildError::PathNotFound {
            path: path.clone(),
            context: "JAVA_HOME path does not exist".to_string(),
        });
    }

    Ok(path)
}

/// Resolve Android NDK home (from `ANDROID_HOME/ndk`)
pub fn resolve_ndk_home(android_home: &Path) -> BuildResult<PathBuf> {
    // Try ANDROID_NDK_HOME first
    if let Ok(ndk_home) = env::var("ANDROID_NDK_HOME") {
        let path = PathBuf::from(ndk_home);
        if path.exists() {
            tracing::debug!("Found NDK from ANDROID_NDK_HOME: {:?}", path);
            return Ok(path);
        }
    }

    // Try ANDROID_HOME/ndk/<version>
    let ndk_dir = android_home.join("ndk");
    if ndk_dir.exists() {
        // Find the latest NDK version
        let mut versions = vec![];
        for entry in std::fs::read_dir(&ndk_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                versions.push(entry.path());
            }
        }

        if !versions.is_empty() {
            versions.sort();
            let latest = versions.last().unwrap();
            tracing::debug!("Found NDK at: {:?}", latest);
            return Ok(latest.clone());
        }
    }

    Err(BuildError::EnvVarError {
        var: "ANDROID_NDK_HOME".to_string(),
        reason: "not found. Set ANDROID_NDK_HOME or install NDK via Android Studio".to_string(),
    })
}
