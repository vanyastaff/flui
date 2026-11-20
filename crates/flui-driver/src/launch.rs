//! What a driver starts (ADR-0239 §3): an app built for this host, an artifact a lane installs, or
//! a view hosted in process, and the process ownership that ends a launched app with its session.
//!
//! A launched process gets a fresh endpoint, a token of [`MIN_TOKEN_LEN`] random bytes written as
//! hex (twice the shortest an app takes) and [`EXIT_ON_DISCONNECT_ENV`], so it exits when the
//! driver's connection closes, however the driver went. The driver also owns the child: dropping the session kills a process that is still
//! running, on Windows with everything in its kill-on-close job and on Linux with its process
//! group, where it also dies with the driver (`linux`). Building is not the driver's business,
//! except [`Launch::cargo_example`], which builds one example of this workspace for host-desktop
//! tests.

#[cfg(target_os = "linux")]
mod linux;

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flui_protocol::launch::{ENDPOINT_ENV, EXIT_ON_DISCONNECT_ENV, MIN_TOKEN_LEN, TOKEN_ENV};

use crate::error::{Error, Result};

/// How long a launched app may take to bind its endpoint.
const CONNECT_WAIT: Duration = Duration::from_secs(30);

/// How long a process whose connection broke is given to report its exit code.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// What a launch starts, by the form it comes in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Artifact {
    /// An executable for this host.
    Executable(PathBuf),
    /// A macOS or iOS Simulator `.app` bundle.
    AppBundle(PathBuf),
    /// An Android `.apk`.
    Apk(PathBuf),
    /// A web build: the directory holding its page and wasm module.
    WasmBundle(PathBuf),
}

impl Artifact {
    /// The lane that installs and starts the artifact, which a missing lane names as the missing
    /// capability.
    fn lane(&self) -> &'static str {
        match self {
            Self::Executable(_) => "desktop",
            Self::AppBundle(_) => "apple",
            Self::Apk(_) => "android",
            Self::WasmBundle(_) => "chrome",
        }
    }
}

/// What to start and how: an app built for this host, an artifact a lane installs, or a view
/// hosted in process, with the arguments, environment and connect wait it starts with.
#[must_use]
pub struct Launch {
    pub(crate) target: Target,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    connect_wait: Duration,
}

pub(crate) enum Target {
    Artifact(Artifact),
    CargoExample {
        example: String,
        package: Option<String>,
    },
    #[cfg(feature = "headless")]
    Headless(HeadlessLaunch),
}

impl Launch {
    /// Start `artifact`.
    pub fn artifact(artifact: Artifact) -> Self {
        Self {
            target: Target::Artifact(artifact),
            args: Vec::new(),
            env: Vec::new(),
            connect_wait: CONNECT_WAIT,
        }
    }

    /// Start the executable at `path`.
    pub fn executable(path: impl Into<PathBuf>) -> Self {
        Self::artifact(Artifact::Executable(path.into()))
    }

    /// Build the example `example` of the workspace the current directory belongs to with
    /// `cargo build` for this host, in its debug profile, which carries the agent (ADR-0239), and start
    /// what it built.
    pub fn cargo_example(example: impl Into<String>) -> Self {
        Self {
            target: Target::CargoExample {
                example: example.into(),
                package: None,
            },
            ..Self::artifact(Artifact::Executable(PathBuf::new()))
        }
    }

    /// The example is `package`'s.
    pub fn package(mut self, package: impl Into<String>) -> Self {
        if let Target::CargoExample { package: named, .. } = &mut self.target {
            *named = Some(package.into());
        }
        self
    }

    /// Pass `arg` to the app.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set `key` to `value` in the app's environment.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Give the app `wait` to bind its endpoint; 30 s when not named.
    pub fn connect_wait(mut self, wait: Duration) -> Self {
        self.connect_wait = wait;
        self
    }

    pub(crate) fn connect_timeout(&self) -> Duration {
        self.connect_wait
    }

    /// The executable this launch starts on this host, built first for an example.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming the lane an artifact of another kind needs, and the build's
    /// failure for an example.
    pub(crate) fn executable_path(&self) -> Result<PathBuf> {
        match &self.target {
            Target::Artifact(Artifact::Executable(path)) => Ok(path.clone()),
            // An APK needs its package and activity, which the path does not name.
            Target::Artifact(Artifact::Apk(apk)) => Err(Error::not_supported(
                "android_launch",
                format!(
                    "{} is started by `lane::android::launch`, which names its package and \
                     activity",
                    apk.display()
                ),
            )),
            Target::Artifact(artifact) => Err(Error::not_supported(
                format!("{}_lane", artifact.lane()),
                format!(
                    "{artifact:?} is installed by the {} lane, which this build of the driver has \
                     not",
                    artifact.lane()
                ),
            )),
            Target::CargoExample { example, package } => build_example(example, package.as_deref()),
            #[cfg(feature = "headless")]
            Target::Headless(_) => Err(Error::invalid_argument(
                "a headless launch starts no executable",
            )),
        }
    }

    /// Start the app at `executable` with a fresh endpoint and token.
    pub(crate) fn spawn(&self, executable: &Path) -> Result<Launched> {
        let place = EndpointPlace::new()?;
        let token = launch_token()?;
        let mut command = Command::new(executable);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .env(ENDPOINT_ENV, &place.address)
            .env(TOKEN_ENV, &token)
            .env(EXIT_ON_DISCONNECT_ENV, "1")
            .stdin(Stdio::null());
        let started = |error: io::Error| {
            Error::Io(io::Error::new(
                error.kind(),
                format!("could not start {}: {error}", executable.display()),
            ))
        };
        // The job comes first, and the app joins it before its first instruction: nothing it
        // starts escapes the job.
        #[cfg(windows)]
        let job = crate::lane::windows::job::KillOnCloseJob::try_new()?;
        #[cfg(windows)]
        let child = job
            .spawn_suspended(&mut command)
            .map_err(|error| match error {
                Error::Io(error) => started(error),
                other => other,
            })?
            .resume()?;
        // On Unix the app leads a process group of its own, which the session ends whole
        // (`AppProcess::end`); on Linux it also dies with the session's starter thread.
        #[cfg(target_os = "linux")]
        let (child, starter) = linux::spawn(command).map_err(started)?;
        #[cfg(all(unix, not(target_os = "linux")))]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        #[cfg(not(any(windows, target_os = "linux")))]
        let child = command.spawn().map_err(started)?;
        Ok(Launched {
            endpoint: place.address.clone(),
            token,
            process: AppProcess {
                child,
                #[cfg(windows)]
                _job: job,
                #[cfg(target_os = "linux")]
                _starter: starter,
                _place: place,
            },
        })
    }
}

impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let target = match &self.target {
            Target::Artifact(artifact) => format!("{artifact:?}"),
            Target::CargoExample { example, package } => {
                format!("cargo example {example} of {package:?}")
            }
            #[cfg(feature = "headless")]
            Target::Headless(headless) => format!("headless {}", headless.name),
        };
        f.debug_struct("Launch")
            .field("target", &target)
            .field("args", &self.args)
            .field("connect_wait", &self.connect_wait)
            .finish_non_exhaustive()
    }
}

/// A started app: its process, and where its agent listens.
pub(crate) struct Launched {
    pub(crate) process: AppProcess,
    pub(crate) endpoint: String,
    pub(crate) token: String,
}

/// A process the session owns: killed when the session ends while it still runs. On Windows it
/// runs in a kill-on-close job, so whatever it started ends with it, and so does the app when the
/// driver's process ends without dropping the session. On Unix it leads a process group, which
/// ends with it.
pub(crate) struct AppProcess {
    child: Child,
    #[cfg(windows)]
    _job: crate::lane::windows::job::KillOnCloseJob,
    /// The thread it was started from, whose end kills it: dropped after the process ended.
    #[cfg(target_os = "linux")]
    _starter: linux::Starter,
    /// Where its agent listens: the private directory its socket lives in (Unix), removed with the
    /// session, after the process ended.
    _place: EndpointPlace,
}

impl AppProcess {
    /// The process id.
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    /// How the process is. On Linux it is left unreaped until [`Self::end`], so its id names its
    /// group until the group is killed.
    fn state(&mut self) -> ProcessState {
        #[cfg(target_os = "linux")]
        {
            linux::state(&self.child)
        }
        #[cfg(not(target_os = "linux"))]
        match self.child.try_wait() {
            Ok(Some(status)) => ProcessState::Ended(status.code()),
            Ok(None) => ProcessState::Running,
            Err(_) => ProcessState::Ended(None),
        }
    }

    /// Whether the process still runs.
    pub(crate) fn is_running(&mut self) -> bool {
        self.state() == ProcessState::Running
    }

    /// The exit code, waiting a little for a process whose connection just broke to finish
    /// exiting; `None` while it runs, or when it ended without one (a signal).
    pub(crate) fn exit_code(&mut self) -> Option<i32> {
        let deadline = Instant::now() + EXIT_WAIT;
        loop {
            match self.state() {
                ProcessState::Ended(code) => return code,
                ProcessState::Running if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                ProcessState::Running => return None,
            }
        }
    }

    /// End the process if it still runs, and reap it, with the process group it leads on Unix.
    /// On Linux the group ends whether or not the app itself still runs, while `waitid` answers
    /// for the unreaped app (`linux::kill_group`). On other Unix hosts it ends while the leader
    /// runs unreaped, when the group's id is still its own, so the signal reaches nothing else; a
    /// leader that already exited leaves its group to [`EXIT_ON_DISCONNECT_ENV`].
    pub(crate) fn end(&mut self) {
        // Before the app is reaped, while its id still names its group.
        #[cfg(target_os = "linux")]
        linux::kill_group(&self.child);
        if self.state() == ProcessState::Running {
            #[cfg(all(unix, not(target_os = "linux")))]
            {
                let group = rustix::process::Pid::from_child(&self.child);
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// How a launched process is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessState {
    /// It runs.
    Running,
    /// It ended, with its exit code, or none when a signal ended it.
    Ended(Option<i32>),
}

impl Drop for AppProcess {
    fn drop(&mut self) {
        self.end();
    }
}

/// Where a launched app binds its endpoint: a fresh pipe name on Windows, a socket in a private
/// directory elsewhere.
pub(crate) struct EndpointPlace {
    pub(crate) address: String,
    #[cfg(unix)]
    _dir: tempfile::TempDir,
}

impl EndpointPlace {
    #[cfg(unix)]
    pub(crate) fn new() -> Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        // The app refuses a socket whose directory another user can enter.
        let dir = tempfile::Builder::new()
            .prefix("flui-driver-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let address = dir
            .path()
            .join("agent.sock")
            .into_os_string()
            .into_string()
            .map_err(|_| {
                Error::invalid_argument("the temporary directory's path is not UTF-8; set TMPDIR")
            })?;
        Ok(Self { address, _dir: dir })
    }

    #[cfg(windows)]
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            address: format!("flui-driver-{}-{}", std::process::id(), random_hex(8)?),
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub(crate) fn new() -> Result<Self> {
        Err(Error::not_supported(
            "local_socket",
            "this host has no local sockets to launch an app with",
        ))
    }
}

/// A fresh launch token: twice the shortest an app takes, as hex.
pub(crate) fn launch_token() -> Result<String> {
    random_hex(MIN_TOKEN_LEN)
}

/// `bytes` random bytes from the OS, as lowercase hex.
fn random_hex(bytes: usize) -> Result<String> {
    let mut buffer = vec![0_u8; bytes];
    getrandom::fill(&mut buffer).map_err(|error| {
        Error::Io(io::Error::other(format!(
            "the OS random source failed: {error}"
        )))
    })?;
    Ok(buffer
        .iter()
        .fold(String::with_capacity(bytes * 2), |mut hex, byte| {
            use std::fmt::Write as _;
            write!(hex, "{byte:02x}").expect("BUG: writing to a String cannot fail");
            hex
        }))
}

/// Builds `example` with cargo and answers the executable it built.
fn build_example(example: &str, package: Option<&str>) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.args([
        "build",
        "--example",
        example,
        "--message-format=json-render-diagnostics",
    ]);
    if let Some(package) = package {
        command.args(["--package", package]);
    }
    let output = command.stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        return Err(Error::Io(io::Error::other(format!(
            "`cargo build --example {example}` failed ({})",
            output.status
        ))));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == example
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| {
            Error::Io(io::Error::other(format!(
                "`cargo build --example {example}` built no executable"
            )))
        })
}

/// A view hosted in process ([`Launch::headless`]).
#[cfg(feature = "headless")]
pub(crate) struct HeadlessLaunch {
    pub(crate) name: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) reply_timeout: Duration,
    pub(crate) attach: HeadlessAttach,
}

/// Attaches the launch's root view to the host on the app's thread.
#[cfg(feature = "headless")]
pub(crate) type HeadlessAttach =
    Box<dyn FnOnce(&flui_testing::HeadlessHost) -> std::result::Result<(), String> + Send>;

#[cfg(feature = "headless")]
impl Launch {
    /// Host the view `root` makes in process, in an 800 by 600 window, on a virtual clock
    /// (ADR-0238 §8). `root` runs on the app's own thread, the view never crossing to the
    /// driver's.
    pub fn headless<V, F>(root: F) -> Self
    where
        F: FnOnce() -> V + Send + 'static,
        V: flui_view::View + Clone + 'static,
    {
        Self {
            target: Target::Headless(HeadlessLaunch {
                name: "headless".to_owned(),
                width: 800,
                height: 600,
                reply_timeout: Duration::from_secs(5),
                attach: Box::new(move |host| {
                    host.attach(&root())
                        .map_err(|error| format!("the root did not attach: {error}"))
                }),
            }),
            ..Self::artifact(Artifact::Executable(PathBuf::new()))
        }
    }

    /// The name the app introduces itself with.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        if let Target::Headless(headless) = &mut self.target {
            headless.name = name.into();
        }
        self
    }

    /// Open the headless window at `width` by `height` logical pixels.
    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        if let Target::Headless(headless) = &mut self.target {
            headless.width = width;
            headless.height = height;
        }
        self
    }

    /// How long the headless app's agent waits for its window's answer, in virtual time, before
    /// it answers `timeout`: an acting request then stays queued and runs at the window's next
    /// drain. 5 s when not named.
    pub fn reply_timeout(mut self, timeout: Duration) -> Self {
        if let Target::Headless(headless) = &mut self.target {
            headless.reply_timeout = timeout;
        }
        self
    }
}
