//! The lane's one door to the Simulator: `xcrun`, every command run with a deadline and a bound on
//! what it may answer.
//!
//! - **Commands** ([`XcrunCommand`]) run to completion through [`Xcrun::run`]: one that outlives
//!   its deadline is killed and answers [`Error::Timeout`] with a `may_have_run` effect, since the
//!   Simulator may have done it; one that prints more than its bound is killed and fails, and
//!   nothing past the bound is kept. Every argument is its own word: no shell parses them, and a
//!   value from outside the lane is checked before it becomes one ([`super::device`]).
//! - **Screenshots** ([`Xcrun::screenshot`]): `simctl io <udid> screenshot` writes a PNG file,
//!   which is read only once its length is within
//!   [`MAX_SCREENSHOT_BYTES`].
//! - **Launch values** go to the app as `SIMCTL_CHILD_` variables in `simctl launch`'s own
//!   environment ([`XcrunCommand::env`]), never on its command line, and never into a message: a
//!   command's [`Display`](std::fmt::Display) names their keys alone.
//!
//! [`HostXcrun`] runs the host's `xcrun`; a test scripts the Simulator with
//! `testing::ScriptedXcrun`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::ErrorCode;

use super::LANE;
use super::device::Udid;
#[cfg(unix)]
use super::screenshot::MAX_SCREENSHOT_BYTES;
use crate::error::{Error, Result};
use crate::lane::tool;

/// The prefix `simctl launch` strips from a variable of its own environment to hand the rest to
/// the app it starts.
pub const CHILD_PREFIX: &str = "SIMCTL_CHILD_";

/// One `xcrun` command. Its [`Debug`] form, like its [`Display`](std::fmt::Display) form, names a
/// launch value's key and never its value.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XcrunCommand {
    /// `xcrun simctl list devices --json`: every simulator, by runtime.
    ListDevices,
    /// `xcrun xcodebuild -version`: the selected Xcode's version and build.
    XcodeVersion,
    /// `xcrun simctl install <udid> <app>`: install the `.app` bundle at `app`.
    Install {
        /// The simulator.
        udid: Udid,
        /// The `.app` bundle.
        app: PathBuf,
    },
    /// `xcrun simctl launch --terminate-running-process <udid> <bundle>`, the app's launch values
    /// `env` in `simctl`'s environment, each key with [`CHILD_PREFIX`] before it.
    Launch {
        /// The simulator.
        udid: Udid,
        /// The app's bundle identifier.
        bundle: String,
        /// The launch values, by the variable the app reads (`FLUI_AGENT_ENDPOINT`).
        env: Vec<(String, String)>,
    },
    /// `xcrun simctl terminate <udid> <bundle>`.
    Terminate {
        /// The simulator.
        udid: Udid,
        /// The app's bundle identifier.
        bundle: String,
    },
    /// `xcrun simctl io <udid> screenshot --type=png <path>`.
    Screenshot {
        /// The simulator.
        udid: Udid,
        /// Where the PNG is written.
        path: PathBuf,
    },
}

impl XcrunCommand {
    /// The arguments after `xcrun`, each its own word: a path is passed as it is, never through
    /// UTF-8, and a relative one that starts with `-` as `./` and it, so no tool reads it as an
    /// option.
    #[must_use]
    pub fn args(&self) -> Vec<OsString> {
        let words = |words: &[&str]| -> Vec<OsString> {
            words.iter().map(|word| OsString::from(*word)).collect()
        };
        let mut args = match self {
            Self::ListDevices => words(&["simctl", "list", "devices", "--json"]),
            Self::XcodeVersion => words(&["xcodebuild", "-version"]),
            Self::Install { udid, .. } => words(&["simctl", "install", udid.as_str()]),
            Self::Launch { udid, bundle, .. } => words(&[
                "simctl",
                "launch",
                "--terminate-running-process",
                udid.as_str(),
                bundle,
            ]),
            Self::Terminate { udid, bundle } => {
                words(&["simctl", "terminate", udid.as_str(), bundle])
            }
            Self::Screenshot { udid, .. } => {
                words(&["simctl", "io", udid.as_str(), "screenshot", "--type=png"])
            }
        };
        if let Self::Install { app: path, .. } | Self::Screenshot { path, .. } = self {
            args.push(path_argument(path));
        }
        args
    }

    /// The variables `simctl` runs with: each launch value's key with [`CHILD_PREFIX`] before it.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        match self {
            Self::Launch { env, .. } => env
                .iter()
                .map(|(key, value)| (format!("{CHILD_PREFIX}{key}"), value.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// `path` as one argument: unchanged, or `./` and it when it is relative and starts with `-`.
fn path_argument(path: &Path) -> OsString {
    let option_like = path.as_os_str().as_encoded_bytes().first() == Some(&b'-');
    if path.is_relative() && option_like {
        Path::new(".").join(path).into_os_string()
    } else {
        path.as_os_str().to_owned()
    }
}

impl std::fmt::Display for XcrunCommand {
    /// The arguments, and the keys of the launch values alone: a token never reaches a message.
    /// A path that is not UTF-8 is shown with its other bytes replaced; the argument itself is
    /// passed as it is.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let words: Vec<String> = self
            .args()
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        f.write_str(&words.join(" "))?;
        if let Self::Launch { env, .. } = self
            && !env.is_empty()
        {
            let keys: Vec<&str> = env.iter().map(|(key, _)| key.as_str()).collect();
            write!(f, " (with {})", keys.join(", "))?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for XcrunCommand {
    /// The command's text, as [`Display`](std::fmt::Display) gives it: each launch value's key
    /// and never its value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XcrunCommand({self})")
    }
}

/// What a command answered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct XcrunReply {
    /// Its exit code; `None` when it ended without one.
    pub exit: Option<i32>,
    /// What it printed, at most the bound it ran with.
    pub stdout: Vec<u8>,
    /// What it printed as errors, at most 64 KiB.
    pub stderr: String,
}

impl XcrunReply {
    /// A command that exited with `exit` having printed `stdout`.
    #[must_use]
    pub fn new(exit: i32, stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            exit: Some(exit),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// The reply, having also printed `stderr` as errors.
    #[must_use]
    pub fn stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = stderr.into();
        self
    }

    /// What it printed, as text; bytes that are not UTF-8 replaced.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Why the command failed, when it did: an exit code other than 0, with what it printed as
    /// errors. `simctl` and `xcodebuild` report their failures through the exit code.
    #[must_use]
    pub fn failure(&self) -> Option<String> {
        let said = self.stderr.trim();
        let said = if said.is_empty() {
            String::new()
        } else {
            format!(": {}", cut(said))
        };
        match self.exit {
            Some(0) => None,
            Some(code) => Some(format!("it exited with {code}{said}")),
            None => Some(format!("it ended without an exit code{said}")),
        }
    }
}

/// `text` cut to what a message carries.
fn cut(text: &str) -> String {
    const MESSAGE: usize = 300;
    match text.char_indices().nth(MESSAGE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// The Simulator, through `xcrun`: see the [module docs](self).
pub trait Xcrun: Send {
    /// Run `command` to its end within `timeout`, keeping at most `limit` bytes of its output.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] with a `may_have_run` effect past `timeout`; [`Error::Lane`] when `xcrun`
    /// cannot be started (unavailable), or the output passes `limit`.
    fn run(
        &mut self,
        command: &XcrunCommand,
        limit: usize,
        timeout: Duration,
    ) -> Result<XcrunReply>;

    /// The PNG bytes of the screen of the simulator `udid`, at most
    /// [`MAX_SCREENSHOT_BYTES`].
    ///
    /// # Errors
    ///
    /// As [`Self::run`]; [`Error::Lane`] when the capture fails or its file is past the bound.
    fn screenshot(&mut self, udid: &Udid, timeout: Duration) -> Result<Vec<u8>>;
}

/// A lane failure the lane can still be run past.
pub(crate) fn lane_error(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}

/// A failure that means the host cannot run the lane at all: no Xcode, no such simulator.
pub(crate) fn unavailable(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: true,
        message: message.into(),
    }
}

/// The error of `command`, which answered `reply` with `failure`: a host with no Xcode, or a
/// simulator gone or shut down, is unavailable, which a report makes cannot-verify.
pub(crate) fn command_failed(command: &XcrunCommand, reply: &XcrunReply, failure: &str) -> Error {
    let gone = [
        "xcrun: error",
        "Invalid device",
        "Unable to lookup in current state: Shutdown",
        "No devices are booted",
        "CoreSimulatorService connection",
    ];
    let host_cannot = gone.iter().any(|said| reply.stderr.contains(said));
    let message = format!("`xcrun {command}` failed: {failure}");
    if host_cannot {
        unavailable(message)
    } else {
        lane_error(message)
    }
}

/// `xcrun` on this host. It runs with the driver's environment (a test's stand-in one under the
/// test kit) less every `SIMCTL_CHILD_` variable, so the only values `simctl launch` hands an app
/// are the lane's own. Its [`Debug`] form names the variables' keys, never their values.
#[derive(Clone)]
pub struct HostXcrun {
    program: PathBuf,
    environment: Option<Vec<(OsString, OsString)>>,
}

impl Default for HostXcrun {
    fn default() -> Self {
        Self::new()
    }
}

impl HostXcrun {
    /// `xcrun` on the `PATH`, which runs the tools of the Xcode `xcode-select` chose.
    #[must_use]
    pub fn new() -> Self {
        Self::at("xcrun")
    }

    /// The `xcrun` at `program`.
    #[must_use]
    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            environment: None,
        }
    }

    /// Run `xcrun` with `variables` in place of the driver's environment, its `SIMCTL_CHILD_`
    /// variables still left out: a test's stand-in environment (`testing::host_xcrun_with`).
    #[cfg_attr(
        not(all(feature = "testing", any(unix, windows))),
        expect(
            dead_code,
            reason = "only the test kit runs xcrun in an environment of its own"
        )
    )]
    #[must_use]
    pub(crate) fn environment(
        mut self,
        variables: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        self.environment = Some(variables.into_iter().collect());
        self
    }

    /// The environment `xcrun` starts from: the driver's, or the one named, less every variable
    /// `simctl launch` would hand an app.
    fn inherited(&self) -> Vec<(OsString, OsString)> {
        let variables: Vec<(OsString, OsString)> = match &self.environment {
            Some(variables) => variables.clone(),
            None => std::env::vars_os().collect(),
        };
        variables
            .into_iter()
            .filter(|(key, _)| !key.as_encoded_bytes().starts_with(CHILD_PREFIX.as_bytes()))
            .collect()
    }

    fn started(&self, error: &std::io::Error) -> Error {
        unavailable(format!(
            "could not start `{}`: {error}; the iOS Simulator lane runs on a Mac with Xcode",
            self.program.display()
        ))
    }
}

impl std::fmt::Debug for HostXcrun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Option<Vec<std::borrow::Cow<'_, str>>> =
            self.environment.as_ref().map(|variables| {
                variables
                    .iter()
                    .map(|(key, _)| key.to_string_lossy())
                    .collect()
            });
        f.debug_struct("HostXcrun")
            .field("program", &self.program)
            .field("environment_keys", &keys)
            .finish()
    }
}

impl Xcrun for HostXcrun {
    fn run(
        &mut self,
        command: &XcrunCommand,
        limit: usize,
        timeout: Duration,
    ) -> Result<XcrunReply> {
        let mut process = Command::new(&self.program);
        process
            .args(command.args())
            .env_clear()
            .envs(self.inherited())
            .envs(command.env());
        let finished =
            tool::run(process, limit, timeout, "xcrun").map_err(|failure| match failure {
                tool::Failure::Start(error) => self.started(&error),
                tool::Failure::Io(error) => Error::Io(error),
                tool::Failure::Timeout => Error::Timeout {
                    effect: Some(Effect::new(EffectKind::MayHaveRun).detail(format!(
                        "`xcrun {command}` was ended after {} ms; the Simulator may have done it",
                        timeout.as_millis()
                    ))),
                    message: format!(
                        "`xcrun {command}` did not end within {} ms",
                        timeout.as_millis()
                    ),
                    retry: ErrorCode::Timeout.default_retry(),
                },
                tool::Failure::OverLimit => lane_error(format!(
                    "`xcrun {command}` printed more than {limit} bytes; nothing past them was read"
                )),
                tool::Failure::OutputLeftOpen => {
                    lane_error(format!("`xcrun {command}` left its output open"))
                }
            })?;
        Ok(XcrunReply {
            exit: finished.exit,
            stdout: finished.stdout,
            stderr: finished.stderr,
        })
    }

    #[cfg(unix)]
    fn screenshot(&mut self, udid: &Udid, timeout: Duration) -> Result<Vec<u8>> {
        use std::io::Read as _;
        use std::os::unix::fs::PermissionsExt as _;

        // A directory only this user can enter, removed with the capture.
        let dir = tempfile::Builder::new()
            .prefix("flui-driver-simulator-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let path = dir.path().join("screen.png");
        let command = XcrunCommand::Screenshot {
            udid: udid.clone(),
            path: path.clone(),
        };
        let reply = self.run(&command, tool::MAX_STDERR, timeout)?;
        if let Some(failure) = reply.failure() {
            return Err(command_failed(&command, &reply, &failure));
        }
        let file = std::fs::File::open(&path)
            .map_err(|error| lane_error(format!("`xcrun {command}` wrote no file: {error}")))?;
        let length = file.metadata()?.len();
        if length > MAX_SCREENSHOT_BYTES {
            return Err(lane_error(format!(
                "the Simulator's screenshot is {length} bytes, past the {MAX_SCREENSHOT_BYTES} a \
                 capture may have"
            )));
        }
        let mut bytes = Vec::new();
        // One byte past the bound tells a file that grew since its length was read.
        file.take(MAX_SCREENSHOT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).map_or(true, |read| read > MAX_SCREENSHOT_BYTES) {
            return Err(lane_error(
                "the Simulator's screenshot grew past the bound as it was read",
            ));
        }
        Ok(bytes)
    }

    #[cfg(not(unix))]
    fn screenshot(&mut self, udid: &Udid, _timeout: Duration) -> Result<Vec<u8>> {
        Err(unavailable(format!(
            "the screen of simulator {udid} is captured on a Mac, where the Simulator runs"
        )))
    }
}
