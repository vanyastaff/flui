//! The installed Chrome, the ChromeDriver that drives it, and the WebDriver session ChromeDriver
//! opens in headless Chrome.
//!
//! ChromeDriver must match Chrome's version, so it is not a tool a host is expected to have:
//! [`resolve`] takes `CHROMEDRIVER` when it names one (the GitHub runners ship one), else the one
//! fetched for the installed Chrome's version into the cache before, else fetches that one from
//! the Chrome for Testing archives into the cache, once per version. `FLUI_CHROME` names a Chrome
//! other than the one installed in the usual place.
//!
//! What ChromeDriver and the archives answer is untrusted: every body is read within a bound
//! ([`MAX_ARCHIVE`], [`MAX_ANSWER`]), every request within a deadline, and the session's
//! WebSocket must be on the loopback (`bidi`). The session's Chrome is always headless: no window
//! of it reaches the desktop.

use std::io::{BufRead as _, BufReader, Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::launch::AppProcess;

/// Where Chrome for Testing publishes its archives, and the newest release of each build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeForTesting {
    /// The archives' root: `<root>/<version>/<platform>/chromedriver-<platform>.zip`.
    pub archives: String,
    /// The root of the newest release of each build: `<root>_<major.minor.build>`.
    pub latest_of_build: String,
}

impl Default for ChromeForTesting {
    fn default() -> Self {
        Self {
            archives: "https://storage.googleapis.com/chrome-for-testing-public".to_owned(),
            latest_of_build: "https://googlechromelabs.github.io/chrome-for-testing/LATEST_RELEASE"
                .to_owned(),
        }
    }
}

/// Larger than any ChromeDriver archive (about 10 MB).
pub const MAX_ARCHIVE: u64 = 64 << 20;

/// The most bytes of an answer of ChromeDriver's, or of a release name, the lane reads.
pub const MAX_ANSWER: u64 = 1 << 20;

/// How long a fetch of the archives may take, whole.
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// How long ChromeDriver gets to report its port, and to open a session in a starting Chrome.
const STARTUP: Duration = Duration::from_secs(60);

/// How long a session's end may take.
const END_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest line of ChromeDriver's output the lane reads whole.
const MAX_OUTPUT_LINE: u64 = 4096;

/// An installed Chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chrome {
    /// Its executable.
    pub binary: PathBuf,
    /// Its version, `major.minor.build.patch`.
    pub version: String,
}

/// The installed Chrome: `FLUI_CHROME`'s, else the one in the usual place; `None` when this host
/// has none.
///
/// # Errors
///
/// When its version cannot be read.
pub fn installed() -> Result<Option<Chrome>> {
    let named = std::env::var_os("FLUI_CHROME").filter(|path| !path.is_empty());
    let Some(binary) = named
        .map(PathBuf::from)
        .or_else(|| candidates().into_iter().find(|path| path.is_file()))
    else {
        return Ok(None);
    };
    let version = version_of(&binary)?;
    Ok(Some(Chrome { binary, version }))
}

fn candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|root| PathBuf::from(root).join(r"Google\Chrome\Application\chrome.exe"))
            .collect()
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        )]
    } else {
        std::env::var_os("PATH").map_or_else(Vec::new, |path| {
            std::env::split_paths(&path)
                .flat_map(|dir| {
                    [
                        "google-chrome",
                        "google-chrome-stable",
                        "chromium",
                        "chromium-browser",
                    ]
                    .map(|name| dir.join(name))
                })
                .collect()
        })
    }
}

/// Chrome's version. On Windows `chrome.exe --version` prints nothing (it opens a window in a
/// running browser instead), so the version is the name of the newest version directory the
/// installer keeps beside the executable.
fn version_of(binary: &Path) -> Result<String> {
    let unreadable = |why: String| Error::Lane {
        lane: "chrome".to_owned(),
        unavailable: true,
        message: format!(
            "the version of Chrome at {} cannot be read: {why}",
            binary.display()
        ),
    };
    if cfg!(windows) {
        let dir = binary
            .parent()
            .ok_or_else(|| unreadable("its executable has no directory".to_owned()))?;
        let versions = std::fs::read_dir(dir)
            .map_err(|error| unreadable(error.to_string()))?
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<String>>();
        return newest_version(&versions)
            .ok_or_else(|| unreadable("no version directory beside it".to_owned()));
    }
    // A headless start never opens a window; `--version` alone does not start a browser.
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| unreadable(error.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|word| is_version(word))
        .map(str::to_owned)
        .ok_or_else(|| unreadable(format!("no version in `{}`", text.trim())))
}

/// Whether `text` is a Chrome version: four dot-separated numbers.
#[must_use]
pub fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The newest of the Chrome versions among `names`, compared number by number: an update in
/// progress leaves the old and the new directory.
#[must_use]
pub fn newest_version(names: &[String]) -> Option<String> {
    names
        .iter()
        .filter(|name| is_version(name))
        .max_by_key(|name| {
            name.split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<u64>>()
        })
        .cloned()
}

/// The Chrome for Testing name of this host's platform; `None` for one it publishes nothing for.
#[must_use]
pub fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "win64",
        ("windows", "x86") => "win32",
        ("linux", "x86_64") => "linux64",
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        _ => return None,
    })
}

/// ChromeDriver's executable name on this host.
#[must_use]
pub fn executable_name() -> &'static str {
    if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    }
}

/// Where fetched ChromeDrivers are kept: the user's cache directory's `flui/chromedriver`.
#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let root = if cfg!(windows) {
        var("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    Some(root.join("flui").join("chromedriver"))
}

/// The ChromeDriver for `chrome`: `CHROMEDRIVER` when set, else the one kept for its version in
/// `cache`, fetched from `archives` first when it is not there yet.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane when no ChromeDriver is published
/// for this platform or Chrome's version; the fetch's failure.
pub fn resolve(chrome: &Chrome, cache: &Path, archives: &ChromeForTesting) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CHROMEDRIVER").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let kept = cache.join(&chrome.version).join(executable_name());
    if kept.is_file() {
        return Ok(kept);
    }
    let platform = platform().ok_or_else(|| Error::Lane {
        lane: "chrome".to_owned(),
        unavailable: true,
        message: format!(
            "Chrome for Testing publishes no ChromeDriver for {}/{}; set CHROMEDRIVER",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    })?;
    fetch(archives, &chrome.version, platform, &kept)?;
    Ok(kept)
}

/// The archive URL of ChromeDriver `version` for `platform` under `archives`.
#[must_use]
pub fn archive_url(archives: &str, version: &str, platform: &str) -> String {
    format!("{archives}/{version}/{platform}/chromedriver-{platform}.zip")
}

/// Fetch ChromeDriver for Chrome `version` on `platform` and write it to `to`. Not every patch
/// release is published; the newest of the same build speaks the same protocol to it.
///
/// # Errors
///
/// [`Error::Lane`] naming what failed: a fetch, an archive past [`MAX_ARCHIVE`] or without
/// ChromeDriver in it, the write.
pub fn fetch(archives: &ChromeForTesting, version: &str, platform: &str, to: &Path) -> Result<()> {
    let agent = agent();
    let archive = match get(
        &agent,
        &archive_url(&archives.archives, version, platform),
        MAX_ARCHIVE,
    ) {
        Ok(archive) => archive,
        Err(Fetched::NotFound) => {
            let build = version.rsplit_once('.').map_or(version, |(build, _)| build);
            let latest = get(&agent, &format!("{}_{build}", archives.latest_of_build), 64)
                .map_err(|failure| fetch_failed(version, &failure.to_string()))?;
            let latest = String::from_utf8_lossy(&latest).trim().to_owned();
            if !is_version(&latest) {
                return Err(fetch_failed(
                    version,
                    &format!("the newest release of build {build} is named {latest:?}"),
                ));
            }
            get(
                &agent,
                &archive_url(&archives.archives, &latest, platform),
                MAX_ARCHIVE,
            )
            .map_err(|failure| fetch_failed(version, &failure.to_string()))?
        }
        Err(failure) => return Err(fetch_failed(version, &failure.to_string())),
    };
    let dir = to
        .parent()
        .ok_or_else(|| fetch_failed(version, "the cache path has no directory"))?;
    std::fs::create_dir_all(dir)?;
    extract(&archive, to)
}

fn fetch_failed(version: &str, why: &str) -> Error {
    Error::Lane {
        lane: "chrome".to_owned(),
        unavailable: true,
        message: format!(
            "no ChromeDriver for Chrome {version}: {why}; set CHROMEDRIVER to one that matches"
        ),
    }
}

/// The HTTP client of the fetch and of ChromeDriver's session: rustls with the platform's
/// verifier, no redirect, a deadline over each exchange.
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .new_agent()
}

#[derive(Debug)]
enum Fetched {
    NotFound,
    Failed(String),
}

impl std::fmt::Display for Fetched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("not published"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> std::result::Result<Vec<u8>, Fetched> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| Fetched::Failed(format!("{url}: {error}")))?;
    match response.status().as_u16() {
        200 => {}
        404 => return Err(Fetched::NotFound),
        status => return Err(Fetched::Failed(format!("{url}: status {status}"))),
    }
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => {
                Fetched::Failed(format!("{url}: larger than {limit} bytes"))
            }
            error => Fetched::Failed(format!("{url}: {error}")),
        })
}

/// Write the archive's ChromeDriver executable to `to`: whole to a partial file, then renamed, so
/// an interrupted fetch leaves nothing a later run takes for a complete one.
///
/// # Errors
///
/// [`Error::Lane`] for an archive that does not read, holds no ChromeDriver, or whose ChromeDriver
/// is past [`MAX_ARCHIVE`] unpacked; the write's failure.
pub fn extract(archive: &[u8], to: &Path) -> Result<()> {
    let refused = |why: String| Error::Lane {
        lane: "chrome".to_owned(),
        unavailable: true,
        message: format!("the ChromeDriver archive is refused: {why}"),
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| refused(e.to_string()))?;
    let name = executable_name();
    let index = (0..zip.len())
        .find(|&index| {
            zip.by_index(index).is_ok_and(|entry| {
                entry
                    .name()
                    .is_ok_and(|path| path.rsplit('/').next() == Some(name))
            })
        })
        .ok_or_else(|| refused(format!("it holds no {name}")))?;
    let entry = zip.by_index(index).map_err(|e| refused(e.to_string()))?;
    let mut bytes = Vec::new();
    // The entry's declared size is the archive's to state; the read is bounded whatever it says.
    entry
        .take(MAX_ARCHIVE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| refused(e.to_string()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ARCHIVE {
        return Err(refused(format!(
            "its {name} is larger than {MAX_ARCHIVE} bytes"
        )));
    }
    let partial = to.with_extension("partial");
    std::fs::write(&partial, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&partial, to)?;
    Ok(())
}

/// A running ChromeDriver, which the session owns: a session it opened is ended and the process
/// killed when it drops.
pub(crate) struct ChromeDriver {
    /// Ended, with whatever it started, as it drops after the session ended.
    _process: AppProcess,
    port: u16,
    session: Option<String>,
}

impl ChromeDriver {
    /// Start the ChromeDriver at `path` on a free loopback port.
    ///
    /// # Errors
    ///
    /// When it cannot be started, or reports no port within its startup bound.
    pub(crate) fn start(path: &Path) -> Result<Self> {
        let mut command = Command::new(path);
        command
            .arg("--port=0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut process = AppProcess::spawn(command, None)?;
        let stdout = process.take_stdout().ok_or_else(|| Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: "ChromeDriver's output is not piped".to_owned(),
        })?;
        let (port, reported) = mpsc::sync_channel::<u16>(1);
        // The rest of its output is read and dropped, so it never blocks on a full pipe.
        std::thread::Builder::new()
            .name("flui-driver-chromedriver".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut port = Some(port);
                let mut line = String::new();
                loop {
                    line.clear();
                    match (&mut reader).take(MAX_OUTPUT_LINE).read_line(&mut line) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {}
                    }
                    if let Some(found) = reported_port(&line)
                        && let Some(port) = port.take()
                    {
                        let _ = port.try_send(found);
                    }
                }
            })?;
        let port = reported.recv_timeout(STARTUP).map_err(|_| Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: format!(
                "ChromeDriver at {} reported no port within {} s",
                path.display(),
                STARTUP.as_secs()
            ),
        })?;
        Ok(Self {
            _process: process,
            port,
            session: None,
        })
    }

    /// Open a WebDriver session in headless Chrome (`chrome`'s binary when given, else the one
    /// ChromeDriver finds) with a `width` by `height` window at a device pixel ratio of one, and
    /// answer its BiDi WebSocket's URL.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] marked as a host that cannot run the lane when ChromeDriver cannot start
    /// Chrome (a version it does not drive, no Chrome); the exchange's failure.
    pub(crate) fn open_session(
        &mut self,
        chrome: Option<&Chrome>,
        (width, height): (u32, u32),
    ) -> Result<String> {
        let mut options = json!({ "args": chrome_args(width, height) });
        if let Some(chrome) = chrome {
            options["binary"] = Value::from(chrome.binary.to_string_lossy().into_owned());
        }
        let capabilities = json!({
            "capabilities": {
                "alwaysMatch": {
                    "browserName": "chrome",
                    "webSocketUrl": true,
                    "goog:chromeOptions": options,
                },
            },
        });
        let answer = self.post("/session", &capabilities)?;
        let value = answer.get("value").unwrap_or(&Value::Null);
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            let message = value.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(Error::Lane {
                lane: "chrome".to_owned(),
                unavailable: error == "session not created",
                message: format!("ChromeDriver opened no session: {error}: {message}"),
            });
        }
        let id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| Self::malformed("names no session"))?;
        self.session = Some(id.to_owned());
        value
            .pointer("/capabilities/webSocketUrl")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Self::malformed("offers no BiDi WebSocket"))
    }

    fn malformed(what: &str) -> Error {
        Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: format!("ChromeDriver's new session {what}"),
        }
    }

    /// `body` posted to ChromeDriver's `path`, answered as JSON within [`MAX_ANSWER`] bytes.
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let failed = |why: String| Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: format!("ChromeDriver did not answer POST {path}: {why}"),
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(STARTUP))
            .build()
            .new_agent();
        let text = agent
            .post(&url)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|error| failed(error.to_string()))?
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER)
            .read_to_string()
            .map_err(|error| failed(error.to_string()))?;
        serde_json::from_str::<Value>(&text).map_err(|error| failed(error.to_string()))
    }
}

impl Drop for ChromeDriver {
    fn drop(&mut self) {
        // Chrome quits with its session; ChromeDriver, and whatever it left, end with the process,
        // which `AppProcess` ends as it drops after this.
        if let Some(session) = self.session.take() {
            let url = format!("http://127.0.0.1:{}/session/{session}", self.port);
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(END_TIMEOUT))
                .build()
                .new_agent();
            let _ = agent.delete(&url).call();
        }
    }
}

/// The port in ChromeDriver's startup line, `… started successfully on port 9515.`.
#[must_use]
pub fn reported_port(line: &str) -> Option<u16> {
    let (_, rest) = line.split_once("started successfully on port ")?;
    rest.trim().trim_end_matches('.').parse::<u16>().ok()
}

/// The arguments the session's Chrome starts with: always headless, so no window of it reaches
/// the desktop; a `width` by `height` window at a device pixel ratio of one; WebGL on the
/// software rasterizer where the host has no GPU, which Chrome no longer falls back to unasked;
/// and without its sandbox only when run as root, which Chrome refuses otherwise.
#[must_use]
pub fn chrome_args(width: u32, height: u32) -> Vec<String> {
    let mut args = vec![
        "--headless=new".to_owned(),
        format!("--window-size={width},{height}"),
        "--force-device-scale-factor=1".to_owned(),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        "--enable-unsafe-swiftshader".to_owned(),
    ];
    if runs_as_root() {
        args.push("--no-sandbox".to_owned());
    }
    args
}

#[cfg(unix)]
fn runs_as_root() -> bool {
    rustix::process::geteuid().is_root()
}

#[cfg(not(unix))]
fn runs_as_root() -> bool {
    false
}
