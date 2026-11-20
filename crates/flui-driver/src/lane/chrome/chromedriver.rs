//! The installed Chrome, the ChromeDriver that drives it, and the WebDriver session ChromeDriver
//! opens in headless Chrome.
//!
//! ChromeDriver must match Chrome's version, so it is not a tool a host is expected to have:
//! [`resolve`] takes the ChromeDriver the launch names (`ChromeDriverSource::Path`, or the
//! `CHROMEDRIVER` file, or the directory `CHROMEWEBDRIVER` names, which GitHub's runners set),
//! else the one a fetch kept in the cache for the installed Chrome's version, its SHA-256 checked
//! against the one recorded beside it before it runs. Only a launch that allows it
//! (`ChromeDriverSource::FetchAllowed`) fetches on a miss, over HTTPS alone, from the Chrome for
//! Testing archives: the archive of the installed Chrome's version when it is published, else, as
//! not every patch release is, the archive of the newest release of the same build
//! (`major.minor.build`, which speaks the same protocol), kept under the installed version. The
//! record beside it names the binary's SHA-256 and the version fetched. Otherwise a miss is a lane
//! the host cannot run, with the install hint. `FLUI_CHROME` names a Chrome other than the one
//! installed in the usual place.
//!
//! The hash record guards against a kept binary that is torn, truncated or replaced by mistake (an
//! interrupted fetch, a disk fault, another tool's file of the same name): such a binary is never
//! run. It does not guard against someone who can write the cache, who can rewrite the record
//! with the binary; the cache is the user's own directory, as trusted as the user's other tools.
//!
//! The cache is written so that parallel fetches never tear it: each file is staged in a file of
//! its own beside its place and linked there without replacing what another fetch put first; where
//! the filesystem refuses links, the file is created in place, never over another's. A fetch that
//! finds a file put first takes it once it holds the same bytes.
//!
//! What ChromeDriver and the archives answer is untrusted: every body is read within a bound
//! ([`MAX_ARCHIVE`], [`MAX_ANSWER`]), every request within a deadline, and the session's
//! WebSocket must be on the loopback (`bidi`). The session's Chrome is always headless: no window
//! of it reaches the desktop.

use std::fs::OpenOptions;
use std::io::{BufRead as _, BufReader, Cursor, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};
use crate::launch::AppProcess;

/// Where Chrome for Testing publishes its archives, and the newest release of each build.
const ARCHIVES: &str = "https://storage.googleapis.com/chrome-for-testing-public";

/// The root of the newest release of each build: `<root>_<major.minor.build>`.
const LATEST_OF_BUILD: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/LATEST_RELEASE";

/// The name of the record of a kept ChromeDriver's hashes, beside it.
const RECORD: &str = "chromedriver.sha256";

/// What fetches a ChromeDriver: the hosts it asks and the client it asks them with.
pub(crate) struct Fetcher {
    archives: String,
    latest_of_build: String,
    agent: ureq::Agent,
}

impl Fetcher {
    /// Chrome for Testing, over HTTPS alone.
    pub(crate) fn chrome_for_testing() -> Self {
        Self {
            archives: ARCHIVES.to_owned(),
            latest_of_build: LATEST_OF_BUILD.to_owned(),
            agent: agent(true),
        }
    }

    /// Stand-in hosts at `root` on the loopback, over plain HTTP: the tests' archives. Any other
    /// host is refused.
    #[cfg(feature = "testing")]
    pub(crate) fn loopback(root: &str) -> Result<Self> {
        let loopback = ["http://127.0.0.1:", "http://[::1]:", "http://localhost:"]
            .iter()
            .any(|prefix| root.starts_with(prefix));
        if !loopback {
            return Err(Error::invalid_argument(format!(
                "{root} is not on the loopback"
            )));
        }
        Ok(Self {
            archives: format!("{root}/archives"),
            latest_of_build: format!("{root}/latest/LATEST_RELEASE"),
            agent: agent(false),
        })
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

/// The ChromeDriver the environment names: the `CHROMEDRIVER` file, else the ChromeDriver in the
/// directory `CHROMEWEBDRIVER` names (GitHub's runners set that one).
#[must_use]
pub fn named_by_environment() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    var("CHROMEDRIVER")
        .map(PathBuf::from)
        .or_else(|| var("CHROMEWEBDRIVER").map(|dir| PathBuf::from(dir).join(executable_name())))
}

/// The ChromeDriver for `chrome`: `named` when given, else the one kept for its version in
/// `cache` once its hash matches its record, else, when `fetch` is given, the one it fetches into
/// `cache`.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane: no ChromeDriver and no fetch allowed
/// (with the install hint), a kept one whose hash does not match its record, none published for
/// this platform or Chrome's version; the fetch's failure.
pub(crate) fn resolve(
    chrome: &Chrome,
    cache: &Path,
    named: Option<PathBuf>,
    fetch: Option<&Fetcher>,
) -> Result<PathBuf> {
    if let Some(named) = named {
        return Ok(named);
    }
    let dir = cache.join(&chrome.version);
    let kept = dir.join(executable_name());
    if kept.exists() {
        verify_kept(&dir)?;
        return Ok(kept);
    }
    let Some(fetcher) = fetch else {
        return Err(unavailable(format!(
            "no ChromeDriver for Chrome {}: set CHROMEDRIVER to one that matches it, or allow the \
             launch to fetch it (`ChromeDriverSource::FetchAllowed`)",
            chrome.version
        )));
    };
    let platform = platform().ok_or_else(|| {
        unavailable(format!(
            "Chrome for Testing publishes no ChromeDriver for {}/{}; set CHROMEDRIVER",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    fetch_into(fetcher, &chrome.version, platform, &dir)?;
    Ok(kept)
}

/// The archive URL of ChromeDriver `version` for `platform` under `archives`.
#[must_use]
pub fn archive_url(archives: &str, version: &str, platform: &str) -> String {
    format!("{archives}/{version}/{platform}/chromedriver-{platform}.zip")
}

/// Fetch ChromeDriver for Chrome `version` on `platform` into `dir`, beside the record of the
/// binary's hash and the version fetched. The archive of `version` itself is taken when it is
/// published; not every patch release is, and then the archive of the newest release of the same
/// build (`major.minor.build`, which speaks the same protocol) is taken, kept under `version`.
fn fetch_into(fetcher: &Fetcher, version: &str, platform: &str, dir: &Path) -> Result<()> {
    let agent = &fetcher.agent;
    let (fetched, archive) = match get(
        agent,
        &archive_url(&fetcher.archives, version, platform),
        MAX_ARCHIVE,
    ) {
        Ok(archive) => (version.to_owned(), archive),
        Err(Fetched::NotFound) => {
            let build = version.rsplit_once('.').map_or(version, |(build, _)| build);
            let latest = get(agent, &format!("{}_{build}", fetcher.latest_of_build), 64)
                .map_err(|failure| fetch_failed(version, &failure.to_string()))?;
            let latest = String::from_utf8_lossy(&latest).trim().to_owned();
            let same_build = latest
                .rsplit_once('.')
                .is_some_and(|(latest_build, _)| latest_build == build);
            if !is_version(&latest) || !same_build {
                return Err(fetch_failed(
                    version,
                    &format!("the newest release of build {build} is named {latest:?}"),
                ));
            }
            let archive = get(
                agent,
                &archive_url(&fetcher.archives, &latest, platform),
                MAX_ARCHIVE,
            )
            .map_err(|failure| fetch_failed(version, &failure.to_string()))?;
            (latest, archive)
        }
        Err(failure) => return Err(fetch_failed(version, &failure.to_string())),
    };
    let binary = extract(&archive)?;
    let record = format!("version {fetched}\nbinary {}\n", sha256_hex(&binary));
    let unusable = |error: std::io::Error| {
        unavailable(format!(
            "the ChromeDriver cache at {} is unusable: {error}; set CHROMEDRIVER",
            dir.display()
        ))
    };
    std::fs::create_dir_all(dir).map_err(unusable)?;
    // The record first: a binary is taken only beside a record that names its hash.
    place(dir, RECORD, record.as_bytes(), false, hard_link)?;
    place(dir, executable_name(), &binary, true, hard_link)?;
    verify_kept(dir)
}

fn fetch_failed(version: &str, why: &str) -> Error {
    unavailable(format!(
        "no ChromeDriver for Chrome {version}: {why}; set CHROMEDRIVER to one that matches"
    ))
}

fn unavailable(message: String) -> Error {
    Error::Lane {
        lane: "chrome".to_owned(),
        unavailable: true,
        message,
    }
}

/// The lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("BUG: writing to a String cannot fail");
            hex
        })
}

/// Check the ChromeDriver kept in `dir` against its record before it runs.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane when the record is missing or
/// malformed, or the binary's hash is not the one it names: a file torn, truncated or replaced
/// is never run.
pub fn verify_kept(dir: &Path) -> Result<()> {
    let refused = |why: &str| {
        unavailable(format!(
            "the ChromeDriver kept in {} is refused: {why}; delete the directory",
            dir.display()
        ))
    };
    let mut record = String::new();
    std::fs::File::open(dir.join(RECORD))
        .map_err(|_| refused("its hash record is missing"))?
        .take(1024)
        .read_to_string(&mut record)
        .map_err(|_| refused("its hash record does not read"))?;
    let expected = record
        .lines()
        .find_map(|line| line.strip_prefix("binary "))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| refused("its hash record names no binary hash"))?;
    let binary = read_bounded(&dir.join(executable_name()))
        .map_err(|_| refused("it does not read whole"))?;
    if sha256_hex(&binary) != expected {
        return Err(refused("its SHA-256 is not the one recorded"));
    }
    Ok(())
}

/// The file at `path`, at most [`MAX_ARCHIVE`] bytes.
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_ARCHIVE + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ARCHIVE {
        return Err(std::io::Error::other("larger than a ChromeDriver"));
    }
    Ok(bytes)
}

/// How a staged file is put in place: a hard link, which never replaces what is there.
pub(crate) type Link = fn(&Path, &Path) -> std::io::Result<()>;

fn hard_link(staged: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::hard_link(staged, target)
}

/// How long a fetch waits for the file another fetch is writing in place, where links are refused.
const WRITER_WAIT: Duration = Duration::from_secs(2);

/// Put `bytes` at `dir/name` without replacing what is there: staged in a file of its own beside
/// it, then linked into place with `link`. Where the filesystem refuses links (FAT32, exFAT, some
/// SMB and FUSE mounts), the file is created in place instead (`create_new`). When another fetch
/// put the file first, it is taken only once it holds the same bytes.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane when the cache cannot be written, or
/// the file another fetch put there differs.
pub(crate) fn place(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    executable: bool,
    link: Link,
) -> Result<()> {
    let target = dir.join(name);
    let unusable = |error: std::io::Error| {
        unavailable(format!(
            "the ChromeDriver cache at {} is unusable: {error}; set CHROMEDRIVER",
            dir.display()
        ))
    };
    let staged = dir.join(format!(
        ".{name}.{}-{}.partial",
        std::process::id(),
        crate::launch::random_hex(8)?
    ));
    let written = write_new(&staged, bytes, executable);
    let linked = written.and_then(|()| link(&staged, &target));
    let _ = std::fs::remove_file(&staged);
    let placed = match linked {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
            // No link here: the file is written in place, never over another fetch's.
            write_new(&target, bytes, executable)
        }
        other => other,
    };
    match placed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            same_bytes(&target, bytes, dir)
        }
        Err(error) => Err(unusable(error)),
    }
}

/// Write `bytes` to a new file at `path` and sync it, executable when asked; an error when a file
/// is there already.
fn write_new(path: &Path, bytes: &[u8], executable: bool) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// Whether the file another fetch put at `target` holds `bytes`, waiting up to [`WRITER_WAIT`]
/// for one still being written in place.
fn same_bytes(target: &Path, bytes: &[u8], dir: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + WRITER_WAIT;
    loop {
        if read_bounded(target).is_ok_and(|there| there == bytes) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(unavailable(format!(
                "{} differs from what this fetch wrote; delete {}",
                target.display(),
                dir.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The HTTP client of a fetch: rustls with the platform's verifier, HTTPS alone unless `https`
/// is false (the tests' loopback stand-in), no redirect, a deadline over each exchange.
fn agent(https: bool) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(https)
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

/// The archive's ChromeDriver executable.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane for an archive that does not read,
/// holds no ChromeDriver, or whose ChromeDriver is past [`MAX_ARCHIVE`] unpacked.
pub fn extract(archive: &[u8]) -> Result<Vec<u8>> {
    let refused = |why: String| unavailable(format!("the ChromeDriver archive is refused: {why}"));
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
    Ok(bytes)
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
    // Port 0 is no port a client can reach: ChromeDriver reports the one it bound.
    rest.trim()
        .trim_end_matches('.')
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
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
