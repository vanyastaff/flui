use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

/// Check if a command exists in PATH
pub fn check_command_exists(command: &str) -> Result<PathBuf> {
    which::which(command)
        .with_context(|| format!("{} not found in PATH", command))
}

/// Get environment variable with error context
pub fn get_env_var(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{} environment variable not set", name))
}

/// Resolve ANDROID_HOME from environment or common paths
pub fn resolve_android_home() -> Result<PathBuf> {
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
        vec![
            PathBuf::from(env::var("HOME").unwrap_or_default()).join("Library/Android/sdk"),
        ]
    } else {
        vec![
            PathBuf::from(env::var("HOME").unwrap_or_default()).join("Android/Sdk"),
        ]
    };

    for path in common_paths {
        if path.exists() {
            tracing::debug!("Found Android SDK at default location: {:?}", path);
            return Ok(path);
        }
    }

    Err(anyhow!(
        "ANDROID_HOME not set and Android SDK not found in default locations"
    ))
}

/// Resolve JAVA_HOME from environment
pub fn resolve_java_home() -> Result<PathBuf> {
    let java_home = get_env_var("JAVA_HOME")?;
    let path = PathBuf::from(java_home);

    if !path.exists() {
        return Err(anyhow!("JAVA_HOME path does not exist: {:?}", path));
    }

    Ok(path)
}

/// Resolve Android NDK home (from ANDROID_HOME/ndk)
pub fn resolve_ndk_home(android_home: &Path) -> Result<PathBuf> {
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

    Err(anyhow!(
        "Android NDK not found. Set ANDROID_NDK_HOME or install NDK via Android Studio"
    ))
}
