//! The driver agent from the CLI's side (ADR-0239).
//!
//! Every debug FLUI app, and a release build with the `driver` feature,
//! serves its windows over a local endpoint when its launch names one. The
//! commands meet at that endpoint and share nothing but the protocol and the
//! credentials in this module:
//!
//! - `flui run --agent` creates a [`Session`]: a fresh endpoint address, a
//!   launch token, and a file naming the token and where a client reaches the
//!   app. It starts a desktop app with the endpoint and the token in its
//!   environment, and an Android app with them as the launching intent's
//!   extras, its endpoint an abstract socket that [`forward::AdbForward`]
//!   carries a loopback port to.
//! - `flui mcp` ([`mcp`]) serves MCP over stdio and forwards each tool call
//!   to that endpoint through a [`client::Connection`].
//! - `flui record` ([`record`]) connects as an observer and writes a recording
//!   of the app's input to a file.
//!
//! The launch contract (the variable and extra names, the shortest token) and
//! the wire types are `flui-protocol`'s, the contract crate the app's server
//! reads too.

pub(crate) mod browser;
pub(crate) mod client;
pub(crate) mod forward;
pub(crate) mod mcp;
pub(crate) mod record;

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) use flui_protocol::launch::{ENDPOINT_ENV, TOKEN_ENV};
use flui_protocol::launch::{ENDPOINT_EXTRA, TOKEN_EXTRA};

use self::forward::AdbForward;

use crate::error::{CliError, CliResult, ResultExt};

/// The feature, on the `flui` facade and on `flui-app`, that keeps the agent
/// in a build without debug assertions; a debug build carries it without one.
pub(crate) const DRIVER_FEATURE: &str = "driver";

/// The file `flui run --agent` writes beside the build output so
/// `flui mcp` finds the running app without arguments.
const SESSION_FILE: &str = "flui-agent.json";

/// The longest Unix socket path every supported OS accepts (`sun_path` is 104
/// bytes on macOS, 108 on Linux, one of them the terminating NUL).
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 100;

/// Where a client reaches an app's agent.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Endpoint {
    /// The endpoint the app binds on this host: a pipe name on Windows, a
    /// socket path elsewhere.
    Local(String),
    /// A loopback port that `adb forward` carries to an Android app's
    /// abstract socket.
    Forwarded(SocketAddr),
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(address) => f.write_str(address),
            Self::Forwarded(address) => write!(f, "{address} (adb forward)"),
        }
    }
}

/// Where a client reaches the app and the token that admits it. Written to
/// the session file, so it is what `flui mcp` reads.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Credentials {
    pub(crate) endpoint: Endpoint,
    /// At least 32 bytes, hex.
    pub(crate) token: String,
    /// The `flui run` process that owns the session.
    pub(crate) pid: u32,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .field("pid", &self.pid)
            .finish()
    }
}

impl Credentials {
    /// The credentials in [`ENDPOINT_ENV`] and [`TOKEN_ENV`], when both are
    /// set and non-empty.
    pub(crate) fn from_env() -> Option<Self> {
        let endpoint = std::env::var(ENDPOINT_ENV).ok().filter(|v| !v.is_empty())?;
        let token = std::env::var(TOKEN_ENV).ok().filter(|v| !v.is_empty())?;
        Some(Self {
            endpoint: Endpoint::Local(endpoint),
            token,
            pid: 0,
        })
    }

    /// The credentials a client uses now: the session file named on its command line, else
    /// the environment, else the nearest `target/flui-agent.json`.
    pub(crate) fn find(session: Option<&Path>) -> Result<Self, String> {
        if let Some(path) = session {
            return Self::read(path);
        }
        if let Some(credentials) = Self::from_env() {
            return Ok(credentials);
        }
        let cwd = std::env::current_dir().map_err(|e| format!("no working directory: {e}"))?;
        match find_session_file(&cwd, target_dir_override().as_deref()) {
            Some(path) => Self::read(&path),
            None => Err(format!(
                "no running FLUI app found from {}. Start one with `flui run --agent`, \
                 or pass --session <file>, or set {ENDPOINT_ENV} and {TOKEN_ENV}",
                cwd.display()
            )),
        }
    }

    /// Reads a session file.
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let bytes =
            fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} is not a flui agent session: {e}", path.display()))
    }
}

/// Where `start` or a directory above it keeps its session file.
///
/// `target_dir` (the command's [`target_dir_override`]) wins; otherwise the
/// nearest ancestor's `target/` that holds one is the answer for a reader, and
/// `start`'s own workspace root for a writer ([`session_file_for_writing`]).
pub(crate) fn find_session_file(start: &Path, target_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = target_dir {
        let file = dir.join(SESSION_FILE);
        return file.is_file().then_some(file);
    }
    start
        .ancestors()
        .map(|dir| dir.join("target").join(SESSION_FILE))
        .find(|file| file.is_file())
}

/// Where a session started in `start` writes its file: beside the build
/// output of the workspace `start` belongs to, or in `target_dir` when the
/// command builds into one.
fn session_file_for_writing(
    start: &Path,
    workspace_root: Option<&Path>,
    target_dir: Option<&Path>,
) -> PathBuf {
    let target = target_dir.map_or_else(
        || workspace_root.unwrap_or(start).join("target"),
        Path::to_path_buf,
    );
    target.join(SESSION_FILE)
}

/// The target directory `CARGO_TARGET_DIR` names, which every project builds
/// into and keeps its session file in. Each command reads it once and passes
/// it on, so nothing above reads the environment.
pub(crate) fn target_dir_override() -> Option<PathBuf> {
    std::env::var_os("CARGO_TARGET_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// An agent session owned by one `flui run --agent`: the credentials, what
/// keeps the endpoint (the private directory holding a desktop app's socket,
/// or an Android app's `adb forward`), and the session file. Dropping it
/// removes all of them, so a session never outlives the command.
pub(crate) struct Session {
    credentials: Credentials,
    /// The address the app binds: the endpoint of [`Self::credentials`] on
    /// the desktop, `@name` for an Android app's abstract socket, the global
    /// a page's app defines its export as.
    app_endpoint: String,
    /// What keeps the endpoint; removed on drop.
    _held: Held,
    file: PathBuf,
    /// The feature a build without debug assertions needs to keep the agent.
    driver_feature: Option<String>,
}

/// What a session holds its endpoint with.
enum Held {
    /// The private directory holding a desktop app's socket (Unix).
    Directory { _dir: tempfile::TempDir },
    /// The loopback port `adb` carries to an Android app's abstract socket.
    Forward { _forward: AdbForward },
}

impl Session {
    /// A new session for the project at `start` (`workspace_root` is the
    /// workspace it belongs to, when it has one; `target_dir` the
    /// command's [`target_dir_override`]).
    pub(crate) fn create(
        start: &Path,
        workspace_root: Option<&Path>,
        target_dir: Option<&Path>,
    ) -> CliResult<Self> {
        let dir = tempfile::Builder::new()
            .prefix("flui-agent-")
            .tempdir()
            .context("could not create the agent session directory")?;
        // The server refuses a socket in a directory anyone else can enter,
        // and the temp directory's mode follows the umask.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))
                .context("could not make the agent session directory private")?;
        }
        let endpoint = endpoint_in(dir.path(), &random_hex(4)?)?;
        let credentials = Credentials {
            endpoint: Endpoint::Local(endpoint.clone()),
            token: launch_token()?,
            pid: std::process::id(),
        };
        let file = session_file_for_writing(start, workspace_root, target_dir);
        write_private(&file, &credentials).context("could not write the agent session file")?;
        Ok(Self {
            credentials,
            app_endpoint: endpoint,
            _held: Held::Directory { _dir: dir },
            file,
            driver_feature: None,
        })
    }

    /// A new session for an Android app on the device `serial`, which `adb`
    /// reaches: the app binds a fresh abstract socket, and a loopback port of
    /// this host is forwarded to it. The rest is [`Self::create`]'s.
    pub(crate) fn create_forwarded(
        start: &Path,
        workspace_root: Option<&Path>,
        target_dir: Option<&Path>,
        adb: &Path,
        serial: &str,
    ) -> CliResult<Self> {
        let socket = format!("flui-agent-{}", random_hex(8)?);
        let forward = AdbForward::open(adb, serial, &socket)?;
        let credentials = Credentials {
            endpoint: Endpoint::Forwarded(forward.address()),
            token: launch_token()?,
            pid: std::process::id(),
        };
        let file = session_file_for_writing(start, workspace_root, target_dir);
        write_private(&file, &credentials).context("could not write the agent session file")?;
        Ok(Self {
            credentials,
            app_endpoint: format!("@{socket}"),
            _held: Held::Forward { _forward: forward },
            file,
            driver_feature: None,
        })
    }

    /// A new session for an app in a page this command's browser loads: the app defines its
    /// export as the global `export`, and this command serves a fresh local endpoint that
    /// carries each connection there ([`browser::relay`]). The rest is [`Self::create`]'s.
    pub(crate) fn create_relayed(
        start: &Path,
        workspace_root: Option<&Path>,
        target_dir: Option<&Path>,
        export: &str,
    ) -> CliResult<Self> {
        let mut session = Self::create(start, workspace_root, target_dir)?;
        export.clone_into(&mut session.app_endpoint);
        Ok(session)
    }

    /// Building with `feature` (`<key>/`[`DRIVER_FEATURE`], by the key the
    /// application names the crate under), for a profile without debug assertions.
    pub(crate) fn with_driver_feature(mut self, feature: String) -> Self {
        self.driver_feature = Some(feature);
        self
    }

    pub(crate) fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The address the app binds: a pipe name or a socket path on the
    /// desktop, `@name` for an Android app's abstract socket, a global
    /// in a page.
    pub(crate) fn app_endpoint(&self) -> &str {
        &self.app_endpoint
    }

    /// The session file `flui mcp` reads.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// The `--features` value a build without debug assertions needs to keep
    /// the agent, when the profile needs one.
    pub(crate) fn driver_feature(&self) -> Option<&str> {
        self.driver_feature.as_deref()
    }

    /// Gives `cmd` (the cargo that builds and runs the app) the driver
    /// feature when the profile needs it, and the endpoint and token.
    pub(crate) fn apply(&self, cmd: &mut Command) {
        if let Some(feature) = &self.driver_feature {
            cmd.args(["--features", feature]);
        }
        cmd.env(ENDPOINT_ENV, &self.app_endpoint)
            .env(TOKEN_ENV, &self.credentials.token);
    }

    /// The `am start` arguments that hand an Android app the endpoint and the
    /// token as the launching intent's string extras.
    pub(crate) fn intent_extras(&self) -> [&str; 6] {
        [
            "--es",
            ENDPOINT_EXTRA,
            &self.app_endpoint,
            "--es",
            TOKEN_EXTRA,
            &self.credentials.token,
        ]
    }

    /// Clears what a previous run of the app left at the endpoint: a socket
    /// file a killed app could not remove would fail the next bind. An
    /// abstract socket and a named pipe leave nothing behind.
    #[cfg_attr(
        windows,
        expect(clippy::unused_self, reason = "a named pipe leaves no file behind")
    )]
    pub(crate) fn clear_endpoint(&self) {
        #[cfg(unix)]
        if let Endpoint::Local(address) = &self.credentials.endpoint {
            let _ = fs::remove_file(address);
        }
    }

    /// The launch values a page's app is given in its launch object: the global it defines its
    /// export as, and the token.
    pub(crate) fn page_launch(&self) -> [(&str, &str); 2] {
        [
            (ENDPOINT_EXTRA, &self.app_endpoint),
            (TOKEN_EXTRA, &self.credentials.token),
        ]
    }
}

/// A fresh launch token: twice the shortest the app takes, as hex.
fn launch_token() -> CliResult<String> {
    random_hex(flui_protocol::launch::MIN_TOKEN_LEN)
}

impl Drop for Session {
    fn drop(&mut self) {
        self.clear_endpoint();
        // Only our own file: a newer session in the same project replaced it.
        if Credentials::read(&self.file).is_ok_and(|c| c.pid == self.credentials.pid) {
            let _ = fs::remove_file(&self.file);
        }
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("credentials", &self.credentials)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
fn endpoint_in(dir: &Path, _unique: &str) -> CliResult<String> {
    let path = dir.join("agent.sock");
    let path = path.to_str().ok_or_else(|| CliError::Unsupported {
        what: "an agent endpoint".into(),
        reason: "the temporary directory's path is not UTF-8".into(),
    })?;
    if path.len() > MAX_SOCKET_PATH {
        return Err(CliError::Unsupported {
            what: "an agent endpoint".into(),
            reason: format!(
                "the socket path {path} is longer than {MAX_SOCKET_PATH} bytes; set TMPDIR to a shorter directory"
            ),
        });
    }
    Ok(path.to_owned())
}

#[cfg(windows)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as the Unix endpoint, which can fail on a long path"
)]
fn endpoint_in(_dir: &Path, unique: &str) -> CliResult<String> {
    Ok(format!("flui-agent-{}-{unique}", std::process::id()))
}

/// `bytes` random bytes from the OS, as lowercase hex.
fn random_hex(bytes: usize) -> CliResult<String> {
    let mut buf = vec![0_u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| CliError::Unsupported {
        what: "an agent launch token".into(),
        reason: format!("the OS random source failed: {e}"),
    })?;
    Ok(buf
        .iter()
        .fold(String::with_capacity(bytes * 2), |mut hex, b| {
            use std::fmt::Write as _;
            write!(hex, "{b:02x}").expect("BUG: writing to a String cannot fail");
            hex
        }))
}

/// Writes `credentials` to `path` for the current user alone, replacing any
/// file there in one step.
fn write_private(path: &Path, credentials: &Credentials) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(credentials).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    //! Each test's project keeps its session file in its own `target/`: no
    //! target-dir override goes in, so a `CARGO_TARGET_DIR` the test runner
    //! inherited cannot put every test's file in one place.

    use super::*;

    #[test]
    fn a_session_names_a_fresh_endpoint_and_a_token_the_app_accepts() {
        let project = tempfile::tempdir().unwrap();
        let session = Session::create(project.path(), None, None).unwrap();
        let credentials = session.credentials();
        // The app refuses a token shorter than this.
        assert!(credentials.token.len() >= flui_protocol::launch::MIN_TOKEN_LEN);
        assert!(credentials.token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(credentials.pid, std::process::id());

        let other = Session::create(project.path(), None, None).unwrap();
        assert_ne!(credentials.token, other.credentials().token);
        assert_ne!(credentials.endpoint, other.credentials().endpoint);
    }

    #[test]
    fn the_session_file_is_what_flui_mcp_discovers_and_goes_with_the_session() {
        let project = tempfile::tempdir().unwrap();
        let nested = project.path().join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let session = Session::create(project.path(), None, None).unwrap();

        let found = find_session_file(&nested, None).expect("found from a nested directory");
        assert_eq!(found, session.file());
        assert_eq!(Credentials::read(&found).unwrap(), *session.credentials());

        let file = session.file().to_owned();
        drop(session);
        assert!(!file.exists(), "the file goes with the session");
    }

    #[cfg(unix)]
    #[test]
    fn the_session_file_and_its_socket_directory_are_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let project = tempfile::tempdir().unwrap();
        let session = Session::create(project.path(), None, None).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(session.file()), 0o600);
        let socket_dir = Path::new(session.app_endpoint()).parent().unwrap();
        assert_eq!(mode(socket_dir), 0o700, "the app refuses anything wider");
    }

    #[test]
    fn a_newer_session_keeps_its_file_when_an_older_one_ends() {
        let project = tempfile::tempdir().unwrap();
        let older = Session::create(project.path(), None, None).unwrap();
        let file = older.file().to_owned();
        // Another `flui run` in the same project replaced the file.
        let replacement = Credentials {
            endpoint: Endpoint::Local("elsewhere".into()),
            token: "t".repeat(32),
            pid: std::process::id() + 1,
        };
        write_private(&file, &replacement).unwrap();
        drop(older);
        assert_eq!(Credentials::read(&file).unwrap(), replacement);
    }

    #[test]
    fn credentials_never_print_their_token() {
        let credentials = Credentials {
            endpoint: Endpoint::Local("e".into()),
            token: "super-secret-token".into(),
            pid: 1,
        };
        assert!(!format!("{credentials:?}").contains("super-secret"));
    }

    /// An `adb` that logs its arguments to `log` and allocates port 43123.
    #[cfg(unix)]
    fn logging_adb(dir: &Path, log: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let adb = dir.join("adb");
        fs::write(
            &adb,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$3 $4\" = 'forward tcp:0' ] && echo 43123\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&adb, fs::Permissions::from_mode(0o755)).unwrap();
        adb
    }

    #[cfg(unix)]
    #[test]
    fn an_android_session_forwards_a_port_to_a_fresh_abstract_socket_and_removes_it() {
        let project = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        let log = tools.path().join("adb.log");
        let adb = logging_adb(tools.path(), &log);

        let session =
            Session::create_forwarded(project.path(), None, None, &adb, "emulator-5554").unwrap();
        let socket = session
            .app_endpoint()
            .strip_prefix('@')
            .expect("the app binds an abstract socket")
            .to_owned();
        assert!(socket.starts_with("flui-agent-"), "{socket}");
        assert_eq!(
            session.credentials().endpoint,
            Endpoint::Forwarded(SocketAddr::from(([127, 0, 0, 1], 43123))),
            "a client connects to the forwarded port"
        );
        assert_eq!(
            session.intent_extras(),
            [
                "--es",
                "flui.agent.endpoint",
                session.app_endpoint(),
                "--es",
                "flui.agent.token",
                session.credentials().token.as_str(),
            ]
        );
        assert_eq!(
            Credentials::read(session.file()).unwrap(),
            *session.credentials()
        );
        let other =
            Session::create_forwarded(project.path(), None, None, &adb, "emulator-5554").unwrap();
        assert_ne!(other.app_endpoint(), session.app_endpoint());

        let calls = || fs::read_to_string(&log).unwrap();
        assert_eq!(
            calls().lines().next(),
            Some(format!("-s emulator-5554 forward tcp:0 localabstract:{socket}").as_str())
        );
        let removed = || {
            calls()
                .lines()
                .filter(|call| *call == "-s emulator-5554 forward --remove tcp:43123")
                .count()
        };
        assert_eq!(removed(), 0, "a live session keeps its forward");
        drop(other);
        assert_eq!(removed(), 1);
        let file = session.file().to_owned();
        drop(session);
        assert_eq!(removed(), 2, "the forward goes with the session");
        assert!(!file.exists(), "the file goes with the session");
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_forward_starts_no_session() {
        use std::os::unix::fs::PermissionsExt as _;
        let project = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        let adb = tools.path().join("adb");
        fs::write(
            &adb,
            "#!/bin/sh\necho 'error: device offline' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&adb, fs::Permissions::from_mode(0o755)).unwrap();

        let refused = Session::create_forwarded(project.path(), None, None, &adb, "emulator-5554");
        assert!(
            matches!(refused, Err(CliError::CommandFailed { .. })),
            "{refused:?}"
        );
        assert!(
            find_session_file(project.path(), None).is_none(),
            "no session file names a port nobody forwards"
        );
    }
}
