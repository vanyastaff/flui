//! The installed Chrome and the ChromeDriver that drives it.
//!
//! ChromeDriver must match Chrome's version, so it is not a tool a host is
//! expected to have: [`chromedriver`] reads the installed Chrome's version and
//! fetches that ChromeDriver from the Chrome for Testing archives into the
//! target directory, once per version. `CHROMEDRIVER` names one to use instead
//! (the GitHub runners ship one), and `FLUI_CHROME` a Chrome other than the
//! one installed in the usual place.

use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};

/// Where Chrome for Testing publishes ChromeDriver archives, by version and
/// platform.
const ARCHIVES: &str = "https://storage.googleapis.com/chrome-for-testing-public";
/// The newest published version of a Chrome build, when the installed
/// patch release has no archive of its own.
const LATEST_OF_BUILD: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/LATEST_RELEASE";
/// Larger than any ChromeDriver archive (about 10 MB).
const ARCHIVE_LIMIT: u64 = 64 << 20;

/// An installed Chrome.
#[derive(Debug, Clone)]
pub(super) struct Chrome {
    /// Its executable.
    pub(super) binary: PathBuf,
    /// Its version, `major.minor.build.patch`.
    pub(super) version: String,
}

/// The installed Chrome, or `None` when this host has none.
pub(super) fn installed() -> anyhow::Result<Option<Chrome>> {
    let Some(binary) = binary() else {
        return Ok(None);
    };
    let version = version_of(&binary)?;
    Ok(Some(Chrome { binary, version }))
}

fn binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FLUI_CHROME").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    candidates().into_iter().find(|path| path.is_file())
}

fn candidates() -> Vec<PathBuf> {
    match std::env::consts::OS {
        "windows" => ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|root| PathBuf::from(root).join(r"Google\Chrome\Application\chrome.exe"))
            .collect(),
        "macos" => vec![PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        )],
        _ => std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .flat_map(|dir| {
                        ["google-chrome", "google-chrome-stable", "chromium"]
                            .map(|name| dir.join(name))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Chrome's version. On Windows `chrome.exe --version` prints nothing (it
/// opens a window in a running browser instead), so the version is the name
/// of the directory the installer keeps beside the executable.
fn version_of(binary: &Path) -> anyhow::Result<String> {
    if cfg!(windows) {
        let dir = binary
            .parent()
            .context("Chrome's executable has no directory")?;
        let mut versions: Vec<String> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_version(name))
            .collect();
        // An update in progress leaves the old and the new directory.
        versions.sort_by_key(|name| numbers(name));
        return versions
            .pop()
            .with_context(|| format!("no version directory beside {}", binary.display()));
    }
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("running {} --version", binary.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|word| is_version(word))
        .map(str::to_owned)
        .with_context(|| format!("no version in `{}`", text.trim()))
}

fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The Chrome for Testing name of this host's platform.
fn platform() -> anyhow::Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "win64",
        ("windows", "x86") => "win32",
        ("linux", "x86_64") => "linux64",
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        (os, arch) => bail!("Chrome for Testing publishes no ChromeDriver for {os}/{arch}"),
    })
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    }
}

/// The ChromeDriver for `chrome`: `CHROMEDRIVER` when set, else the one
/// fetched for its version into `cache`, fetching it first when it is not
/// there yet.
pub(super) fn chromedriver(chrome: &Chrome, cache: &Path) -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CHROMEDRIVER").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let dir = cache.join(&chrome.version);
    let driver = dir.join(executable_name());
    if driver.is_file() {
        return Ok(driver);
    }
    let platform = platform()?;
    let archive = match fetch(&archive_url(&chrome.version, platform)) {
        Ok(archive) => archive,
        Err(Fetch::NotFound) => {
            // Not every patch release is published; the newest of the same
            // build speaks the same protocol to it.
            let build = chrome
                .version
                .rsplit_once('.')
                .map_or(chrome.version.as_str(), |(build, _)| build);
            let latest = match fetch(&format!("{LATEST_OF_BUILD}_{build}")) {
                Ok(text) => String::from_utf8_lossy(&text).trim().to_owned(),
                Err(error) => bail!("no ChromeDriver for Chrome {}: {error}", chrome.version),
            };
            fetch(&archive_url(&latest, platform)).map_err(|error| {
                anyhow::anyhow!(
                    "fetching ChromeDriver {latest} for Chrome {}: {error}",
                    chrome.version
                )
            })?
        }
        Err(error) => bail!("fetching ChromeDriver {}: {error}", chrome.version),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    extract(&archive, &driver)?;
    println!(
        "fetched ChromeDriver for Chrome {} into {}",
        chrome.version,
        driver.display()
    );
    Ok(driver)
}

fn archive_url(version: &str, platform: &str) -> String {
    format!("{ARCHIVES}/{version}/{platform}/chromedriver-{platform}.zip")
}

#[derive(Debug)]
enum Fetch {
    NotFound,
    Failed(String),
}

impl std::fmt::Display for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("not published"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

fn fetch(url: &str) -> Result<Vec<u8>, Fetch> {
    let mut response = ureq::get(url).call().map_err(|error| match error {
        ureq::Error::StatusCode(404) => Fetch::NotFound,
        error => Fetch::Failed(format!("{url}: {error}")),
    })?;
    response
        .body_mut()
        .with_config()
        .limit(ARCHIVE_LIMIT)
        .read_to_vec()
        .map_err(|error| Fetch::Failed(format!("{url}: {error}")))
}

/// Write the archive's ChromeDriver executable to `to`.
fn extract(archive: &[u8], to: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).context("reading the archive")?;
    let name = executable_name();
    let index = (0..zip.len())
        .find(|&index| {
            zip.by_index(index).is_ok_and(|entry| {
                entry
                    .name()
                    .is_ok_and(|path| path.rsplit('/').next() == Some(name))
            })
        })
        .with_context(|| format!("the archive holds no {name}"))?;
    let mut entry = zip.by_index(index).context("reading the archive")?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .with_context(|| format!("extracting {name}"))?;
    // Written whole and then renamed, so an interrupted fetch leaves no
    // executable that a later run would take for a complete one.
    let partial = to.with_extension("partial");
    std::fs::write(&partial, &bytes).with_context(|| format!("writing {}", partial.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("marking {} executable", partial.display()))?;
    }
    std::fs::rename(&partial, to).with_context(|| format!("writing {}", to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chrome_version_has_four_numbers() {
        assert!(is_version("155.0.8059.84"));
        assert!(!is_version("155.0.8059"));
        assert!(!is_version("Google"));
        assert!(!is_version("155.0..84"));
        let mut versions = ["99.0.1.2".to_owned(), "155.0.8059.84".to_owned()];
        versions.sort_by_key(|name| numbers(name));
        assert_eq!(versions.last().map(String::as_str), Some("155.0.8059.84"));
        assert_eq!(
            archive_url("155.0.8059.84", "win64"),
            "https://storage.googleapis.com/chrome-for-testing-public/155.0.8059.84/win64/chromedriver-win64.zip"
        );
    }
}
