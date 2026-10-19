//! The development agent from the CLI's side (ADR-0095 §3).
//!
//! The application serves its semantics tree over a local endpoint
//! (`flui_devtools::agent::AgentServer`, behind the app's `dev-agent`
//! feature). Two commands meet at that endpoint and share nothing but its
//! framing and the credentials in this module:
//!
//! - `flui run --agent` creates a [`Session`]: a fresh endpoint address, a
//!   launch token, and a file naming both. It starts the app with the
//!   endpoint and the token in its environment.
//! - `flui mcp` ([`mcp`]) serves MCP over stdio and forwards each tool call
//!   to that endpoint through a [`client::Connection`].
//!
//! The CLI links no FLUI crate, so the wire (newline-delimited JSON, a hello
//! line first) is spelled out here and in [`client`]; `flui-devtools`' own
//! docs are the other half of the contract, and `commands/run.rs`' env-name
//! test pins the shared names.

pub(crate) mod client;
pub(crate) mod mcp;

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{CliError, CliResult, ResultExt};

/// The environment variable the app reads its endpoint address from.
pub(crate) const ENDPOINT_ENV: &str = "FLUI_AGENT_ENDPOINT";

/// The environment variable the app reads its launch token from.
pub(crate) const TOKEN_ENV: &str = "FLUI_AGENT_TOKEN";

/// The Cargo feature an application declares to host the agent server.
pub(crate) const APP_FEATURE: &str = "dev-agent";

/// The file `flui run --agent` writes beside the build output so
/// `flui mcp` finds the running app without arguments.
const SESSION_FILE: &str = "flui-agent.json";

/// The longest Unix socket path every supported OS accepts (`sun_path` is 104
/// bytes on macOS, 108 on Linux, one of them the terminating NUL).
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 100;

/// Where an app listens and the token that admits a client. Written to the
/// session file, so it is what `flui mcp` reads.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Credentials {
    /// A pipe name on Windows, a socket path elsewhere.
    pub(crate) endpoint: String,
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
            endpoint,
            token,
            pid: 0,
        })
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
/// `CARGO_TARGET_DIR` wins; otherwise the nearest ancestor's `target/` that
/// holds one is the answer for a reader, and `start`'s own workspace root for
/// a writer ([`session_file_for_writing`]).
pub(crate) fn find_session_file(start: &Path) -> Option<PathBuf> {
    if let Some(dir) = target_dir_override() {
        let file = dir.join(SESSION_FILE);
        return file.is_file().then_some(file);
    }
    start
        .ancestors()
        .map(|dir| dir.join("target").join(SESSION_FILE))
        .find(|file| file.is_file())
}

/// Where a session started in `start` writes its file: beside the build
/// output of the workspace `start` belongs to.
fn session_file_for_writing(start: &Path, workspace_root: Option<&Path>) -> PathBuf {
    let target =
        target_dir_override().unwrap_or_else(|| workspace_root.unwrap_or(start).join("target"));
    target.join(SESSION_FILE)
}

fn target_dir_override() -> Option<PathBuf> {
    std::env::var_os("CARGO_TARGET_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// An agent session owned by one `flui run --agent`: the credentials, the
/// private directory holding the socket, and the session file. Dropping it
/// removes all three, so a session never outlives the command.
pub(crate) struct Session {
    credentials: Credentials,
    /// Holds the socket (Unix). Removed on drop.
    _dir: tempfile::TempDir,
    file: PathBuf,
}

impl Session {
    /// A new session for the project at `start` (`workspace_root` is the
    /// workspace it belongs to, when it has one).
    pub(crate) fn create(start: &Path, workspace_root: Option<&Path>) -> CliResult<Self> {
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
        let token = random_hex(32)?;
        let endpoint = endpoint_in(dir.path(), &random_hex(4)?)?;
        let credentials = Credentials {
            endpoint,
            token,
            pid: std::process::id(),
        };
        let file = session_file_for_writing(start, workspace_root);
        write_private(&file, &credentials).context("could not write the agent session file")?;
        Ok(Self {
            credentials,
            _dir: dir,
            file,
        })
    }

    pub(crate) fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The session file `flui mcp` reads.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// Gives `cmd` (the app, or the cargo that runs it) the endpoint and token.
    pub(crate) fn apply(&self, cmd: &mut Command) {
        cmd.env(ENDPOINT_ENV, &self.credentials.endpoint)
            .env(TOKEN_ENV, &self.credentials.token);
    }

    /// Clears what a previous run of the app left at the endpoint: a socket
    /// file a killed app could not remove would fail the next bind.
    pub(crate) fn clear_endpoint(&self) {
        #[cfg(unix)]
        let _ = fs::remove_file(&self.credentials.endpoint);
    }
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
    use super::*;

    #[test]
    fn a_session_names_a_fresh_endpoint_and_a_token_the_app_accepts() {
        let project = tempfile::tempdir().unwrap();
        let session = Session::create(project.path(), None).unwrap();
        let credentials = session.credentials();
        // flui-devtools refuses a token shorter than this.
        assert!(credentials.token.len() >= 32);
        assert!(credentials.token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(credentials.pid, std::process::id());

        let other = Session::create(project.path(), None).unwrap();
        assert_ne!(credentials.token, other.credentials().token);
        assert_ne!(credentials.endpoint, other.credentials().endpoint);
    }

    #[test]
    fn the_session_file_is_what_flui_mcp_discovers_and_goes_with_the_session() {
        let project = tempfile::tempdir().unwrap();
        let nested = project.path().join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let session = Session::create(project.path(), None).unwrap();

        let found = find_session_file(&nested).expect("found from a nested directory");
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
        let session = Session::create(project.path(), None).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(session.file()), 0o600);
        let socket_dir = Path::new(&session.credentials().endpoint).parent().unwrap();
        assert_eq!(
            mode(socket_dir),
            0o700,
            "flui-devtools refuses anything wider"
        );
    }

    #[test]
    fn a_newer_session_keeps_its_file_when_an_older_one_ends() {
        let project = tempfile::tempdir().unwrap();
        let older = Session::create(project.path(), None).unwrap();
        let file = older.file().to_owned();
        // Another `flui run` in the same project replaced the file.
        let replacement = Credentials {
            endpoint: "elsewhere".into(),
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
            endpoint: "e".into(),
            token: "super-secret-token".into(),
            pid: 1,
        };
        assert!(!format!("{credentials:?}").contains("super-secret"));
    }
}
