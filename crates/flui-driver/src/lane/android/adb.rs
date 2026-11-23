//! The lane's one door to the device: `adb`, every command run with a deadline and a bound on what
//! it may answer, and every argument from outside the lane quoted for the device's shell.
//!
//! - **Commands** ([`AdbCommand`]) run to completion through [`Adb::run`]: a command that outlives
//!   its deadline is killed and answers [`Error::Timeout`] with a `may_have_run` effect, since the
//!   device may have done it; one that prints more than its bound is killed and fails, and nothing
//!   past the bound is kept.
//! - **Streams** ([`Adb::open`]) are the long-lived commands, `getevent` and `uinput`: what they
//!   print arrives as records of at most [`MAX_RECORD`] bytes, split as lines or as JSON objects
//!   ([`Framing`]), at most [`RECORD_ROOM`] waiting; past that the reader stops reading until the
//!   lane takes one, so the device's own pipe holds the rest.
//! - **Quoting.** `adb shell` joins its arguments into one line the device's shell parses, so a
//!   value the lane did not write (typed text, a package name) goes into it through [`quote`],
//!   whatever it holds.
//!
//! [`HostAdb`] runs the host's `adb`; a test scripts the device with `testing::ScriptedAdb`.

use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::ErrorCode;

use super::LANE;
use crate::error::{Error, Result};
use crate::lane::tool::{self, end};

/// The most bytes one record of a stream may hold.
pub const MAX_RECORD: usize = 4096;

/// How many records of a stream wait for the lane before its reader stops reading.
pub const RECORD_ROOM: usize = 1024;

/// One `adb` command against the device.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdbCommand {
    /// `adb devices -l`: the devices the adb server knows, no device named.
    Devices,
    /// `adb shell <line>`: `line` runs in the device's shell, each value from outside the lane
    /// quoted with [`quote`].
    Shell(String),
    /// `adb shell -tt <line>`: the same on a terminal, so a program that buffers what it prints
    /// into a pipe (`getevent`) writes each line as it prints it.
    ShellTerminal(String),
    /// `adb exec-out <line>`: the same, its output passed through unchanged (an image).
    ExecOut(String),
    /// `adb <args>`: a command of the host's `adb` against the device (`install`, `forward`).
    Host(Vec<String>),
}

impl AdbCommand {
    /// The arguments after `adb -s <serial>`.
    #[must_use]
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Devices => vec!["devices".to_owned(), "-l".to_owned()],
            Self::Shell(line) => vec!["shell".to_owned(), line.clone()],
            Self::ShellTerminal(line) => vec!["shell".to_owned(), "-tt".to_owned(), line.clone()],
            Self::ExecOut(line) => vec!["exec-out".to_owned(), line.clone()],
            Self::Host(args) => args.clone(),
        }
    }

    /// Whether the command names a device: every one but [`Self::Devices`].
    #[must_use]
    pub fn names_device(&self) -> bool {
        !matches!(self, Self::Devices)
    }
}

impl std::fmt::Display for AdbCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.args().join(" "))
    }
}

/// What a command answered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct AdbReply {
    /// Its exit code; `None` when it ended without one.
    pub exit: Option<i32>,
    /// What it printed, at most the bound it ran with.
    pub stdout: Vec<u8>,
    /// What it printed as errors, at most 64 KiB.
    pub stderr: String,
}

impl AdbReply {
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

    /// Why the command failed, when it did: an exit code other than 0, or a device-side failure
    /// the device's shell reported with exit 0 (an older `adb` passes no exit code through, and a
    /// Java command prints its exception and may still exit 0). What it printed is searched for a
    /// failure only when it printed a status (`Printed::Status`): data (a window dump, an image)
    /// may hold any text.
    #[must_use]
    pub fn failure(&self, printed: Printed) -> Option<String> {
        let said = |text: &str| {
            text.lines()
                .map(str::trim)
                .find(|line| {
                    line.starts_with("Error")
                        || line.starts_with("error:")
                        || line.contains("Exception")
                })
                .map(str::to_owned)
        };
        let reason = said(&self.stderr).or_else(|| match printed {
            Printed::Status => said(&self.text()),
            Printed::Data => None,
        });
        match (self.exit, reason) {
            (Some(0), None) => None,
            (_, Some(reason)) => Some(cut(&reason)),
            (Some(code), None) => Some(format!(
                "it exited with {code}{}",
                if self.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", cut(self.stderr.trim()))
                }
            )),
            (None, None) => Some("it ended without an exit code".to_owned()),
        }
    }
}

/// What a command prints, which decides whether its output is searched for a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Printed {
    /// A status a Java command or `am` writes, where a failure is written too (`input`, `am`,
    /// `install`, `forward`).
    Status,
    /// Data that may hold any text: a window dump, an image, a property, a window title.
    Data,
}

/// `text` cut to what a message carries.
fn cut(text: &str) -> String {
    const MESSAGE: usize = 300;
    match text.char_indices().nth(MESSAGE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// How a stream's output is cut into records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Framing {
    /// At each line break, a carriage return before it dropped: `getevent`'s lines.
    Lines,
    /// At the end of each top-level JSON object, whatever lies between objects dropped: `uinput`
    /// answers with objects and no line breaks.
    JsonObjects,
}

/// A long-lived command: records of what it prints, and lines into its input.
pub trait AdbStream: Send {
    /// Write `line` and a line break to the command's input.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the command no longer reads its input.
    fn send_line(&mut self, line: &str) -> Result<()>;

    /// The next record within `wait`; `Ok(None)` when none came. A `wait` of zero takes only what
    /// already arrived.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] once the command ended, naming why, and for a record past [`MAX_RECORD`].
    fn next_record(&mut self, wait: Duration) -> Result<Option<String>>;
}

/// The device, through `adb`: see the [module docs](self).
pub trait Adb: Send {
    /// Run `command` to its end within `timeout`, keeping at most `limit` bytes of its output.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] with a `may_have_run` effect past `timeout`; [`Error::Lane`] when `adb`
    /// cannot be started (unavailable), or the output passes `limit`.
    fn run(&mut self, command: &AdbCommand, limit: usize, timeout: Duration) -> Result<AdbReply>;

    /// Start `command` as a stream whose output is cut as `framing` says. Dropping the stream ends
    /// the command.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when `adb` cannot be started.
    fn open(&mut self, command: &AdbCommand, framing: Framing) -> Result<Box<dyn AdbStream>>;
}

/// `value` as one word of the device's shell, whatever it holds: in single quotes, each single
/// quote of it closed, escaped and reopened.
#[must_use]
pub fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// A shell line of `words`, each quoted.
#[must_use]
pub fn shell_line(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| quote(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A lane failure the lane can still be run past.
pub(crate) fn lane_error(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}

/// A failure that means the host cannot run the lane at all: no `adb`, no device.
pub(crate) fn unavailable(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: true,
        message: message.into(),
    }
}

/// The error of `command`, which answered `reply` with `failure`: a device that went or was
/// never there is unavailable, which a report makes cannot-verify.
pub(crate) fn command_failed(command: &AdbCommand, reply: &AdbReply, failure: &str) -> Error {
    let gone = [
        "device offline",
        "not found",
        "no devices/emulators found",
        "unauthorized",
        "device still authorizing",
    ];
    let device_gone = gone.iter().any(|said| reply.stderr.contains(said));
    let message = format!("`adb {command}` failed: {failure}");
    if device_gone {
        unavailable(message)
    } else {
        lane_error(message)
    }
}

/// `adb` on this host, against one device.
#[derive(Debug, Clone)]
pub struct HostAdb {
    program: PathBuf,
    serial: String,
}

impl HostAdb {
    /// `adb` at `program` against the device `serial`: there is no `HostAdb` for no device, so
    /// every command it runs names the device someone named (`devices` alone lists them all).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an empty serial.
    pub fn try_new(program: impl Into<PathBuf>, serial: impl Into<String>) -> Result<Self> {
        let serial = serial.into();
        if serial.trim().is_empty() {
            return Err(Error::invalid_argument(
                "an adb serial names a device; an empty one names none",
            ));
        }
        Ok(Self {
            program: program.into(),
            serial,
        })
    }

    /// The host's `adb` against the device `serial`: `platform-tools` of the SDK `ANDROID_HOME`
    /// or `ANDROID_SDK_ROOT` names, else `adb` on the `PATH`.
    ///
    /// # Errors
    ///
    /// As [`Self::try_new`].
    pub fn locate(serial: impl Into<String>) -> Result<Self> {
        let sdk = std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT"));
        let program = sdk
            .map(|sdk| Path::new(&sdk).join("platform-tools"))
            .and_then(|tools| {
                ["adb.exe", "adb"]
                    .into_iter()
                    .map(|name| tools.join(name))
                    .find(|path| path.is_file())
            })
            .unwrap_or_else(|| PathBuf::from("adb"));
        Self::try_new(program, serial)
    }

    fn command(&self, command: &AdbCommand) -> Command {
        let mut process = Command::new(&self.program);
        if command.names_device() {
            process.args(["-s", self.serial.as_str()]);
        }
        process.args(command.args());
        process
    }

    fn started(&self, error: &io::Error) -> Error {
        unavailable(format!(
            "could not start `{}`: {error}; install the Android SDK's platform-tools or set \
             ANDROID_HOME",
            self.program.display()
        ))
    }
}

impl Adb for HostAdb {
    fn run(&mut self, command: &AdbCommand, limit: usize, timeout: Duration) -> Result<AdbReply> {
        let finished = tool::run(self.command(command), limit, timeout, "adb").map_err(
            |failure| match failure {
                tool::Failure::Start(error) => self.started(&error),
                tool::Failure::Io(error) => Error::Io(error),
                tool::Failure::Timeout => Error::Timeout {
                    effect: Some(Effect::new(EffectKind::MayHaveRun).detail(format!(
                        "`adb {command}` was ended after {} ms; the device may have done it",
                        timeout.as_millis()
                    ))),
                    message: format!(
                        "`adb {command}` did not end within {} ms",
                        timeout.as_millis()
                    ),
                    retry: ErrorCode::Timeout.default_retry(),
                },
                tool::Failure::OverLimit => lane_error(format!(
                    "`adb {command}` printed more than {limit} bytes; nothing past them was read"
                )),
                tool::Failure::OutputLeftOpen => {
                    lane_error(format!("`adb {command}` left its output open"))
                }
            },
        )?;
        Ok(AdbReply {
            exit: finished.exit,
            stdout: finished.stdout,
            stderr: finished.stderr,
        })
    }

    fn open(&mut self, command: &AdbCommand, framing: Framing) -> Result<Box<dyn AdbStream>> {
        let mut process = self.command(command);
        process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = process.spawn().map_err(|error| self.started(&error))?;
        let stdout = child
            .stdout
            .take()
            .expect("BUG: the stream's output was piped");
        let stdin = child.stdin.take();
        let (records, inbox) = mpsc::sync_channel(RECORD_ROOM);
        let what = command.to_string();
        let reader = thread::Builder::new()
            .name("flui-driver-adb-stream".into())
            .spawn(move || read_records(stdout, framing, &records, &what))?;
        Ok(Box::new(HostStream {
            child,
            stdin,
            inbox,
            reader: Some(reader),
            ended: None,
        }))
    }
}

/// What a stream's reader hands over: a record, or why the stream ended.
type Incoming = std::result::Result<String, String>;

/// Cut what `from` prints into records as `framing` says and hand them over, one at a time;
/// the reader blocks while [`RECORD_ROOM`] records wait.
fn read_records(from: impl Read, framing: Framing, records: &SyncSender<Incoming>, what: &str) {
    let mut splitter = Splitter::new(framing);
    let mut reader = BufReader::new(from);
    let mut buffer = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => {
                let _ = records.send(Err(format!("`adb {what}` ended")));
                return;
            }
            Ok(read) => read,
            Err(error) => {
                let _ = records.send(Err(format!("reading `adb {what}` failed: {error}")));
                return;
            }
        };
        for &byte in &buffer[..read] {
            match splitter.push(byte) {
                Ok(Some(record)) => {
                    if records.send(Ok(record)).is_err() {
                        return;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = records.send(Err(format!("`adb {what}` {error}")));
                    return;
                }
            }
        }
    }
}

/// Cuts bytes into records: see [`Framing`].
#[derive(Debug)]
pub struct Splitter {
    framing: Framing,
    record: Vec<u8>,
    depth: u32,
    in_string: bool,
    escaped: bool,
}

impl Splitter {
    /// A splitter cutting as `framing` says.
    #[must_use]
    pub fn new(framing: Framing) -> Self {
        Self {
            framing,
            record: Vec::new(),
            depth: 0,
            in_string: false,
            escaped: false,
        }
    }

    /// Take `byte`; the record it ends, if it ends one.
    ///
    /// # Errors
    ///
    /// When a record passes [`MAX_RECORD`] bytes.
    pub fn push(&mut self, byte: u8) -> std::result::Result<Option<String>, String> {
        match self.framing {
            Framing::Lines => {
                if byte == b'\n' {
                    if self.record.last() == Some(&b'\r') {
                        self.record.pop();
                    }
                    return Ok(Some(self.take()));
                }
                self.keep(byte)?;
            }
            Framing::JsonObjects => {
                if self.depth == 0 && byte != b'{' {
                    // Between objects: whatever it is, it is no record.
                    return Ok(None);
                }
                self.keep(byte)?;
                if self.in_string {
                    match (self.escaped, byte) {
                        (true, _) => self.escaped = false,
                        (false, b'\\') => self.escaped = true,
                        (false, b'"') => self.in_string = false,
                        _ => {}
                    }
                    return Ok(None);
                }
                match byte {
                    b'"' => self.in_string = true,
                    b'{' => self.depth += 1,
                    b'}' => {
                        self.depth -= 1;
                        if self.depth == 0 {
                            return Ok(Some(self.take()));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn keep(&mut self, byte: u8) -> std::result::Result<(), String> {
        if self.record.len() >= MAX_RECORD {
            return Err(format!("printed a record longer than {MAX_RECORD} bytes"));
        }
        self.record.push(byte);
        Ok(())
    }

    fn take(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.record)).into_owned()
    }
}

/// A stream of the host's `adb`.
struct HostStream {
    child: Child,
    stdin: Option<ChildStdin>,
    inbox: Receiver<Incoming>,
    reader: Option<JoinHandle<()>>,
    ended: Option<String>,
}

impl AdbStream for HostStream {
    fn send_line(&mut self, line: &str) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| lane_error("the stream takes no input"))?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush())
            .map_err(|error| lane_error(format!("the stream no longer reads its input: {error}")))
    }

    fn next_record(&mut self, wait: Duration) -> Result<Option<String>> {
        if let Some(why) = &self.ended {
            return Err(lane_error(why.clone()));
        }
        let incoming = if wait.is_zero() {
            match self.inbox.try_recv() {
                Ok(incoming) => incoming,
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => Err("the stream's reader ended".to_owned()),
            }
        } else {
            match self.inbox.recv_timeout(wait) {
                Ok(incoming) => incoming,
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => Err("the stream's reader ended".to_owned()),
            }
        };
        incoming.map(Some).map_err(|why| {
            self.ended = Some(why.clone());
            lane_error(why)
        })
    }
}

impl Drop for HostStream {
    fn drop(&mut self) {
        drop(self.stdin.take());
        end(&mut self.child);
        // The reader ends at the closed pipe, or at the dropped inbox once it sends. It is not
        // joined: a pipe a child of `adb` kept open would hold the drop.
        drop(self.reader.take());
    }
}
