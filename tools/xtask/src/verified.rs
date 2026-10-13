//! `check-changed`'s record of the trees it verified green, and of the runs
//! waiting in the host lock's queue to verify one.
//!
//! A run is identified by a [`Key`]: a hash of everything that decides its
//! result ([`RunInputs`]): the checkout's whole tree as `git add -A` would
//! stage it (tracked, modified and untracked files, `.gitignore` honoured),
//! the merge base with the base revision, the machine ([`host`]), the
//! toolchain's versions, the Cargo configuration files cargo reads
//! ([`cargo_config`]), every environment variable but the few proven not to
//! change a result ([`result_neutral`]), and the rendered step list. A step
//! this host skips is a line of that list, so a host that skipped wasm32
//! never certifies one that runs it. The key keeps the tree apart from a
//! digest of the rest.
//!
//! Both records live in the per-user directory beside the host lock
//! ([`crate::host_lock::LockSettings::state_dir`]):
//!
//! - `verified/<key>`: written only after every step passed, and only when
//!   the tree is still the one the run started its steps on. The directory
//!   keeps the newest [`CAP`] markers.
//! - `waiting/<start>-<pid>.waiter`: one per run between computing its key
//!   and taking the lock, naming its checkout and key, beside a
//!   `.alive` file the run holds an advisory lock on ([`File::lock`]). The
//!   operating system drops that lock when the run exits, crashed or not, so
//!   a waiter whose `.alive` file can be locked is dead and is ignored (and
//!   removed).
//!
//! [`Verifier::verify`] checks the record before the run takes the lock and
//! again right after (another run may have verified the same key while this
//! one waited), with the run planned again under the lock (the checkout may
//! have changed meanwhile, and the scope with it), then looks for the newest newer waiter from the same checkout
//! with the same inputs other than the tree: if it saw another tree, it will
//! check the newer one, so this run stops without claiming anything
//! ([`Outcome::Superseded`]); if it saw the same tree, this run goes ahead
//! and the newer one will find its marker. A newer waiter asking something
//! else (another base, toolchain, environment or step list) is not this
//! run's concern. Every record is best effort:
//! a directory that cannot be used only costs the saving, never a result.

use std::fmt::{self, Write as _};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

/// The markers kept; older ones are removed when a new one is written.
pub(crate) const CAP: usize = 256;

/// Exit code of a run superseded while it waited.
const SUPERSEDED: u8 = 3;

/// What decides a run's result, hashed into its [`Key`].
#[derive(Debug, Clone)]
pub(crate) struct RunInputs {
    checkout: PathBuf,
    base: String,
    /// The machine: its name, OS and architecture ([`host`]).
    host: String,
    toolchain: String,
    /// The Cargo configuration files cargo reads here ([`cargo_config`]).
    cargo_config: String,
    env: Vec<(String, String)>,
}

impl RunInputs {
    /// This process's inputs for verifying `checkout` against `base`: the
    /// host, toolchain, Cargo configuration and environment are read now, the
    /// tree and merge base by each [`Self::key`].
    pub(crate) fn of_this_run(checkout: &Path, base: &str) -> Self {
        Self {
            checkout: checkout.to_path_buf(),
            base: base.to_owned(),
            host: host(),
            toolchain: toolchain(checkout),
            cargo_config: cargo_config(checkout, cargo_home().as_deref()),
            env: results_env(std::env::vars_os(), checkout),
        }
    }

    /// The key of running `steps` (the rendered plan) on the checkout as it
    /// is now.
    pub(crate) fn key(&self, steps: &str) -> anyhow::Result<Key> {
        let tree = worktree_tree(&self.checkout)?;
        let merge_base = git(&self.checkout, &["merge-base", &self.base, "HEAD"], None)?;
        Ok(self.key_with(&tree, &merge_base, steps))
    }

    /// The key for `steps` on `tree` at `merge_base`, with this run's other
    /// inputs.
    fn key_with(&self, tree: &str, merge_base: &str, steps: &str) -> Key {
        let mut fields = vec![
            ("merge-base", merge_base.trim()),
            ("host", &self.host),
            ("toolchain", &self.toolchain),
            ("cargo-config", &self.cargo_config),
        ];
        fields.extend(
            self.env
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );
        fields.push(("steps", steps));
        Key {
            tree: tree.trim().to_owned(),
            rest: digest(&fields),
        }
    }
}

/// Hex SHA-256 of `fields`, each length-prefixed so no two field lists hash
/// one text.
fn digest(fields: &[(&str, &str)]) -> String {
    let mut hasher = Sha256::new();
    for (name, value) in fields {
        hasher.update(format!("{name}:{}:{}:", name.len(), value.len()));
        hasher.update(value);
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A run's identity: the tree it checks, and a digest of every other input
/// of its [`RunInputs`]. Kept apart so a waiter is superseded only by a run
/// asking the same question of a newer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    /// The git tree object of the checkout's content.
    tree: String,
    /// Digest of the merge base, host, toolchain, Cargo configuration,
    /// environment and steps.
    rest: String,
}

impl Key {
    /// The marker's file name: one digest of both parts.
    fn marker_name(&self) -> String {
        digest(&[("tree", &self.tree), ("rest", &self.rest)])
    }
}

/// The machine a run verifies on: its name, OS and architecture. A marker
/// in a home directory shared between machines certifies only its own.
fn host() -> String {
    let name = ["COMPUTERNAME", "HOSTNAME"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|name| !name.is_empty()))
        .or_else(|| {
            ["/proc/sys/kernel/hostname", "/etc/hostname"]
                .into_iter()
                .find_map(|path| std::fs::read_to_string(path).ok())
        })
        .or_else(|| {
            Command::new("hostname")
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        })
        .unwrap_or_default();
    format!(
        "{} {} {}",
        name.trim().to_ascii_lowercase(),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// `CARGO_HOME`, or `~/.cargo` (`USERPROFILE` on Windows, `HOME` elsewhere).
fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            std::env::var_os(home).map(|home| PathBuf::from(home).join(".cargo"))
        })
}

/// The Cargo configuration files cargo reads for a build in `checkout`, as
/// path and content: `.cargo/config.toml` and `.cargo/config` in the
/// checkout and every directory above it, then those in `cargo_home`. They
/// select compilers, linkers, wrappers, flags and profiles; an absent file
/// adds nothing.
fn cargo_config(checkout: &Path, cargo_home: Option<&Path>) -> String {
    let dirs = checkout
        .ancestors()
        .map(|dir| dir.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf));
    let mut text = String::new();
    for dir in dirs {
        for name in ["config.toml", "config"] {
            let path = dir.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let _ = writeln!(text, "{}:{}:{content}", path.display(), content.len());
            }
        }
    }
    text
}

/// `git <args>` in `dir`, with `index` as `GIT_INDEX_FILE` when given; its
/// trimmed standard output.
fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> anyhow::Result<String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        // never refresh the real index's stat cache as a side effect
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let out = command.output().context("spawning `git`")?;
    if !out.status.success() {
        bail!(
            "`git {}` failed ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The tree object of `checkout` with every change staged, untracked files
/// included: `git add -A` into a copy of the index, then `git write-tree`.
/// The real index is only read.
fn worktree_tree(checkout: &Path) -> anyhow::Result<String> {
    let real = checkout.join(git(checkout, &["rev-parse", "--git-path", "index"], None)?);
    let scratch = tempfile::tempdir().context("a directory for the scratch index")?;
    let index = scratch.path().join("index");
    if real.exists() {
        // the copy keeps the stat cache, so unchanged files are not rehashed
        std::fs::copy(&real, &index).context("copying the index")?;
    }
    git(checkout, &["add", "-A"], Some(&index))?;
    git(checkout, &["write-tree"], Some(&index))
}

/// The toolchain's versions as the checkout's pinned toolchain reports them;
/// a tool that does not answer is recorded as absent.
fn toolchain(checkout: &Path) -> String {
    let tools: [(&str, &[&str]); 5] = [
        ("rustc", &["-vV"]),
        ("cargo", &["-V"]),
        ("cargo", &["clippy", "-V"]),
        ("cargo", &["nextest", "--version"]),
        ("cargo", &["hack", "--version"]),
    ];
    tools
        .iter()
        .fold(String::new(), |mut text, (program, args)| {
            let version = Command::new(program)
                .args(*args)
                .current_dir(checkout)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map_or_else(
                    || "absent".to_owned(),
                    |out| String::from_utf8_lossy(&out.stdout).trim().to_owned(),
                );
            let _ = writeln!(text, "{program} {}: {version}", args.join(" "));
            text
        })
}

/// The variables of `vars` a run's result may depend on: every one but the
/// [`result_neutral`] ones, sorted, with names upper-cased (Windows' names
/// are case-insensitive). The search paths ([`SEARCH_PATHS`]) lose their
/// entries inside `checkout`.
pub(crate) fn results_env(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    checkout: &Path,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vars
        .into_iter()
        .map(|(name, value)| (name.to_string_lossy().to_ascii_uppercase(), value))
        .filter(|(name, _)| !result_neutral(name))
        .map(|(name, value)| {
            let value = if SEARCH_PATHS.contains(&name.as_str()) {
                outside(&value, checkout)
            } else {
                value.to_string_lossy().into_owned()
            };
            (name, value)
        })
        .collect();
    env.sort();
    env
}

/// The program and library search paths. Cargo prepends the checkout's own
/// output directories to them for the program it runs (`cargo xtask` is
/// one), so those entries name the checkout, not a tool; every other entry is
/// keyed, since it picks the linker, `git`, the cross wrappers and the rest.
const SEARCH_PATHS: [&str; 4] = [
    "PATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
];

/// The entries of the search path `value` that lie outside `checkout`,
/// newline-separated.
fn outside(value: &std::ffi::OsStr, checkout: &Path) -> String {
    let spelled = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.trim_end_matches('/');
        if cfg!(windows) {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        }
    };
    let checkout = spelled(checkout);
    std::env::split_paths(value)
        .map(|entry| spelled(&entry))
        .filter(|entry| {
            entry
                .strip_prefix(&checkout)
                .is_none_or(|rest| !rest.is_empty() && !rest.starts_with('/'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the variable `name` (upper-case) is proven not to change what a
/// run builds, tests or reports. Everything else is keyed, so a variable
/// nobody has looked at, a test control such as `PROPTEST_CASES` among them,
/// misses the cache instead of reusing a result it may not share.
///
/// Kept out of the key, each for its reason:
///
/// - job and thread counts (`CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS`,
///   `NUMBER_OF_PROCESSORS`) and make's jobserver handles (`CARGO_MAKEFLAGS`,
///   `MAKEFLAGS`, `MFLAGS`, a new pipe per process): the suite must pass at
///   any parallelism, and the host lock sets the counts itself
///   ([`crate::host_lock::Parallelism`]);
/// - the host lock's own controls (`FLUI_XTASK_LOCK_FILE`,
///   `FLUI_XTASK_NO_LOCK`, `FLUI_XTASK_LOCK_HELD`): they decide whom the run
///   waits for, not what it runs;
/// - cargo's own terminal styling (`CARGO_TERM_*`): read by cargo and
///   nextest for their progress output, by nothing a test runs (the
///   application-level switches `NO_COLOR`, `CLICOLOR_FORCE` and `TERM` are
///   read by `flui-log` and `flui-cli`, so they are keyed);
/// - what cargo sets for xtask itself (`CARGO`, `CARGO_MANIFEST_DIR`,
///   `CARGO_MANIFEST_PATH`, `CARGO_PKG_*`, `CARGO_CRATE_NAME`,
///   `CARGO_BIN_NAME`, `CARGO_PRIMARY_PACKAGE`, `CARGO_RUSTC_CURRENT_DIR`):
///   they describe the xtask package of whichever checkout runs it, and cargo
///   sets its own for every crate it compiles and every test it runs;
/// - locations whose content is keyed elsewhere: `CARGO_HOME` (its
///   configuration is keyed by content, [`cargo_config`]), `CARGO_TARGET_DIR`
///   (where the outputs go; `check-changed` refuses one outside the
///   checkout) and rustup's controls (`RUSTUP_*`: the toolchain they select
///   is keyed by its versions, which [`toolchain`] asks with this
///   environment);
/// - the shell's bookkeeping (`PWD`, `OLDPWD`, `SHLVL`, `_`): every step runs
///   from the repository root;
/// - per-session and per-process identifiers, new for every terminal, trace
///   or agent session: `WT_SESSION`, `TERM_SESSION_ID`, `BAGGAGE`,
///   `SENTRY-TRACE`, `CHROME_CRASHPAD_PIPE_NAME`, and the agent harness's
///   `CLAUDECODE` and `CLAUDE_*` (its own settings and session identity; no
///   code in the workspace reads them).
///
/// `PATH` and the library search paths are keyed ([`SEARCH_PATHS`]): the
/// toolchain's versions cover rustc, cargo, clippy, nextest and cargo-hack,
/// not the linker, `git` or the cross wrappers a step runs.
fn result_neutral(name: &str) -> bool {
    const NEUTRAL: [&str; 28] = [
        "CARGO_BUILD_JOBS",
        "NEXTEST_TEST_THREADS",
        "NUMBER_OF_PROCESSORS",
        "CARGO_MAKEFLAGS",
        "MAKEFLAGS",
        "MFLAGS",
        "FLUI_XTASK_LOCK_FILE",
        "FLUI_XTASK_NO_LOCK",
        "FLUI_XTASK_LOCK_HELD",
        "CARGO",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_RUSTC_CURRENT_DIR",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "PWD",
        "OLDPWD",
        "SHLVL",
        "_",
        "WT_SESSION",
        "TERM_SESSION_ID",
        "CLAUDECODE",
        "BAGGAGE",
        "SENTRY-TRACE",
        "CHROME_CRASHPAD_PIPE_NAME",
    ];
    const NEUTRAL_PREFIXES: [&str; 4] = ["CARGO_TERM_", "CARGO_PKG_", "RUSTUP_", "CLAUDE_"];
    NEUTRAL.contains(&name)
        || NEUTRAL_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// A verified run, as a later run names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verified {
    pid: u32,
    /// Seconds since the Unix epoch.
    at: u64,
    checkout: String,
}

impl Verified {
    fn this_run(checkout: &str) -> Self {
        Self {
            pid: std::process::id(),
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            checkout: checkout.to_owned(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        Some(Self {
            pid: lines.next()?.parse().ok()?,
            at: lines.next()?.parse().ok()?,
            checkout: lines.next()?.to_owned(),
        })
    }
}

impl fmt::Display for Verified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ago = SystemTime::now()
            .duration_since(UNIX_EPOCH + Duration::from_secs(self.at))
            .map_or(0, |ago| ago.as_secs());
        let ago = match ago {
            0..60 => format!("{ago} s"),
            60..7200 => format!("{} min", ago / 60),
            _ => format!("{} h", ago / 3600),
        };
        write!(
            f,
            "the run in {} (pid {}), {ago} ago",
            self.checkout, self.pid
        )
    }
}

/// The `verified/` markers.
#[derive(Debug, Clone)]
struct Store {
    dir: PathBuf,
}

impl Store {
    fn marker(&self, key: &Key) -> PathBuf {
        self.dir.join(key.marker_name())
    }

    fn lookup(&self, key: &Key) -> Option<Verified> {
        Verified::parse(&std::fs::read_to_string(self.marker(key)).ok()?)
    }

    /// Writes the marker through a staged file renamed into place, so a
    /// reader sees a whole one or none, then prunes beyond [`CAP`].
    fn record(&self, key: &Key, run: &Verified) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let staged = self
            .dir
            .join(format!("{}.{}.tmp", key.marker_name(), std::process::id()));
        let text = format!("{}\n{}\n{}\n", run.pid, run.at, run.checkout);
        std::fs::write(&staged, text)
            .and_then(|()| std::fs::rename(&staged, self.marker(key)))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&staged);
            })?;
        self.prune(&self.marker(key));
        Ok(())
    }

    /// Removes the oldest markers beyond [`CAP`], never `written` (markers
    /// written within one clock tick share a time).
    fn prune(&self, written: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut markers: Vec<(SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_none() && entry.path() != written)
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        if markers.len() < CAP {
            return;
        }
        markers.sort();
        for (_, path) in &markers[..=markers.len() - CAP] {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A run's place in the `waiting/` registry, removed on drop.
#[derive(Debug)]
struct Ticket {
    dir: PathBuf,
    order: (u128, u32),
    /// Holds the advisory lock that says this waiter is alive.
    alive: Option<File>,
}

/// A waiter another run registered.
#[derive(Debug)]
struct Waiter {
    order: (u128, u32),
    checkout: String,
    key: Key,
}

fn stem((started, pid): (u128, u32)) -> String {
    format!("{started:039}-{pid}")
}

impl Ticket {
    /// Registers this process as waiting to verify `key` for `checkout`.
    fn register(dir: &Path, checkout: &str, key: &Key) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let pid = std::process::id();
        let mut started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let alive = loop {
            let path = dir.join(format!("{}.alive", stem((started, pid))));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => break file,
                // two registrations from this process within the clock's tick
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => started += 1,
                Err(error) => return Err(error),
            }
        };
        // From here on, dropping the ticket removes whatever was created.
        let ticket = Self {
            dir: dir.to_path_buf(),
            order: (started, pid),
            alive: Some(alive),
        };
        if let Some(alive) = &ticket.alive {
            alive.lock()?;
        }
        // The `.waiter` file appears only once the `.alive` lock is held, so
        // a reader never mistakes a registering run for a dead one.
        let staged = ticket.path("waiter.tmp");
        std::fs::write(&staged, format!("{checkout}\n{}\n{}\n", key.tree, key.rest))?;
        std::fs::rename(&staged, ticket.path("waiter"))?;
        Ok(ticket)
    }

    fn path(&self, extension: &str) -> PathBuf {
        self.dir.join(format!("{}.{extension}", stem(self.order)))
    }

    /// The newest live waiter registered after this one that asks the same
    /// question of `checkout`: the same inputs but maybe another tree.
    fn newest_after(&self, checkout: &str, key: &Key) -> Option<Waiter> {
        live_waiters(&self.dir)
            .into_iter()
            .filter(|waiter| {
                waiter.order > self.order
                    && waiter.checkout == checkout
                    && waiter.key.rest == key.rest
            })
            .max_by_key(|waiter| waiter.order)
    }

    /// Leaves the files behind with no live owner, as a run that crashed
    /// while waiting does.
    #[cfg(test)]
    fn abandon(mut self) {
        drop(self.alive.take());
        std::mem::forget(self);
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.path("waiter"));
        let _ = std::fs::remove_file(self.path("alive"));
        drop(self.alive.take());
    }
}

/// The waiters in `dir` whose owner is alive; a dead one's files are removed.
fn live_waiters(dir: &Path) -> Vec<Waiter> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "waiter"))
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?;
            let (started, pid) = name.split_once('-')?;
            let order = (started.parse().ok()?, pid.parse().ok()?);
            let alive = path.with_extension("alive");
            let live = File::open(&alive).is_ok_and(|file| match file.try_lock() {
                Err(TryLockError::WouldBlock) => true,
                Ok(()) => {
                    let _ = file.unlock();
                    false
                }
                Err(TryLockError::Error(_)) => false,
            });
            if !live {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(&alive);
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            let mut lines = text.lines();
            Some(Waiter {
                order,
                checkout: lines.next()?.to_owned(),
                key: Key {
                    tree: lines.next()?.to_owned(),
                    rest: lines.next()?.to_owned(),
                },
            })
        })
        .collect()
}

/// How a [`Verifier::verify`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The steps ran and passed.
    Passed,
    /// An earlier run verified the same key; nothing ran.
    AlreadyVerified(Verified),
    /// A newer run of the same checkout will check another tree; nothing
    /// ran and nothing is claimed.
    Superseded,
}

impl Outcome {
    /// Success for a verified tree, [`SUPERSEDED`] otherwise.
    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Self::Passed | Self::AlreadyVerified(_) => ExitCode::SUCCESS,
            Self::Superseded => ExitCode::from(SUPERSEDED),
        }
    }
}

/// Runs a verification once per key: see the module docs.
pub(crate) struct Verifier<'a> {
    /// The directory beside the host lock; `None` keeps no records.
    dir: Option<PathBuf>,
    checkout: String,
    /// `false` (`--no-cache`) runs even when the key is recorded.
    use_cache: bool,
    out: &'a mut dyn Write,
}

/// `checkout` as waiters compare it and markers name it: canonical, without
/// the verbatim prefix Windows' canonical form carries (the plain path names
/// the same directory and reads as the user typed it).
fn checkout_name(checkout: &Path) -> String {
    let canonical = checkout
        .canonicalize()
        .unwrap_or_else(|_| checkout.to_path_buf());
    let name = canonical.display().to_string();
    name.strip_prefix(r"\\?\").unwrap_or(&name).to_owned()
}

impl<'a> Verifier<'a> {
    /// Records under `dir` (beside the host lock) for `checkout`.
    pub(crate) fn new(
        dir: Option<&Path>,
        checkout: &Path,
        use_cache: bool,
        out: &'a mut dyn Write,
    ) -> Self {
        Self {
            dir: dir.map(Path::to_path_buf),
            checkout: checkout_name(checkout),
            use_cache,
            out,
        }
    }

    fn store(&self) -> Option<Store> {
        self.dir.as_ref().map(|dir| Store {
            dir: dir.join("verified"),
        })
    }

    fn cached(&mut self, key: &Key) -> Option<Verified> {
        let found = self
            .store()
            .filter(|_| self.use_cache)
            .and_then(|store| store.lookup(key))?;
        let _ = writeln!(
            self.out,
            "check-changed: already verified green by {found} (same tree, base, toolchain, \
             environment and steps); nothing to run (--no-cache runs it anyway)"
        );
        Some(found)
    }

    fn note(&mut self, text: impl fmt::Display) {
        let _ = writeln!(self.out, "check-changed: {text}");
    }

    /// Takes the lock with `lock`, plans the run with `plan` and runs the
    /// planned steps with `run`, skipping what a record makes unnecessary.
    ///
    /// `planned` is the plan made before the lock: its key only probes the
    /// record (and registers the waiter), so it can spare the wait but never
    /// decides what runs. The checkout may change while the run waits, moving
    /// the scope, so the run is planned again under the lock, and both the
    /// steps run and the key recorded come from that plan. `key` is asked of
    /// it again after `run` passed: the marker is written only when the two
    /// agree.
    pub(crate) fn verify<P, G>(
        mut self,
        planned: &P,
        plan: impl FnOnce() -> anyhow::Result<P>,
        mut key: impl FnMut(&P) -> anyhow::Result<Key>,
        lock: impl FnOnce() -> G,
        run: impl FnOnce(&P) -> anyhow::Result<()>,
    ) -> anyhow::Result<Outcome> {
        let before = match key(planned) {
            Ok(before) => before,
            Err(error) => {
                self.note(format_args!(
                    "cannot compute the run's key ({error:#}); running uncached"
                ));
                let _guard = lock();
                run(&plan()?)?;
                return Ok(Outcome::Passed);
            }
        };
        if let Some(found) = self.cached(&before) {
            return Ok(Outcome::AlreadyVerified(found));
        }
        let ticket = self
            .dir
            .as_ref()
            .and_then(|dir| Ticket::register(&dir.join("waiting"), &self.checkout, &before).ok());
        let _guard = lock();
        let plan = plan()?;
        let now = match key(&plan) {
            Ok(now) => now,
            Err(error) => {
                drop(ticket);
                self.note(format_args!(
                    "cannot compute the run's key ({error:#}); running uncached"
                ));
                run(&plan)?;
                return Ok(Outcome::Passed);
            }
        };
        if let Some(found) = self.cached(&now) {
            return Ok(Outcome::AlreadyVerified(found));
        }
        let newer = ticket
            .as_ref()
            .and_then(|ticket| ticket.newest_after(&self.checkout, &now));
        drop(ticket);
        if newer.is_some_and(|newer| newer.key.tree != now.tree) {
            self.note(
                "superseded by a newer check-changed of this checkout; this tree was not verified",
            );
            return Ok(Outcome::Superseded);
        }
        run(&plan)?;
        match key(&plan) {
            Ok(after) if after == now => {
                if let Some(store) = self.store()
                    && let Err(error) = store.record(&now, &Verified::this_run(&self.checkout))
                {
                    self.note(format_args!("could not record the verified tree: {error}"));
                }
            }
            Ok(_) => self.note("the checkout changed during the run; not recorded as verified"),
            Err(error) => self.note(format_args!(
                "cannot compute the run's key ({error:#}); not recorded as verified"
            )),
        }
        Ok(Outcome::Passed)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;

    use super::*;
    use crate::table_test::run_table;

    /// A key for tree `tree`, with the other inputs every case shares.
    fn key(tree: &str) -> Key {
        asking(tree, "inputs")
    }

    /// A key for tree `tree` with the other inputs digested to `rest`.
    fn asking(tree: &str, rest: &str) -> Key {
        Key {
            tree: tree.to_owned(),
            rest: rest.to_owned(),
        }
    }

    fn inputs() -> RunInputs {
        RunInputs {
            checkout: PathBuf::from("checkout"),
            base: "origin/main".to_owned(),
            host: "builder windows x86_64".to_owned(),
            toolchain: "rustc 1.99.0".to_owned(),
            cargo_config: String::new(),
            env: Vec::new(),
        }
    }

    /// The rendered plan [`inputs`]' keys run.
    const STEPS: &str = "$ cargo clippy -p a";

    /// A fresh record directory and checkout.
    struct Case {
        dir: tempfile::TempDir,
        checkout: tempfile::TempDir,
    }

    impl Case {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("temp dir"),
                checkout: tempfile::tempdir().expect("temp checkout"),
            }
        }

        fn checkout(&self) -> String {
            checkout_name(self.checkout.path())
        }

        fn store(&self) -> Store {
            Store {
                dir: self.dir.path().join("verified"),
            }
        }

        fn waiting(&self) -> PathBuf {
            self.dir.path().join("waiting")
        }

        /// Runs `verify`: `keys` are what `key` returns in turn (the last
        /// repeats), `during_lock` runs as the lock is taken (another run's
        /// actions while this one waited). The outcome, whether the steps
        /// ran, and what was printed.
        fn verify(
            &self,
            use_cache: bool,
            keys: &[&str],
            during_lock: impl FnOnce(),
            result: anyhow::Result<()>,
        ) -> (anyhow::Result<Outcome>, bool, String) {
            let (outcome, ran, printed) =
                self.verify_plans(use_cache, keys, ["inputs"; 2], during_lock, result);
            (outcome, ran.is_some(), printed)
        }

        /// [`Self::verify`] with the plan made before the lock and the one
        /// made under it (a key's other inputs); which plan ran, if any.
        fn verify_plans(
            &self,
            use_cache: bool,
            keys: &[&str],
            [before, under_lock]: [&str; 2],
            during_lock: impl FnOnce(),
            result: anyhow::Result<()>,
        ) -> (anyhow::Result<Outcome>, Option<String>, String) {
            let asked = Cell::new(0);
            let ran = RefCell::new(None);
            let mut out = Vec::new();
            let outcome = Verifier::new(
                Some(self.dir.path()),
                self.checkout.path(),
                use_cache,
                &mut out,
            )
            .verify(
                &before.to_owned(),
                || Ok(under_lock.to_owned()),
                |plan| {
                    let index = asked.get().min(keys.len() - 1);
                    asked.set(asked.get() + 1);
                    Ok(asking(keys[index], plan))
                },
                during_lock,
                |plan| {
                    *ran.borrow_mut() = Some(plan.clone());
                    result
                },
            );
            (
                outcome,
                ran.into_inner(),
                String::from_utf8_lossy(&out).into_owned(),
            )
        }
    }

    fn a_recorded_key_skips_the_run() {
        let case = Case::new();
        let earlier = Verified {
            pid: 42,
            at: 0,
            checkout: "D:/earlier".to_owned(),
        };
        case.store().record(&key("k"), &earlier).expect("record");
        let locked = Cell::new(false);
        let (outcome, ran, printed) = case.verify(true, &["k"], || locked.set(true), Ok(()));
        assert_eq!(
            outcome.expect("verified"),
            Outcome::AlreadyVerified(earlier)
        );
        assert!(!ran && !locked.get(), "nothing runs and nothing waits");
        assert!(
            printed.contains("already verified green by the run in D:/earlier (pid 42)"),
            "{printed}"
        );
    }

    fn a_key_verified_while_waiting_skips_the_run() {
        let case = Case::new();
        let store = case.store();
        let (outcome, ran, _) = case.verify(
            true,
            &["k"],
            || {
                store
                    .record(&key("k"), &Verified::this_run("elsewhere"))
                    .expect("record");
            },
            Ok(()),
        );
        assert!(
            matches!(outcome, Ok(Outcome::AlreadyVerified(_))),
            "{outcome:?}"
        );
        assert!(!ran);
    }

    fn a_passing_run_is_recorded_and_no_cache_runs_anyway() {
        let case = Case::new();
        let (outcome, ran, _) = case.verify(true, &["k"], || (), Ok(()));
        assert_eq!(outcome.expect("passes"), Outcome::Passed);
        assert!(ran);
        let recorded = case.store().lookup(&key("k")).expect("the key is recorded");
        assert_eq!(recorded.checkout, case.checkout());
        let (outcome, ran, _) = case.verify(false, &["k"], || (), Ok(()));
        assert_eq!(outcome.expect("passes"), Outcome::Passed);
        assert!(ran, "--no-cache runs a recorded key");
    }

    fn a_failing_run_is_not_recorded() {
        let case = Case::new();
        let (outcome, ran, _) =
            case.verify(true, &["k"], || (), Err(anyhow::anyhow!("clippy failed")));
        assert!(outcome.is_err() && ran);
        assert!(case.store().lookup(&key("k")).is_none());
    }

    fn a_tree_changed_during_the_run_is_not_recorded() {
        let case = Case::new();
        let (outcome, _, printed) = case.verify(true, &["k", "k", "edited"], || (), Ok(()));
        assert_eq!(outcome.expect("passes"), Outcome::Passed);
        for recorded in ["k", "edited"] {
            assert!(case.store().lookup(&key(recorded)).is_none(), "{recorded}");
        }
        assert!(printed.contains("changed during the run"), "{printed}");
    }

    /// An edit while the run waited moved the scope to a crate the first
    /// plan left out: the run checks the new scope, and the marker names
    /// the new tree with the new steps.
    fn a_checkout_changed_while_waiting_runs_the_new_plan() {
        let case = Case::new();
        let (outcome, ran, _) = case.verify_plans(
            true,
            &["old", "edited"],
            ["$ cargo clippy -p a", "$ cargo clippy -p a -p b"],
            || (),
            Ok(()),
        );
        assert_eq!(outcome.expect("passes"), Outcome::Passed);
        assert_eq!(ran.as_deref(), Some("$ cargo clippy -p a -p b"));
        let store = case.store();
        assert!(
            store
                .lookup(&asking("edited", "$ cargo clippy -p a -p b"))
                .is_some(),
            "the new tree is recorded with the steps it ran"
        );
        for (tree, steps) in [
            ("edited", "$ cargo clippy -p a"),
            ("old", "$ cargo clippy -p a"),
        ] {
            assert!(
                store.lookup(&asking(tree, steps)).is_none(),
                "{tree}: {steps}"
            );
        }
    }

    fn a_newer_waiter_with_another_key_supersedes() {
        let case = Case::new();
        let newer = RefCell::new(None);
        let (outcome, ran, printed) = case.verify(
            true,
            &["old"],
            || {
                *newer.borrow_mut() = Some(
                    Ticket::register(&case.waiting(), &case.checkout(), &key("new"))
                        .expect("register"),
                );
            },
            Ok(()),
        );
        let outcome = outcome.expect("an outcome");
        assert_eq!(outcome, Outcome::Superseded);
        assert_ne!(outcome.exit_code(), ExitCode::SUCCESS);
        assert!(!ran);
        assert!(
            printed.contains(
                "superseded by a newer check-changed of this checkout; this tree was not verified"
            ),
            "{printed}"
        );
        assert!(
            case.store().lookup(&key("old")).is_none(),
            "nothing is claimed"
        );
    }

    /// Newer waiters that do not supersede: one with the same key, one from
    /// another checkout, one asking another question (another base,
    /// toolchain, environment or step list) of a newer tree, and one that
    /// died while waiting.
    fn other_newer_waiters_let_the_run_proceed() {
        type Newer = fn(&Case) -> Option<Ticket>;
        let rows: [(&str, Newer); 4] = [
            ("another_question", |case| {
                Some(
                    Ticket::register(&case.waiting(), &case.checkout(), &asking("new", "release"))
                        .expect("register"),
                )
            }),
            ("same_key", |case| {
                Some(
                    Ticket::register(&case.waiting(), &case.checkout(), &key("k"))
                        .expect("register"),
                )
            }),
            ("another_checkout", |case| {
                Some(Ticket::register(&case.waiting(), "elsewhere", &key("new")).expect("register"))
            }),
            ("dead", |case| {
                Ticket::register(&case.waiting(), &case.checkout(), &key("new"))
                    .expect("register")
                    .abandon();
                None
            }),
        ];
        for (row, newer) in rows {
            let case = Case::new();
            let held = RefCell::new(None);
            let (outcome, ran, _) =
                case.verify(true, &["k"], || *held.borrow_mut() = newer(&case), Ok(()));
            assert_eq!(outcome.expect("passes"), Outcome::Passed, "{row}");
            assert!(ran, "{row}");
        }
    }

    #[test]
    fn verify_contract() {
        run_table(
            "verify_contract",
            &[
                ("a_recorded_key_skips_the_run", a_recorded_key_skips_the_run),
                (
                    "a_key_verified_while_waiting_skips_the_run",
                    a_key_verified_while_waiting_skips_the_run,
                ),
                (
                    "a_passing_run_is_recorded_and_no_cache_runs_anyway",
                    a_passing_run_is_recorded_and_no_cache_runs_anyway,
                ),
                (
                    "a_failing_run_is_not_recorded",
                    a_failing_run_is_not_recorded,
                ),
                (
                    "a_tree_changed_during_the_run_is_not_recorded",
                    a_tree_changed_during_the_run_is_not_recorded,
                ),
                (
                    "a_checkout_changed_while_waiting_runs_the_new_plan",
                    a_checkout_changed_while_waiting_runs_the_new_plan,
                ),
                (
                    "a_newer_waiter_with_another_key_supersedes",
                    a_newer_waiter_with_another_key_supersedes,
                ),
                (
                    "other_newer_waiters_let_the_run_proceed",
                    other_newer_waiters_let_the_run_proceed,
                ),
            ],
        );
    }

    /// A git repository in a temporary directory with one commit.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp repo");
        let git = |args: &[&str]| {
            git(dir.path(), args, None).expect("git");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.path().join(".gitignore"), "ignored\n").expect("write");
        std::fs::write(dir.path().join("tracked"), "one\n").expect("write");
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=xtask",
            "-c",
            "user.email=xtask@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            "init",
        ]);
        dir
    }

    /// The tree counts every file `git add -A` would stage, and leaves the
    /// real index as it was.
    #[test]
    fn the_tree_covers_uncommitted_and_untracked_files() {
        let repo = repo();
        let path = repo.path();
        let index = path.join(".git").join("index");
        let index_before = std::fs::read(&index).expect("index");
        let clean = worktree_tree(path).expect("tree");
        assert_eq!(
            clean,
            git(path, &["rev-parse", "HEAD^{tree}"], None).expect("HEAD's tree")
        );
        std::fs::write(path.join("ignored"), "x").expect("write");
        assert_eq!(
            worktree_tree(path).expect("tree"),
            clean,
            "ignored files do not count"
        );
        std::fs::write(path.join("tracked"), "two\n").expect("write");
        let edited = worktree_tree(path).expect("tree");
        assert_ne!(edited, clean, "an uncommitted edit counts");
        std::fs::write(path.join("untracked"), "new\n").expect("write");
        assert_ne!(
            worktree_tree(path).expect("tree"),
            edited,
            "an untracked file counts"
        );
        assert_eq!(
            std::fs::read(&index).expect("index"),
            index_before,
            "the real index is untouched"
        );
        assert!(
            git(path, &["status", "--porcelain"], None)
                .expect("status")
                .contains("?? untracked"),
            "nothing was staged"
        );
    }

    /// Every input changes the key.
    #[test]
    fn each_input_changes_the_key() {
        let base = inputs().key_with("tree", "base", STEPS);
        type Row = (&'static str, fn() -> Key);
        let rows: [Row; 7] = [
            ("tree", || inputs().key_with("other", "base", STEPS)),
            ("host", || {
                RunInputs {
                    host: "other-builder windows x86_64".to_owned(),
                    ..inputs()
                }
                .key_with("tree", "base", STEPS)
            }),
            ("cargo_config", || {
                RunInputs {
                    cargo_config: "[build]\nrustc-wrapper = \"sccache\"\n".to_owned(),
                    ..inputs()
                }
                .key_with("tree", "base", STEPS)
            }),
            ("merge_base", || inputs().key_with("tree", "other", STEPS)),
            ("toolchain", || {
                RunInputs {
                    toolchain: "rustc 1.100.0".to_owned(),
                    ..inputs()
                }
                .key_with("tree", "base", STEPS)
            }),
            ("env", || {
                RunInputs {
                    env: vec![("RUSTFLAGS".to_owned(), "-Dwarnings".to_owned())],
                    ..inputs()
                }
                .key_with("tree", "base", STEPS)
            }),
            ("steps", || {
                inputs().key_with(
                    "tree",
                    "base",
                    "$ cargo clippy -p a\ncheck-changed: skipped wasm32",
                )
            }),
        ];
        let same: Vec<&str> = rows
            .iter()
            .filter(|(_, changed)| changed() == base)
            .map(|(row, _)| *row)
            .collect();
        assert!(
            same.is_empty(),
            "inputs that leave the key unchanged: {same:?}"
        );
        assert_eq!(
            inputs().key_with("tree", "base", STEPS),
            base,
            "the key is deterministic"
        );
    }

    /// Every inherited variable is keyed unless it is proven result-neutral:
    /// a test control or a variable nobody listed misses the cache.
    #[test]
    fn every_variable_but_the_neutral_ones_is_keyed() {
        let checkout = Path::new("/work/flui");
        let key_of = |vars: &[(&str, &OsString)]| {
            let vars = vars
                .iter()
                .map(|(name, value)| (OsString::from(name), (*value).clone()));
            RunInputs {
                env: results_env(vars, checkout),
                ..inputs()
            }
            .key_with("tree", "base", STEPS)
        };
        let set = OsString::from("1");
        let unset = key_of(&[]);
        let keyed = [
            "PROPTEST_CASES",
            "XTASK_TEST_UNLISTED_VARIABLE",
            "RUSTFLAGS",
            "CARGO_PROFILE_DEV_DEBUG",
            "FLUI_HEADLESS",
            "NEXTEST_PROFILE",
            "CC_AARCH64_LINUX_ANDROID",
            "RUST_LOG",
            "NO_COLOR",
            "CLICOLOR_FORCE",
            "PATH",
        ];
        let neutral = [
            "CARGO_BUILD_JOBS",
            "NEXTEST_TEST_THREADS",
            "CARGO_MAKEFLAGS",
            "FLUI_XTASK_NO_LOCK",
            "CARGO_TERM_COLOR",
            "CARGO_MANIFEST_DIR",
            "CARGO_PKG_VERSION",
            "CARGO_TARGET_DIR",
            "RUSTUP_TOOLCHAIN",
            "SHLVL",
            "WT_SESSION",
            "CLAUDE_CODE_SESSION_ID",
        ];
        let unkeyed: Vec<&str> = keyed
            .into_iter()
            .filter(|name| key_of(&[(name, &set)]) == unset)
            .collect();
        assert!(unkeyed.is_empty(), "not keyed: {unkeyed:?}");
        let misses: Vec<&str> = neutral
            .into_iter()
            .filter(|name| key_of(&[(name, &set)]) != unset)
            .collect();
        assert!(misses.is_empty(), "neutral but keyed: {misses:?}");
        assert_eq!(
            key_of(&[("proptest_cases", &set)]),
            key_of(&[("PROPTEST_CASES", &set)]),
            "names are case-insensitive"
        );
        // A search path keys every entry but the checkout's own outputs.
        let path = |entries: &[&str]| std::env::join_paths(entries).expect("joinable");
        let system = path(&["/usr/bin"]);
        assert_eq!(
            key_of(&[("PATH", &path(&["/work/flui/target/debug/deps", "/usr/bin"]))]),
            key_of(&[("PATH", &system)]),
        );
        for other in [
            path(&["/opt/llvm/bin", "/usr/bin"]),
            path(&["/work/flui-other/bin", "/usr/bin"]),
        ] {
            assert_ne!(
                key_of(&[("PATH", &other)]),
                key_of(&[("PATH", &system)]),
                "{other:?}"
            );
        }
    }

    /// The configuration cargo reads: the checkout's, every parent's and
    /// the Cargo home's, each by content.
    #[test]
    fn cargo_configuration_on_the_discovery_path_is_keyed() {
        let root = tempfile::tempdir().expect("temp dir");
        let checkout = root.path().join("parent").join("checkout");
        let home = root.path().join("cargo-home");
        let write = |path: &Path, text: &str| {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        };
        write(&checkout.join(".cargo/config.toml"), "[build]\nown = 1\n");
        write(
            &root.path().join("parent/.cargo/config"),
            "[target.x]\nlinker = \"a\"\n",
        );
        write(
            &home.join("config.toml"),
            "[build]\nxtask-test-marker = 16\n",
        );
        let config = cargo_config(&checkout, Some(&home));
        for text in ["own = 1", "linker = \"a\"", "xtask-test-marker = 16"] {
            assert!(config.contains(text), "`{text}` is read: {config}");
        }
        write(
            &root.path().join("parent/.cargo/config"),
            "[target.x]\nlinker = \"b\"\n",
        );
        assert_ne!(
            cargo_config(&checkout, Some(&home)),
            config,
            "a parent directory's edit counts"
        );
        assert!(!cargo_config(&checkout, None).contains("xtask-test-marker = 16"));
        assert!(
            host().ends_with(&format!(
                " {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            "the host names its OS and architecture"
        );
    }

    #[test]
    fn the_store_keeps_the_newest_markers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store {
            dir: dir.path().to_path_buf(),
        };
        for index in 0..=CAP {
            store
                .record(&key(&format!("{index:04}")), &Verified::this_run("c"))
                .expect("record");
        }
        let kept = std::fs::read_dir(dir.path()).expect("dir").count();
        assert_eq!(kept, CAP);
        assert!(
            store.lookup(&key(&format!("{CAP:04}"))).is_some(),
            "the newest is kept"
        );
    }
}
