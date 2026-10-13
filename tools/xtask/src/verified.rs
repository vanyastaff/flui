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
//!   the checkout is still the one snapshotted under the lock before the run
//!   was planned. The directory keeps the newest [`CAP`] markers.
//! - `waiting/<start>-<pid>.waiter`: one per run between computing its key
//!   and taking the lock, naming its checkout and key, beside a
//!   `.alive` file the run holds an advisory lock on ([`File::lock`]). The
//!   operating system drops that lock when the run exits, crashed or not, so
//!   a waiter whose `.alive` file can be locked is dead and is ignored (and
//!   removed).
//!
//! [`Verifier::verify`] checks the record before the run takes the lock and
//! again right after (another run may have verified the same key while this
//! one waited), under the lock snapshotting the checkout first and planning
//! it again ([`Checks::replan`]) if it changed while the run waited, then
//! looks for the newest newer waiter from the same checkout with the same
//! inputs other than the tree: if it saw another tree, it will
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

/// The checkout's content at one moment: its tree with every change staged
/// and its merge base with the base revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    tree: String,
    merge_base: String,
}

impl Snapshot {
    /// `checkout` as it is now, against `base`.
    pub(crate) fn of(checkout: &Path, base: &str) -> anyhow::Result<Self> {
        Ok(Self {
            tree: worktree_tree(checkout)?,
            merge_base: git(checkout, &["merge-base", base, "HEAD"], None)?,
        })
    }
}

/// What decides a run's result besides its [`Snapshot`] and steps, hashed
/// into its [`Key`].
#[derive(Debug, Clone)]
pub(crate) struct RunInputs {
    /// The machine: its name, OS and architecture ([`host`]).
    host: String,
    toolchain: String,
    /// The Cargo configuration files cargo reads here ([`cargo_config`]).
    cargo_config: Vec<u8>,
    /// Names and values as the platform encodes them ([`results_env`]).
    env: Vec<(String, Vec<u8>)>,
}

impl RunInputs {
    /// This process's inputs for verifying `checkout`, read now.
    pub(crate) fn of_this_run(checkout: &Path) -> Self {
        // Cargo prepends the directory it ran this binary from, and its
        // subdirectories, to the library search path.
        let exe = std::env::current_exe().ok();
        let cargo_outputs = exe.as_deref().and_then(Path::parent);
        Self {
            host: host(),
            toolchain: toolchain(checkout),
            cargo_config: cargo_config(
                checkout,
                cargo_home(checkout, |name| std::env::var_os(name)).as_deref(),
            ),
            env: results_env(std::env::vars_os(), cargo_outputs, cfg!(windows)),
        }
    }

    /// The key of running `steps` (the rendered plan) on `snapshot`.
    pub(crate) fn key(&self, snapshot: &Snapshot, steps: &str) -> Key {
        let mut fields = vec![
            ("merge-base", snapshot.merge_base.trim().as_bytes()),
            ("host", self.host.as_bytes()),
            ("toolchain", self.toolchain.as_bytes()),
            ("cargo-config", self.cargo_config.as_slice()),
        ];
        fields.extend(
            self.env
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_slice())),
        );
        fields.push(("steps", steps.as_bytes()));
        Key {
            tree: snapshot.tree.trim().to_owned(),
            rest: digest(&fields),
        }
    }
}

/// Hex SHA-256 of `fields`, each encoded as the name's length, the name, the
/// value's length and the value, so no two field lists hash one text.
fn digest(fields: &[(&str, &[u8])]) -> String {
    let mut hasher = Sha256::new();
    for (name, value) in fields {
        hasher.update(format!("{}:{name}:{}:", name.len(), value.len()));
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
        digest(&[
            ("tree", self.tree.as_bytes()),
            ("rest", self.rest.as_bytes()),
        ])
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

/// `CARGO_HOME`, or `~/.cargo` (`USERPROFILE` on Windows, `HOME` elsewhere),
/// with `var` reading the environment. A relative one is resolved against
/// `checkout`, the directory every step's cargo runs in, wherever xtask
/// itself was started.
fn cargo_home(
    checkout: &Path,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    var("CARGO_HOME")
        .filter(|home| !home.is_empty())
        .map(|home| checkout.join(home))
        .or_else(|| {
            let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            var(home).map(|home| PathBuf::from(home).join(".cargo"))
        })
}

/// The Cargo configuration files cargo reads for a build in `checkout`, as
/// path and raw content: `.cargo/config.toml` and `.cargo/config` in the
/// checkout and every directory above it, then those in `cargo_home`. They
/// select compilers, linkers, wrappers, flags and profiles; an absent file
/// adds nothing, and one present but unreadable adds its error, since cargo
/// fails on it.
fn cargo_config(checkout: &Path, cargo_home: Option<&Path>) -> Vec<u8> {
    let dirs = checkout
        .ancestors()
        .map(|dir| dir.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf));
    let mut bytes = Vec::new();
    for dir in dirs {
        for name in ["config.toml", "config"] {
            let path = dir.join(name);
            match std::fs::read(&path) {
                Ok(content) => {
                    let _ = write!(bytes, "{}:{}:", path.display(), content.len());
                    bytes.extend_from_slice(&content);
                    bytes.push(b'\n');
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    let _ = writeln!(bytes, "{}:unreadable:{:?}", path.display(), error.kind());
                }
            }
        }
    }
    bytes
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

/// The toolchain's versions as the checkout's pinned toolchain reports them,
/// and the cross wrappers' a native step may run (`cargo-xwin`,
/// `cargo-zigbuild`); a tool that does not answer is recorded as absent.
fn toolchain(checkout: &Path) -> String {
    let tools: [(&str, &[&str]); 7] = [
        ("rustc", &["-vV"]),
        ("cargo", &["-V"]),
        ("cargo", &["clippy", "-V"]),
        ("cargo", &["nextest", "--version"]),
        ("cargo", &["hack", "--version"]),
        ("cargo-xwin", &["--version"]),
        ("cargo-zigbuild", &["--version"]),
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
/// [`result_neutral`] ones, sorted. With `fold_case` (Windows, whose names
/// are case-insensitive) names are upper-cased; elsewhere `rustflags` and
/// `RUSTFLAGS` are different variables and keep their case. Values are kept
/// as the platform encodes them, so two that differ only in bytes no Unicode
/// text spells are keyed apart. The search paths ([`SEARCH_PATHS`]) lose
/// their entries inside `cargo_outputs`, the directory Cargo ran xtask from.
pub(crate) fn results_env(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    cargo_outputs: Option<&Path>,
    fold_case: bool,
) -> Vec<(String, Vec<u8>)> {
    let mut env: Vec<(String, Vec<u8>)> = vars
        .into_iter()
        .map(|(name, value)| {
            let name = name.to_string_lossy();
            let name = if fold_case {
                name.to_ascii_uppercase()
            } else {
                name.into_owned()
            };
            (name, value)
        })
        .filter(|(name, value)| !result_neutral(name, value))
        .map(|(name, value)| {
            let value = if SEARCH_PATHS.contains(&name.as_str()) {
                outside(&value, cargo_outputs, fold_case)
            } else {
                value.as_encoded_bytes().to_vec()
            };
            (name, value)
        })
        .collect();
    env.sort();
    env
}

/// The program and library search paths. Cargo prepends the output
/// directory of the program it runs (`cargo xtask` is one) and that
/// directory's `deps` and build-script outputs to them, so those entries name
/// the build, not a tool; every other entry is keyed, a directory of the
/// checkout's own included, since it picks the linker, `git`, the cross
/// wrappers and the rest.
const SEARCH_PATHS: [&str; 4] = [
    "PATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
];

/// The entries of the search path `value` that do not lie in
/// `cargo_outputs`, newline-separated, each as the platform encodes it. With
/// `windows` (case-insensitive paths, `\` a separator) entries are compared
/// lower-cased with `/` for `\`; elsewhere `\` is an ordinary file-name byte
/// and case counts.
fn outside(value: &std::ffi::OsStr, cargo_outputs: Option<&Path>, windows: bool) -> Vec<u8> {
    let spelled = |path: &Path| {
        let mut bytes = path.as_os_str().as_encoded_bytes().to_vec();
        if windows {
            for byte in &mut bytes {
                if *byte == b'\\' {
                    *byte = b'/';
                }
            }
            bytes.make_ascii_lowercase();
        }
        while bytes.last() == Some(&b'/') {
            bytes.pop();
        }
        bytes
    };
    let outputs = cargo_outputs.map(spelled);
    let entries: Vec<Vec<u8>> = std::env::split_paths(value)
        .map(|entry| spelled(&entry))
        .filter(|entry| {
            outputs.as_ref().is_none_or(|outputs| {
                entry
                    .strip_prefix(outputs.as_slice())
                    .is_none_or(|rest| !rest.is_empty() && !rest.starts_with(b"/"))
            })
        })
        .collect();
    entries.join(&b'\n')
}

/// Whether the variable `name` set to `value` is proven not to change what a
/// run builds, tests or reports. Everything else is keyed, so a variable
/// nobody has looked at, a test control such as `PROPTEST_CASES` among them,
/// misses the cache instead of reusing a result it may not share.
///
/// Kept out of the key, each for its reason:
///
/// - job and thread counts, while the value is one the tool accepts
///   ([`valid_count`]): the suite must pass at any parallelism, and the host
///   lock sets the counts itself ([`crate::host_lock::Parallelism`]); a value
///   the tool rejects fails the run, so it is keyed;
/// - `CARGO_MAKEFLAGS`, the jobserver handle cargo hands each build script
///   (a new pipe per process), replaced by cargo's own for every build
///   script it runs. `MAKEFLAGS` and `MFLAGS` are keyed: besides a jobserver
///   they can carry flags a `make` invoked by a build script obeys;
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
/// toolchain's versions cover rustc, cargo, clippy, nextest, cargo-hack and
/// the cross wrappers, not the linker or `git` a step runs.
fn result_neutral(name: &str, value: &std::ffi::OsStr) -> bool {
    if let Some(valid) = valid_count(name, &value.to_string_lossy()) {
        return valid;
    }
    const NEUTRAL: [&str; 23] = [
        "CARGO_MAKEFLAGS",
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

/// For a job or thread count variable, whether `value` is a count its tool
/// accepts; `None` for any other variable.
///
/// - `CARGO_BUILD_JOBS` (cargo's `build.jobs`, The Cargo Book, "Configuration"):
///   a non-zero integer, a negative one counting back from the logical CPUs,
///   or `default`; cargo refuses `0` ("jobs may not be 0") and anything else.
/// - `NEXTEST_TEST_THREADS` (`cargo nextest run --help`, `--test-threads`:
///   "integer or \"num-cpus\""): a non-zero integer, negative counting back
///   from the CPUs, or `num-cpus`; nextest refuses `0` and anything else.
/// - `NUMBER_OF_PROCESSORS` (Windows' own CPU count): a positive integer.
fn valid_count(name: &str, value: &str) -> Option<bool> {
    let non_zero = || value.parse::<i64>().is_ok_and(|count| count != 0);
    match name {
        "CARGO_BUILD_JOBS" => Some(value == "default" || non_zero()),
        "NEXTEST_TEST_THREADS" => Some(value == "num-cpus" || non_zero()),
        "NUMBER_OF_PROCESSORS" => Some(value.parse::<u64>().is_ok_and(|count| count > 0)),
        _ => None,
    }
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
        let found = self.lookup(key)?;
        self.announce(&found);
        Some(found)
    }

    fn lookup(&self, key: &Key) -> Option<Verified> {
        self.store()
            .filter(|_| self.use_cache)
            .and_then(|store| store.lookup(key))
    }

    /// Reports that `found` already verified this run's key.
    fn announce(&mut self, found: &Verified) {
        let _ = writeln!(
            self.out,
            "check-changed: already verified green by {found} (same tree, base, toolchain, \
             environment and steps); nothing to run (--no-cache runs it anyway)"
        );
    }

    fn note(&mut self, text: impl fmt::Display) {
        let _ = writeln!(self.out, "check-changed: {text}");
    }

    /// Runs `checks` once per key, waiting for the lock with `lock`.
    ///
    /// `before` is the checkout's snapshot taken before `planned` was made
    /// (and the pre-lock checks run on it); their key only probes the record
    /// and registers the waiter, so it can spare the wait but never decides
    /// what runs, and its hit is taken only if a second snapshot still equals
    /// `before`. Under the lock the run reads its other inputs again
    /// ([`Checks::reread_inputs`]) and holds to one invariant: it snapshots
    /// the checkout first, plans it again ([`Checks::replan`]) unless that
    /// snapshot is still `before`, runs the plan, and records the marker only
    /// when a final snapshot equals the first and the inputs read again after
    /// the steps still give its key. An edit at any point after the first
    /// snapshot leaves the run's own result standing but unrecorded.
    pub(crate) fn verify<C: Checks, G>(
        mut self,
        checks: &mut C,
        before: anyhow::Result<Snapshot>,
        planned: C::Plan,
        lock: impl FnOnce() -> G,
    ) -> anyhow::Result<Outcome> {
        let before = match before {
            Ok(before) => before,
            Err(error) => {
                self.note(format_args!(
                    "cannot snapshot the checkout ({error:#}); running uncached"
                ));
                let _guard = lock();
                let plan = checks.replan()?;
                checks.run(&plan)?;
                return Ok(Outcome::Passed);
            }
        };
        let probe = checks.key(&before, &planned);
        // The plan was made after `before`: a hit stands only if the checkout
        // is still that tree, else the run goes on to plan it under the lock.
        if let Some(found) = self.lookup(&probe)
            && checks.snapshot().is_ok_and(|now| now == before)
        {
            self.announce(&found);
            return Ok(Outcome::AlreadyVerified(found));
        }
        let ticket = self
            .dir
            .as_ref()
            .and_then(|dir| Ticket::register(&dir.join("waiting"), &self.checkout, &probe).ok());
        let _guard = lock();
        // What the steps will run with, not what the probe was keyed with.
        checks.reread_inputs();
        let first = match checks.snapshot() {
            Ok(first) => first,
            Err(error) => {
                drop(ticket);
                self.note(format_args!(
                    "cannot snapshot the checkout ({error:#}); running uncached"
                ));
                let plan = checks.replan()?;
                checks.run(&plan)?;
                return Ok(Outcome::Passed);
            }
        };
        let plan = if first == before {
            planned
        } else {
            self.note("the checkout changed while this run waited; checking it again");
            checks.replan()?
        };
        let now = checks.key(&first, &plan);
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
        checks.run(&plan)?;
        // The other inputs may have changed while the steps ran too: a record
        // names only what held from the first snapshot to the last.
        checks.reread_inputs();
        match checks.snapshot() {
            Ok(last) if last == first && checks.key(&last, &plan) != now => self.note(
                "the Cargo configuration, toolchain or environment changed during the run; not recorded as verified",
            ),
            Ok(last) if last == first => {
                if let Some(store) = self.store()
                    && let Err(error) = store.record(&now, &Verified::this_run(&self.checkout))
                {
                    self.note(format_args!("could not record the verified tree: {error}"));
                }
            }
            Ok(_) => self.note("the checkout changed during the run; not recorded as verified"),
            Err(error) => self.note(format_args!(
                "cannot snapshot the checkout ({error:#}); not recorded as verified"
            )),
        }
        Ok(Outcome::Passed)
    }
}

/// The run a [`Verifier`] verifies once per key.
pub(crate) trait Checks {
    /// The steps planned for one tree.
    type Plan;

    /// The checkout as it is now.
    fn snapshot(&mut self) -> anyhow::Result<Snapshot>;

    /// The key of running `plan` on `snapshot`.
    fn key(&self, snapshot: &Snapshot, plan: &Self::Plan) -> Key;

    /// Reads the key's inputs besides the tree and the plan (Cargo
    /// configuration, toolchain) again: called once the lock is held, so the
    /// key a run records names what its steps ran with, not what was there
    /// while it waited, and again after the steps, so a change while they ran
    /// leaves the run unrecorded.
    fn reread_inputs(&mut self);

    /// Plans the checkout as it is now, running again the checks the pre-lock
    /// plan stood on (formatting): the checkout changed while the run waited,
    /// or could not be snapshotted.
    fn replan(&mut self) -> anyhow::Result<Self::Plan>;

    /// Runs `plan`'s steps.
    fn run(&mut self, plan: &Self::Plan) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;

    use super::*;
    use crate::table_test::run_table;

    /// The steps planned for tree `tree`.
    fn plan_of(tree: &str) -> String {
        format!("$ cargo clippy # scope of {tree}")
    }

    /// The key of tree `tree` run with its own plan.
    fn key(tree: &str) -> Key {
        asking(tree, &plan_of(tree))
    }

    /// A key for tree `tree` with the other inputs digested to `rest`.
    fn asking(tree: &str, rest: &str) -> Key {
        Key {
            tree: tree.to_owned(),
            rest: rest.to_owned(),
        }
    }

    fn snap(tree: &str, merge_base: &str) -> Snapshot {
        Snapshot {
            tree: tree.to_owned(),
            merge_base: merge_base.to_owned(),
        }
    }

    fn inputs() -> RunInputs {
        RunInputs {
            host: "builder windows x86_64".to_owned(),
            toolchain: "rustc 1.99.0".to_owned(),
            cargo_config: Vec::new(),
            env: Vec::new(),
        }
    }

    /// The rendered plan [`inputs`]' keys run.
    const STEPS: &str = "$ cargo clippy -p a";

    /// A checkout whose content follows `trees`: each snapshot or plan sees
    /// the next tree (the last repeats), and a plan is that tree's scope.
    struct Timeline<'a> {
        trees: &'a [&'a str],
        seen: usize,
        /// The inputs besides tree and plan, appended to the key's rest.
        inputs: &'a str,
        /// What [`Checks::reread_inputs`] finds under the lock, if they
        /// changed while the run waited.
        inputs_under_lock: Option<&'a str>,
        /// What the inputs become while the steps run.
        inputs_during_run: Option<&'a str>,
        replanned: bool,
        ran: Option<String>,
        result: Option<anyhow::Result<()>>,
    }

    impl<'a> Timeline<'a> {
        fn new(trees: &'a [&'a str], result: anyhow::Result<()>) -> Self {
            Self {
                trees,
                seen: 0,
                inputs: "",
                inputs_under_lock: None,
                inputs_during_run: None,
                replanned: false,
                ran: None,
                result: Some(result),
            }
        }

        fn next(&mut self) -> String {
            let tree = self.trees[self.seen.min(self.trees.len() - 1)];
            self.seen += 1;
            tree.to_owned()
        }
    }

    impl Checks for Timeline<'_> {
        type Plan = String;

        fn snapshot(&mut self) -> anyhow::Result<Snapshot> {
            Ok(snap(&self.next(), "base"))
        }

        fn key(&self, snapshot: &Snapshot, plan: &String) -> Key {
            asking(&snapshot.tree, &format!("{plan}{}", self.inputs))
        }

        fn reread_inputs(&mut self) {
            if let Some(inputs) = self.inputs_under_lock.take() {
                self.inputs = inputs;
            }
        }

        fn replan(&mut self) -> anyhow::Result<String> {
            self.replanned = true;
            Ok(plan_of(&self.next()))
        }

        fn run(&mut self, plan: &String) -> anyhow::Result<()> {
            self.ran = Some(plan.clone());
            if let Some(inputs) = self.inputs_during_run.take() {
                self.inputs_under_lock = Some(inputs);
            }
            self.result.take().unwrap_or(Ok(()))
        }
    }

    /// What one [`Case::verify_timeline`] did.
    struct Run {
        outcome: anyhow::Result<Outcome>,
        /// The plan that ran, if any.
        ran: Option<String>,
        replanned: bool,
        printed: String,
    }

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

        /// [`Self::verify_timeline`]: the outcome, whether the steps ran, and
        /// what was printed.
        fn verify(
            &self,
            use_cache: bool,
            trees: &[&str],
            during_lock: impl FnOnce(),
            result: anyhow::Result<()>,
        ) -> (anyhow::Result<Outcome>, bool, String) {
            let run = self.verify_timeline(use_cache, trees, during_lock, result);
            (run.outcome, run.ran.is_some(), run.printed)
        }

        /// Runs `verify` on a checkout whose content follows `trees`
        /// ([`Timeline`]), planned before the lock for the first tree;
        /// `during_lock` runs as the lock is taken (another run's actions
        /// while this one waited).
        fn verify_timeline(
            &self,
            use_cache: bool,
            trees: &[&str],
            during_lock: impl FnOnce(),
            result: anyhow::Result<()>,
        ) -> Run {
            self.run_timeline(use_cache, Timeline::new(trees, result), during_lock)
        }

        /// Runs `verify` on `checks`, planned before the lock for its first
        /// tree; `during_lock` runs as the lock is taken.
        fn run_timeline(
            &self,
            use_cache: bool,
            mut checks: Timeline<'_>,
            during_lock: impl FnOnce(),
        ) -> Run {
            let before = checks.snapshot();
            let planned = plan_of(&before.as_ref().expect("a snapshot").tree);
            let mut out = Vec::new();
            let outcome = Verifier::new(
                Some(self.dir.path()),
                self.checkout.path(),
                use_cache,
                &mut out,
            )
            .verify(&mut checks, before, planned, during_lock);
            Run {
                outcome,
                ran: checks.ran,
                replanned: checks.replanned,
                printed: String::from_utf8_lossy(&out).into_owned(),
            }
        }

        /// The markers recorded.
        fn recorded(&self) -> usize {
            std::fs::read_dir(self.dir.path().join("verified")).map_or(0, Iterator::count)
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
        let run = case.verify_timeline(true, &["k"], || (), Ok(()));
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        assert_eq!(run.ran, Some(plan_of("k")));
        assert!(!run.replanned, "an unchanged checkout keeps its plan");
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
        assert_eq!(case.recorded(), 0);
    }

    fn a_tree_changed_during_the_run_is_not_recorded() {
        let case = Case::new();
        let (outcome, _, printed) = case.verify(true, &["k", "k", "edited"], || (), Ok(()));
        assert_eq!(outcome.expect("passes"), Outcome::Passed);
        assert_eq!(case.recorded(), 0);
        assert!(printed.contains("changed during the run"), "{printed}");
    }

    /// An edit while the run waited moved the scope: the run plans (and
    /// checks formatting) again, runs the new plan, and the marker names the
    /// new tree with the steps it ran.
    fn a_checkout_changed_while_waiting_runs_the_new_plan() {
        let case = Case::new();
        let run = case.verify_timeline(true, &["old", "edited"], || (), Ok(()));
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        assert!(run.replanned);
        assert_eq!(run.ran, Some(plan_of("edited")));
        assert!(
            run.printed.contains("changed while this run waited"),
            "{}",
            run.printed
        );
        assert!(case.store().lookup(&key("edited")).is_some());
        assert_eq!(case.recorded(), 1, "only the new tree with its own plan");
    }

    /// The tree is snapshotted before it is planned under the lock: an edit
    /// landing while the run plans makes the plan and the snapshot disagree,
    /// and the run is not recorded rather than recorded with a plan of
    /// another tree.
    fn a_tree_changed_while_planning_is_not_recorded() {
        let case = Case::new();
        let run = case.verify_timeline(true, &["old", "edited", "edited again"], || (), Ok(()));
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        assert_eq!(run.ran, Some(plan_of("edited again")));
        assert_eq!(case.recorded(), 0, "nothing is recorded");
        assert!(
            run.printed.contains("changed during the run"),
            "{}",
            run.printed
        );
    }

    /// A marker found before the lock was keyed with a tree snapshotted
    /// before the plan was made: if the checkout changed meanwhile, the hit
    /// is not taken and the new tree is planned and run.
    fn a_hit_for_a_tree_edited_while_planning_is_not_taken() {
        let case = Case::new();
        case.store()
            .record(&key("k"), &Verified::this_run("elsewhere"))
            .expect("record");
        let run = case.verify_timeline(true, &["k", "edited"], || (), Ok(()));
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        assert!(run.replanned);
        assert_eq!(run.ran, Some(plan_of("edited")));
    }

    /// Inputs that changed while the run waited (a Cargo configuration, a
    /// toolchain) are what its steps run with, so its record is keyed with
    /// them, not with what the probe saw.
    fn inputs_changed_while_waiting_key_the_record() {
        let case = Case::new();
        let checks = Timeline {
            inputs_under_lock: Some(" config edited"),
            ..Timeline::new(&["k"], Ok(()))
        };
        let run = case.run_timeline(true, checks, || ());
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        let store = case.store();
        assert!(
            store
                .lookup(&asking("k", &format!("{} config edited", plan_of("k"))))
                .is_some(),
            "recorded with the inputs the steps ran with"
        );
        assert!(
            store.lookup(&key("k")).is_none(),
            "not with the inputs seen before the wait"
        );
    }

    /// Inputs that changed while the steps ran: neither the key the run
    /// started with nor the one it ended with was verified throughout.
    fn inputs_changed_during_the_run_are_not_recorded() {
        let case = Case::new();
        let checks = Timeline {
            inputs_during_run: Some(" config edited"),
            ..Timeline::new(&["k"], Ok(()))
        };
        let run = case.run_timeline(true, checks, || ());
        assert_eq!(run.outcome.expect("passes"), Outcome::Passed);
        assert_eq!(case.recorded(), 0, "{}", run.printed);
        assert!(
            run.printed.contains("changed during the run; not recorded"),
            "{}",
            run.printed
        );
    }

    /// Fields are encoded unambiguously: a name and value that share a text
    /// with another split of it hash apart.
    fn field_boundaries_are_part_of_the_digest() {
        assert_ne!(
            digest(&[("RUSTFLAGS", b"9:xxxxxxxxx")]),
            digest(&[("RUSTFLAGS:9", b"xxxxxxxxx")])
        );
    }

    fn a_newer_waiter_with_another_key_supersedes() {
        let case = Case::new();
        let newer = RefCell::new(None);
        let (outcome, ran, printed) = case.verify(
            true,
            &["old"],
            || {
                *newer.borrow_mut() = Some(
                    Ticket::register(
                        &case.waiting(),
                        &case.checkout(),
                        &asking("new", &plan_of("old")),
                    )
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
                Some(
                    Ticket::register(&case.waiting(), "elsewhere", &asking("new", &plan_of("k")))
                        .expect("register"),
                )
            }),
            ("dead", |case| {
                Ticket::register(
                    &case.waiting(),
                    &case.checkout(),
                    &asking("new", &plan_of("k")),
                )
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
                    "a_tree_changed_while_planning_is_not_recorded",
                    a_tree_changed_while_planning_is_not_recorded,
                ),
                (
                    "a_hit_for_a_tree_edited_while_planning_is_not_taken",
                    a_hit_for_a_tree_edited_while_planning_is_not_taken,
                ),
                (
                    "inputs_changed_while_waiting_key_the_record",
                    inputs_changed_while_waiting_key_the_record,
                ),
                (
                    "inputs_changed_during_the_run_are_not_recorded",
                    inputs_changed_during_the_run_are_not_recorded,
                ),
                (
                    "field_boundaries_are_part_of_the_digest",
                    field_boundaries_are_part_of_the_digest,
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
        let base = inputs().key(&snap("tree", "base"), STEPS);
        type Row = (&'static str, fn() -> Key);
        let rows: [Row; 7] = [
            ("tree", || inputs().key(&snap("other", "base"), STEPS)),
            ("host", || {
                RunInputs {
                    host: "other-builder windows x86_64".to_owned(),
                    ..inputs()
                }
                .key(&snap("tree", "base"), STEPS)
            }),
            ("cargo_config", || {
                RunInputs {
                    cargo_config: b"[build]\nrustc-wrapper = \"sccache\"\n".to_vec(),
                    ..inputs()
                }
                .key(&snap("tree", "base"), STEPS)
            }),
            ("merge_base", || inputs().key(&snap("tree", "other"), STEPS)),
            ("toolchain", || {
                RunInputs {
                    toolchain: "rustc 1.100.0".to_owned(),
                    ..inputs()
                }
                .key(&snap("tree", "base"), STEPS)
            }),
            ("env", || {
                RunInputs {
                    env: vec![("RUSTFLAGS".to_owned(), b"-Dwarnings".to_vec())],
                    ..inputs()
                }
                .key(&snap("tree", "base"), STEPS)
            }),
            ("steps", || {
                inputs().key(
                    &snap("tree", "base"),
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
            inputs().key(&snap("tree", "base"), STEPS),
            base,
            "the key is deterministic"
        );
    }

    /// The key of `vars` as this run's environment, case-folded or not.
    fn key_of(vars: &[(&str, &str)], fold_case: bool) -> Key {
        raw_key_of(
            vars.iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
            fold_case,
        )
    }

    /// The key of `vars` as the environment of an xtask Cargo ran from
    /// `/work/flui/target/debug`.
    fn raw_key_of(vars: impl IntoIterator<Item = (OsString, OsString)>, fold_case: bool) -> Key {
        RunInputs {
            env: results_env(vars, Some(Path::new("/work/flui/target/debug")), fold_case),
            ..inputs()
        }
        .key(&snap("tree", "base"), STEPS)
    }

    /// `/` and a code unit no Unicode text spells, distinct for each `n`:
    /// a lossy conversion turns them all into one replacement character.
    #[cfg(unix)]
    fn unspellable(n: u8) -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'/', 0x80 + n])
    }

    #[cfg(windows)]
    fn unspellable(n: u8) -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[u16::from(b'/'), 0xD800 + u16::from(n)])
    }

    /// Every inherited variable is keyed unless it is proven result-neutral:
    /// a test control, a variable nobody listed, or a count its tool would
    /// reject misses the cache.
    #[test]
    fn every_variable_but_the_neutral_ones_is_keyed() {
        let unset = key_of(&[], true);
        let keyed = [
            ("PROPTEST_CASES", "100000"),
            ("XTASK_TEST_UNLISTED_VARIABLE", "1"),
            ("RUSTFLAGS", "1"),
            ("CARGO_PROFILE_DEV_DEBUG", "1"),
            ("FLUI_HEADLESS", "1"),
            ("NEXTEST_PROFILE", "1"),
            ("CC_AARCH64_LINUX_ANDROID", "1"),
            ("RUST_LOG", "1"),
            ("NO_COLOR", "1"),
            ("CLICOLOR_FORCE", "1"),
            ("MAKEFLAGS", "-n"),
            ("PATH", "/usr/bin"),
            ("CARGO_BUILD_JOBS", "invalid"),
            ("CARGO_BUILD_JOBS", "0"),
            ("NEXTEST_TEST_THREADS", "invalid"),
            ("NEXTEST_TEST_THREADS", "0"),
            ("NUMBER_OF_PROCESSORS", "-2"),
        ];
        let neutral = [
            ("CARGO_BUILD_JOBS", "28"),
            ("CARGO_BUILD_JOBS", "-2"),
            ("CARGO_BUILD_JOBS", "default"),
            ("NEXTEST_TEST_THREADS", "16"),
            ("NEXTEST_TEST_THREADS", "-1"),
            ("NEXTEST_TEST_THREADS", "num-cpus"),
            ("NUMBER_OF_PROCESSORS", "32"),
            ("CARGO_MAKEFLAGS", "-j --jobserver-auth=3,4"),
            ("FLUI_XTASK_NO_LOCK", "1"),
            ("CARGO_TERM_COLOR", "always"),
            ("CARGO_MANIFEST_DIR", "/work/flui/tools/xtask"),
            ("CARGO_PKG_VERSION", "0.2.0"),
            ("CARGO_TARGET_DIR", "target"),
            ("RUSTUP_TOOLCHAIN", "1.99.0"),
            ("SHLVL", "3"),
            ("WT_SESSION", "0f1e"),
            ("CLAUDE_CODE_SESSION_ID", "0f1e"),
        ];
        let unkeyed: Vec<_> = keyed
            .into_iter()
            .filter(|var| key_of(&[*var], true) == unset)
            .collect();
        assert!(unkeyed.is_empty(), "not keyed: {unkeyed:?}");
        let misses: Vec<_> = neutral
            .into_iter()
            .filter(|var| key_of(&[*var], true) != unset)
            .collect();
        assert!(misses.is_empty(), "neutral but keyed: {misses:?}");
        // Windows' names are case-insensitive; Unix's are not, so there
        // `rustflags` (which cargo ignores) is not `RUSTFLAGS`.
        let flags = |name| [(name, "-Copt-level=0")];
        assert_eq!(
            key_of(&flags("rustflags"), true),
            key_of(&flags("RUSTFLAGS"), true)
        );
        assert_ne!(
            key_of(&flags("rustflags"), false),
            key_of(&flags("RUSTFLAGS"), false)
        );
        assert_ne!(
            key_of(&[("cargo_build_jobs", "invalid")], false),
            key_of(&[("CARGO_BUILD_JOBS", "invalid")], false),
        );
        // Values are keyed as the platform encodes them, not as text.
        let report_dir = |n| {
            raw_key_of(
                [(OsString::from("FLUI_COVERAGE_REPORT_DIR"), unspellable(n))],
                true,
            )
        };
        assert_ne!(report_dir(0), report_dir(1));
        let search_path = |n| raw_key_of([(OsString::from("PATH"), unspellable(n))], true);
        assert_ne!(search_path(0), search_path(1));
        // A search path keys every entry but the ones Cargo added for the
        // xtask it ran, the checkout's own directories included.
        let path = |entries: &[&str]| {
            std::env::join_paths(entries)
                .expect("joinable")
                .to_string_lossy()
                .into_owned()
        };
        let system = key_of(&[("PATH", &path(&["/usr/bin"]))], true);
        assert_eq!(
            key_of(
                &[("PATH", &path(&["/work/flui/target/debug/deps", "/usr/bin"]))],
                true
            ),
            system,
        );
        assert_eq!(
            key_of(
                &[(
                    "PATH",
                    &path(&[
                        "/work/flui/target/debug",
                        "/work/flui/target/debug/build/x/out",
                        "/usr/bin"
                    ])
                )],
                true
            ),
            system,
        );
        for other in [
            path(&["/opt/llvm/bin", "/usr/bin"]),
            path(&["/work/flui-other/bin", "/usr/bin"]),
            path(&["/work/flui/bin", "/usr/bin"]),
            path(&["/work/flui/target/debug-other", "/usr/bin"]),
        ] {
            assert_ne!(key_of(&[("PATH", &other)], true), system, "{other:?}");
        }
        // `\` separates path components only on Windows; elsewhere it is a
        // file-name byte, so `tools\bin` is another directory than `tools/bin`.
        let tools = |entry, windows| key_of(&[("PATH", entry)], windows);
        assert_eq!(
            tools(r"/tmp/tools\bin", true),
            tools("/tmp/tools/bin", true)
        );
        assert_ne!(
            tools(r"/tmp/tools\bin", false),
            tools("/tmp/tools/bin", false)
        );
    }

    /// The configuration cargo reads: the checkout's, every parent's and
    /// the Cargo home's, each by content; a relative `CARGO_HOME` names a
    /// directory under the checkout, where cargo runs, not under the
    /// directory xtask was started in.
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
        let relative = cargo_home(&checkout, |name| {
            (name == "CARGO_HOME").then(|| OsString::from("../../cargo-home"))
        });
        let config = String::from_utf8(cargo_config(&checkout, relative.as_deref()))
            .expect("UTF-8 configuration");
        for text in ["own = 1", "linker = \"a\"", "xtask-test-marker = 16"] {
            assert!(config.contains(text), "`{text}` is read: {config}");
        }
        assert_eq!(
            cargo_home(&checkout, |name| (name == "CARGO_HOME")
                .then(|| home.clone().into())),
            Some(home.clone()),
            "an absolute CARGO_HOME is taken as it is"
        );
        write(
            &root.path().join("parent/.cargo/config"),
            "[target.x]\nlinker = \"b\"\n",
        );
        assert_ne!(
            cargo_config(&checkout, Some(&home)),
            config.as_bytes(),
            "a parent directory's edit counts"
        );
        assert!(
            !String::from_utf8_lossy(&cargo_config(&checkout, None))
                .contains("xtask-test-marker = 16")
        );
        // A file cargo would fail to load is keyed, never read as absent:
        // its raw bytes when it is no text, its error when it cannot be read.
        let own = checkout.join(".cargo/config.toml");
        std::fs::remove_file(&own).expect("remove");
        let mut keyed = vec![cargo_config(&checkout, None)];
        for bytes in [
            &b"[build]\nx = \"\xff\"\n"[..],
            &b"[build]\nx = \"\xfe\"\n"[..],
        ] {
            std::fs::write(&own, bytes).expect("write");
            keyed.push(cargo_config(&checkout, None));
        }
        std::fs::remove_file(&own).expect("remove");
        std::fs::create_dir(&own).expect("a directory where the file was");
        keyed.push(cargo_config(&checkout, None));
        for (i, a) in keyed.iter().enumerate() {
            for b in &keyed[i + 1..] {
                assert_ne!(a, b, "absent, two non-UTF-8 files and an unreadable one");
            }
        }
        assert!(
            host().ends_with(&format!(
                " {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            "the host names its OS and architecture"
        );
    }

    /// The cross wrappers a native step runs are versioned with the
    /// toolchain, installed or not, so reinstalling one misses the cache.
    #[test]
    fn the_toolchain_covers_the_cross_wrappers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let versions = toolchain(dir.path());
        for wrapper in ["cargo-xwin --version: ", "cargo-zigbuild --version: "] {
            assert!(versions.contains(wrapper), "{wrapper}in {versions}");
        }
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
