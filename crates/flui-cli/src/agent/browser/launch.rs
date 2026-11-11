//! Starting a browser with its remote protocol on, in a profile of its own, and linking to it.
//!
//! The browser picks a free port (`--remote-debugging-port=0`) and prints the WebSocket it
//! listens on to its stderr, which is read until that line comes and drained afterwards, so the
//! browser never blocks on a full pipe. Chromium also writes the port to `DevToolsActivePort`
//! in its profile, read as well in case its stderr is not a console's.

use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::bidi::Bidi;
use super::cdp::Cdp;
use super::socket::Socket;
use super::{BrowserLink, Dialect, LinkError};

/// How long the browser has to start: to say where it listens and answer the link's first
/// commands. A cold start in a fresh profile on a busy machine takes tens of seconds, and
/// Firefox answers `session.new` only once its first window is up.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// How long connecting to the browser's WebSocket may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long removing the profile is tried while the browser's helpers still write into it.
const PROFILE_REMOVAL: Duration = Duration::from_secs(5);

/// What Firefox reads from a fresh profile before it starts. Its remote agent applies its own
/// recommended preferences only once it runs, after the first window's startup work, so the
/// work a first start does in a new profile (the welcome and default-browser pages, the
/// update and telemetry checks, the captive-portal probe) is turned off before it, as a
/// WebDriver client's profile does. The agent logs at `Debug`, which a failed start reports.
const FIREFOX_PREFERENCES: &str = r#"user_pref("remote.log.level", "Debug");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.startup.page", 0);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("startup.homepage_welcome_url", "about:blank");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("app.update.disabledForTesting", true);
user_pref("app.normandy.enabled", false);
user_pref("network.captive-portal-service.enabled", false);
user_pref("browser.sessionstore.resume_from_crash", false);
user_pref("toolkit.startup.max_resumed_crashes", -1);
"#;

/// How long a browser that exited has for its stderr to be read to the end.
const STDERR_DRAIN: Duration = Duration::from_secs(1);

/// How many of the browser's last stderr lines a failed start reports.
const STDERR_TAIL: usize = 64;

/// The longest stderr line a failed start reports, in bytes; the rest of a longer one is cut.
const STDERR_LINE: usize = 512;

/// Why a browser could not be started and linked. A failure after the browser started carries
/// the browser's last lines of stderr, which say why it did not get further.
#[derive(Debug)]
pub(crate) enum LaunchError {
    /// The browser could not be started, or its profile not made.
    Start(std::io::Error),
    /// The browser exited, or never said where it listens.
    Silent { why: String, stderr: Vec<String> },
    /// The browser said where it listens but did not answer the link's `command` before the
    /// start's deadline.
    NotReady {
        command: &'static str,
        stderr: Vec<String>,
    },
    /// The browser's protocol failed.
    Link {
        error: LinkError,
        stderr: Vec<String>,
    },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stderr = match self {
            Self::Start(error) => return write!(f, "could not start the browser: {error}"),
            Self::Silent { why, stderr } => {
                write!(f, "the browser opened no remote protocol: {why}")?;
                stderr
            }
            Self::NotReady { command, stderr } => {
                write!(
                    f,
                    "the browser did not answer `{command}` within {} s of starting",
                    START_TIMEOUT.as_secs()
                )?;
                stderr
            }
            Self::Link { error, stderr } => {
                error.fmt(f)?;
                stderr
            }
        };
        if stderr.is_empty() {
            return write!(f, "; it wrote nothing to stderr");
        }
        write!(f, "; its last lines of stderr:")?;
        for line in stderr {
            write!(f, "\n  {line}")?;
        }
        Ok(())
    }
}

impl LaunchError {
    /// A failure of the link while the browser starts: a command it did not answer in time
    /// ran out the start's deadline.
    fn linking(failure: AttachError, stderr: Vec<String>) -> Self {
        match failure.error {
            LinkError::Timeout => Self::NotReady {
                command: failure.command,
                stderr,
            },
            error => Self::Link { error, stderr },
        }
    }
}

/// A link that failed while the browser started, with the command it failed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttachError {
    pub(super) command: &'static str,
    pub(super) error: LinkError,
}

impl AttachError {
    /// Names `command` as the one `error` came from.
    pub(super) fn at(command: &'static str) -> impl FnOnce(LinkError) -> Self {
        move |error| Self { command, error }
    }
}

/// The browser's last lines of stderr, kept by the thread that drains it.
#[derive(Clone, Default)]
struct StderrTail(Arc<Mutex<VecDeque<String>>>);

impl StderrTail {
    fn push(&self, mut line: String) {
        if line.len() > STDERR_LINE {
            let mut end = STDERR_LINE;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
            line.push('…');
        }
        // A reader that panicked holding the lock left whole lines; they are still read.
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == STDERR_TAIL {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn lines(&self) -> Vec<String> {
        let lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        lines.iter().cloned().collect()
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
        if dialect == Dialect::Bidi {
            std::fs::write(profile.path().join("user.js"), FIREFOX_PREFERENCES)
                .map_err(LaunchError::Start)?;
        }
        let mut command = Command::new(executable);
        command.args(arguments(dialect, profile.path(), headless));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(LaunchError::Start)?;
        let ready_by = Instant::now() + START_TIMEOUT;
        let mut browser = Self { child, profile };
        let stderr = browser.child.stderr.take();
        let (listening, said) = mpsc::channel::<String>();
        let tail = StderrTail::default();
        if let Some(stderr) = stderr {
            let tail = tail.clone();
            thread::Builder::new()
                .name("flui-browser-stderr".into())
                .spawn(move || {
                    // Read to the end, the link's or not, so the browser never blocks on a
                    // full pipe; a line that is not text is kept as far as it is.
                    let mut reader = BufReader::new(stderr);
                    let mut bytes = Vec::new();
                    while matches!(reader.read_until(b'\n', &mut bytes), Ok(read) if read > 0) {
                        let line = String::from_utf8_lossy(&bytes).trim_end().to_owned();
                        bytes.clear();
                        if let Some(url) = listening_url(dialect, &line) {
                            let _ = listening.send(url);
                        }
                        tail.push(line);
                    }
                })
                .map_err(LaunchError::Start)?;
        }
        let url = browser
            .wait_for_url(dialect, &said, ready_by)
            .map_err(|why| LaunchError::Silent {
                why,
                stderr: tail.lines(),
            })?;
        let socket = Socket::connect(&url, CONNECT_TIMEOUT).map_err(|error| LaunchError::Link {
            error,
            stderr: tail.lines(),
        })?;
        let link = attach(dialect, socket, ready_by)
            .map_err(|failure| LaunchError::linking(failure, tail.lines()))?;
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
        deadline: Instant,
    ) -> Result<String, String> {
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
                // What it wrote before it went is still being read: its reader ends when the
                // pipe closes, which a helper of the browser may hold open a while longer.
                let drained_by = Instant::now() + STDERR_DRAIN;
                while let Err(mpsc::RecvTimeoutError::Timeout) | Ok(_) =
                    said.recv_timeout(drained_by.saturating_duration_since(Instant::now()))
                {
                    if Instant::now() >= drained_by {
                        break;
                    }
                }
                return Err(format!("it exited ({status})"));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "it named no WebSocket within {} s",
                    START_TIMEOUT.as_secs()
                ));
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

/// Links to the browser on `socket`, which speaks `dialect`. The browser is still starting, so
/// each of its answers may take until `ready_by`.
fn attach(
    dialect: Dialect,
    socket: Socket,
    ready_by: Instant,
) -> Result<Box<dyn BrowserLink>, AttachError> {
    Ok(match dialect {
        Dialect::Bidi => Box::new(Bidi::attach(socket, ready_by)?),
        Dialect::Cdp => Box::new(Cdp::attach(socket, ready_by)?),
    })
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

    /// A browser that exits before it names a socket is reported with what it wrote to stderr:
    /// its last lines, each cut to a bounded length.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_does_not_start_is_reported_with_its_stderr() {
        use std::os::unix::fs::PermissionsExt as _;

        let bin = tempfile::tempdir().expect("a temporary directory");
        let browser = bin.path().join("browser");
        let long = "x".repeat(STDERR_LINE * 2);
        let lines = (0..STDERR_TAIL + 8)
            .map(|line| format!("line {line}"))
            .chain([long.clone(), "no profile here".to_owned()]);
        let script: String = std::iter::once("#!/bin/sh\n".to_owned())
            .chain(lines.map(|line| format!("echo '{line}' >&2\n")))
            .chain(["exit 3\n".to_owned()])
            .collect();
        std::fs::write(&browser, script).expect("the stand-in is written");
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in is executable");

        let failure = LaunchedBrowser::start(&browser, Dialect::Bidi, true)
            .map(drop)
            .expect_err("a browser that exits does not start");
        let LaunchError::Silent { why, stderr } = &failure else {
            panic!("an exited browser is silent, not {failure:?}");
        };
        assert!(why.contains("exited"), "{why}");
        assert_eq!(
            stderr.len(),
            STDERR_TAIL,
            "the last lines are kept, no more"
        );
        assert_eq!(
            stderr.first().map(String::as_str),
            Some("line 10"),
            "the earliest lines went first"
        );
        assert_eq!(
            stderr[STDERR_TAIL - 2],
            format!("{}…", &long[..STDERR_LINE]),
            "a long line is cut"
        );
        assert_eq!(
            stderr.last().map(String::as_str),
            Some("no profile here"),
            "what it wrote last, before it exited, is read"
        );
        let text = failure.to_string();
        assert!(
            text.contains("its last lines of stderr:\n  line 10\n"),
            "{text}"
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

#[cfg(test)]
mod linking {
    use std::net::TcpListener;

    use serde_json::{Value, json};
    use tungstenite::Message;

    use super::super::COMMAND_TIMEOUT;
    use super::*;

    /// A browser's WebSocket on the loopback that answers its first command `first_after` it
    /// came (never when `None`), as Firefox answers `session.new` once its window is up, and
    /// every later one at once. Returns its URL.
    fn starting_browser(first_after: Option<Duration>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!(
            "ws://{}/session",
            listener.local_addr().expect("the port's address")
        );
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(mut ws) = tungstenite::accept(stream) else {
                return;
            };
            let mut delay = first_after;
            // Ends once the link goes and its socket with it.
            while let Ok(message) = ws.read() {
                let Message::Text(text) = message else {
                    continue;
                };
                let Some(after) = delay else {
                    continue;
                };
                thread::sleep(after);
                delay = Some(Duration::ZERO);
                let command = serde_json::from_str::<Value>(&text).expect("a command is JSON");
                let result = match command["method"].as_str() {
                    Some("browsingContext.getTree") => {
                        json!({ "contexts": [{ "context": "tab" }] })
                    }
                    Some("Target.getTargets") => {
                        json!({ "targetInfos": [{ "type": "page", "targetId": "tab" }] })
                    }
                    Some("Target.attachToTarget") => json!({ "sessionId": "session" }),
                    _ => json!({}),
                };
                let answer = json!({ "id": command["id"], "type": "success", "result": result });
                if ws.send(Message::text(answer.to_string())).is_err() {
                    return;
                }
            }
        });
        url
    }

    fn link(
        dialect: Dialect,
        first_after: Option<Duration>,
        ready_by: Instant,
    ) -> Result<(), AttachError> {
        let socket = Socket::connect(&starting_browser(first_after), CONNECT_TIMEOUT)
            .expect("the browser's socket is reached");
        attach(dialect, socket, ready_by).map(drop)
    }

    #[test]
    fn a_browser_still_starting_has_until_the_start_deadline_to_answer() {
        let late = COMMAND_TIMEOUT + Duration::from_millis(500);
        thread::scope(|scope| {
            for dialect in [Dialect::Bidi, Dialect::Cdp] {
                scope.spawn(move || {
                    // Slower than a command may take, within the start's deadline.
                    let linked = link(dialect, Some(late), Instant::now() + late * 2);
                    assert!(linked.is_ok(), "{dialect:?}: {linked:?}");

                    // Silent past the start's deadline, which ends the wait however long a
                    // command may take.
                    let start = Instant::now();
                    let silent = link(dialect, None, start + Duration::from_millis(300))
                        .expect_err("a silent browser is not linked");
                    assert!(
                        start.elapsed() < COMMAND_TIMEOUT / 2,
                        "{dialect:?} waited on"
                    );
                    let first = match dialect {
                        Dialect::Bidi => "session.new",
                        Dialect::Cdp => "Target.getTargets",
                    };
                    let said = vec!["still starting".to_owned()];
                    let failure = LaunchError::linking(silent.clone(), said.clone());
                    assert!(
                        matches!(
                            &failure,
                            LaunchError::NotReady { command, stderr }
                                if *command == first && *stderr == said
                        ),
                        "{dialect:?}: {silent:?}"
                    );
                    let text = failure.to_string();
                    assert!(
                        text.contains(&format!("`{first}`"))
                            && text.ends_with("\n  still starting"),
                        "{dialect:?}: {text}"
                    );
                });
            }
        });
    }
}
