//! Starting a browser with its remote protocol on, in a profile of its own, and linking to it.
//!
//! The browser picks a free port (`--remote-debugging-port=0`) and prints the WebSocket it
//! listens on to its stderr, which is read until that line comes and drained afterwards, so the
//! browser never blocks on a full pipe. Chromium also writes the port to `DevToolsActivePort`
//! in its profile, read as well in case its stderr is not a console's.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::bidi::Bidi;
use super::cdp::Cdp;
use super::socket::Socket;
use super::{BrowserLink, Dialect, LinkError};

/// How long the browser has to start listening.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long connecting to the browser's WebSocket may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long removing the profile is tried while the browser's helpers still write into it.
const PROFILE_REMOVAL: Duration = Duration::from_secs(5);

/// Why a browser could not be started and linked.
#[derive(Debug)]
pub(crate) enum LaunchError {
    /// The browser could not be started, or its profile not made.
    Start(std::io::Error),
    /// The browser exited, or never said where it listens.
    Silent(String),
    /// The browser's protocol failed.
    Link(LinkError),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) => write!(f, "could not start the browser: {error}"),
            Self::Silent(why) => write!(f, "the browser opened no remote protocol: {why}"),
            Self::Link(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for LaunchError {}

/// A browser this command started, in a profile of its own that goes with it. Dropping it
/// ends the browser.
pub(crate) struct LaunchedBrowser {
    child: Child,
    profile: tempfile::TempDir,
}

impl LaunchedBrowser {
    /// Starts the browser at `executable`, which speaks `dialect`, on a blank tab (without a
    /// window when `headless`), and links to that tab.
    pub(crate) fn start(
        executable: &Path,
        dialect: Dialect,
        headless: bool,
    ) -> Result<(Self, Box<dyn BrowserLink>), LaunchError> {
        let profile = tempfile::Builder::new()
            .prefix("flui-browser-")
            .tempdir()
            .map_err(LaunchError::Start)?;
        let mut command = Command::new(executable);
        command.args(arguments(dialect, profile.path(), headless));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(LaunchError::Start)?;
        let mut browser = Self { child, profile };
        let stderr = browser.child.stderr.take();
        let (listening, said) = mpsc::channel::<String>();
        if let Some(stderr) = stderr {
            thread::Builder::new()
                .name("flui-browser-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines() {
                        let Ok(line) = line else { break };
                        if let Some(url) = listening_url(dialect, &line) {
                            let _ = listening.send(url);
                        }
                    }
                })
                .map_err(LaunchError::Start)?;
        }
        let url = browser.wait_for_url(dialect, &said)?;
        let socket = Socket::connect(&url, CONNECT_TIMEOUT).map_err(LaunchError::Link)?;
        let link: Box<dyn BrowserLink> = match dialect {
            Dialect::Bidi => Box::new(Bidi::attach(socket).map_err(LaunchError::Link)?),
            Dialect::Cdp => Box::new(Cdp::attach(socket).map_err(LaunchError::Link)?),
        };
        Ok((browser, link))
    }

    /// The profile the browser runs in, gone once the browser is dropped.
    #[cfg(test)]
    pub(crate) fn profile(&self) -> &Path {
        self.profile.path()
    }

    /// The WebSocket the browser says it listens on.
    fn wait_for_url(
        &mut self,
        dialect: Dialect,
        said: &mpsc::Receiver<String>,
    ) -> Result<String, LaunchError> {
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match said.recv_timeout(Duration::from_millis(50)) {
                Ok(url) => return Ok(url),
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
            }
            if dialect == Dialect::Cdp
                && let Some(url) = active_port(self.profile.path())
            {
                return Ok(url);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(LaunchError::Silent(format!("it exited ({status})")));
            }
            if Instant::now() >= deadline {
                return Err(LaunchError::Silent(format!(
                    "it named no WebSocket within {} s",
                    START_TIMEOUT.as_secs()
                )));
            }
        }
    }
}

impl Drop for LaunchedBrowser {
    fn drop(&mut self) {
        // Ended before its profile goes, which a running browser holds.
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The browser's helper processes outlive it for a moment and write into the profile
        // as they go, which fails a removal that races them: it is tried again until they
        // are gone.
        let deadline = Instant::now() + PROFILE_REMOVAL;
        loop {
            match std::fs::remove_dir_all(self.profile.path()) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if Instant::now() >= deadline => break,
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

/// The browser's command line: its remote protocol on a port it picks, a profile of its own
/// and no first-run pages, on a blank tab.
fn arguments(dialect: Dialect, profile: &Path, headless: bool) -> Vec<String> {
    let mut arguments = vec!["--remote-debugging-port=0".to_owned()];
    match dialect {
        Dialect::Cdp => {
            arguments.extend([
                format!("--user-data-dir={}", profile.display()),
                "--no-first-run".to_owned(),
                "--no-default-browser-check".to_owned(),
                "--disable-search-engine-choice-screen".to_owned(),
            ]);
            if headless {
                arguments.push("--headless=new".to_owned());
            }
            // Chromium refuses to start its sandbox as root (a container's user, often).
            #[cfg(target_os = "linux")]
            if rustix::process::geteuid().is_root() {
                arguments.push("--no-sandbox".to_owned());
            }
        }
        Dialect::Bidi => {
            arguments.extend([
                "--profile".to_owned(),
                profile.display().to_string(),
                "--no-remote".to_owned(),
                "--new-instance".to_owned(),
            ]);
            if headless {
                arguments.push("--headless".to_owned());
            }
        }
    }
    arguments.push("about:blank".to_owned());
    arguments
}

/// The WebSocket a line of the browser's stderr names.
fn listening_url(dialect: Dialect, line: &str) -> Option<String> {
    match dialect {
        Dialect::Cdp => line
            .trim()
            .strip_prefix("DevTools listening on ")
            .map(|url| url.trim().to_owned()),
        // Firefox names its host and port; its BiDi sessions are at `/session`.
        Dialect::Bidi => line
            .trim()
            .strip_prefix("WebDriver BiDi listening on ")
            .map(|url| format!("{}/session", url.trim().trim_end_matches('/'))),
    }
}

/// The WebSocket Chromium wrote to its profile's `DevToolsActivePort`: the port, then the
/// browser's path.
fn active_port(profile: &Path) -> Option<String> {
    let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    let mut lines = text.lines();
    let port = lines.next()?.trim().parse::<u16>().ok()?;
    let path = lines.next()?.trim();
    path.starts_with('/')
        .then(|| format!("ws://127.0.0.1:{port}{path}"))
}

/// The program a browser `flui devices` found is started as: on macOS the executable inside
/// its `.app` bundle, elsewhere the path itself.
pub(crate) fn executable(launch: &Path) -> std::io::Result<PathBuf> {
    if launch
        .extension()
        .is_some_and(|extension| extension == "app")
    {
        let output = crate::proc::output_with_timeout(
            Command::new("defaults")
                .arg("read")
                .arg(launch.join("Contents/Info"))
                .arg("CFBundleExecutable"),
            Duration::from_secs(5),
        )?;
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if name.is_empty() {
            return Err(std::io::Error::other(format!(
                "{} names no executable",
                launch.display()
            )));
        }
        return Ok(launch.join("Contents/MacOS").join(name));
    }
    Ok(launch.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_browser_is_started_on_a_port_it_picks_and_says_where_it_listens() {
        let profile = Path::new("/tmp/profile");
        let chromium = arguments(Dialect::Cdp, profile, true);
        assert_eq!(chromium[0], "--remote-debugging-port=0");
        assert!(chromium.contains(&"--user-data-dir=/tmp/profile".to_owned()));
        assert!(chromium.contains(&"--headless=new".to_owned()));
        let firefox = arguments(Dialect::Bidi, profile, false);
        assert!(
            firefox
                .windows(2)
                .any(|pair| pair == ["--profile", "/tmp/profile"])
        );
        assert!(!firefox.contains(&"--headless".to_owned()));
        assert_eq!(firefox.last().map(String::as_str), Some("about:blank"));

        assert_eq!(
            listening_url(
                Dialect::Cdp,
                "DevTools listening on ws://127.0.0.1:41234/devtools/browser/0a-1b\r"
            )
            .as_deref(),
            Some("ws://127.0.0.1:41234/devtools/browser/0a-1b")
        );
        assert_eq!(
            listening_url(
                Dialect::Bidi,
                "WebDriver BiDi listening on ws://127.0.0.1:9222"
            )
            .as_deref(),
            Some("ws://127.0.0.1:9222/session")
        );
        assert_eq!(
            listening_url(Dialect::Bidi, "DevTools listening on ws://x"),
            None
        );
    }

    #[test]
    fn chromium_names_its_socket_in_its_profile_too() {
        let profile = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(active_port(profile.path()), None);
        std::fs::write(
            profile.path().join("DevToolsActivePort"),
            "41234\n/devtools/browser/0a-1b\n",
        )
        .expect("the port file is written");
        assert_eq!(
            active_port(profile.path()).as_deref(),
            Some("ws://127.0.0.1:41234/devtools/browser/0a-1b")
        );
    }

    #[test]
    fn the_profile_goes_with_the_browser_while_its_helpers_still_write_into_it() {
        let profile = tempfile::tempdir().expect("a temporary directory");
        let path = profile.path().to_path_buf();
        // The test's own binary stands in for the browser; a thread for its helpers, which
        // keep writing into the profile after it ended.
        let child = Command::new(std::env::current_exe().expect("the test's own binary"))
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .expect("the stand-in starts");
        let (writing, started) = mpsc::channel::<()>();
        let helpers = {
            let path = path.clone();
            thread::spawn(move || {
                let until = Instant::now() + Duration::from_millis(200);
                let mut written = 0_u32;
                while written < 64 || Instant::now() < until {
                    let _ = std::fs::write(path.join(written.to_string()), b"state");
                    written = written.wrapping_add(1);
                    if written == 64 {
                        let _ = writing.send(());
                    }
                }
            })
        };
        started.recv().expect("the helpers write into the profile");
        drop(LaunchedBrowser { child, profile });
        helpers.join().expect("the helpers end");
        assert!(!path.exists(), "the profile outlived its browser");
    }
}
