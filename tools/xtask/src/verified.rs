//! `check-changed`'s record of the trees it verified green, and of the runs
//! waiting in the host lock's queue to verify one.
//!
//! A run is identified by a [`Key`]: a hash of everything that decides its
//! result ([`RunInputs`]): the checkout's whole tree as `git add -A` would
//! stage it (tracked, modified and untracked files, `.gitignore` honoured),
//! the merge base with the base revision, the toolchain's versions, the
//! environment variables that change what cargo builds or tests
//! ([`changes_results`]), and the rendered step list. A step this host skips
//! is a line of that list, so a host that skipped wasm32 never certifies one
//! that runs it.
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
//! one waited), then looks for a newer waiter from the same checkout: one
//! with another key will check a newer tree, so this run stops without
//! claiming anything ([`Outcome::Superseded`]); one with the same key lets
//! this run go ahead and will find its marker. Every record is best effort:
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
    toolchain: String,
    env: Vec<(String, String)>,
    steps: String,
}

impl RunInputs {
    /// This process's inputs for verifying `checkout` against `base` with
    /// `steps` (the rendered plan): the toolchain and environment are read
    /// now, the tree and merge base by each [`Self::key`].
    pub(crate) fn of_this_run(checkout: &Path, base: &str, steps: &str) -> Self {
        Self {
            checkout: checkout.to_path_buf(),
            base: base.to_owned(),
            toolchain: toolchain(checkout),
            env: results_env(std::env::vars_os()),
            steps: steps.to_owned(),
        }
    }

    /// The key of the checkout as it is now.
    pub(crate) fn key(&self) -> anyhow::Result<Key> {
        let tree = worktree_tree(&self.checkout)?;
        let merge_base = git(&self.checkout, &["merge-base", &self.base, "HEAD"], None)?;
        Ok(self.key_with(&tree, &merge_base))
    }

    /// The key for `tree` on `merge_base`, with this run's other inputs.
    /// Each field is length-prefixed, so no two input sets hash one text.
    fn key_with(&self, tree: &str, merge_base: &str) -> Key {
        let mut hasher = Sha256::new();
        let mut field = |name: &str, value: &str| {
            hasher.update(format!("{name}:{}:", value.len()));
            hasher.update(value);
        };
        field("tree", tree.trim());
        field("merge-base", merge_base.trim());
        field("toolchain", &self.toolchain);
        for (name, value) in &self.env {
            field(name, value);
        }
        field("steps", &self.steps);
        Key(hasher
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }))
    }
}

/// A run's identity: hex SHA-256 of its [`RunInputs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key(String);

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
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

/// The variables of `vars` that [`changes_results`], sorted, with names
/// upper-cased (Windows' names are case-insensitive).
pub(crate) fn results_env(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vars
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().to_ascii_uppercase(),
                value.to_string_lossy().into_owned(),
            )
        })
        .filter(|(name, _)| changes_results(name))
        .collect();
    env.sort();
    env
}

/// Whether the variable `name` (upper-case) can change what cargo builds or
/// tests: `RUSTFLAGS`, `RUSTDOCFLAGS`, `CARGO_*`, `FLUI_*` and `NEXTEST_*`,
/// except job and thread counts, locations, terminal styling, the variables
/// cargo sets for the program it runs, and the run lock's own controls.
pub(crate) fn changes_results(name: &str) -> bool {
    const CARGO_EXCEPT: [&str; 11] = [
        "CARGO",
        "CARGO_BUILD_JOBS",
        "CARGO_MAKEFLAGS",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_RUSTC_CURRENT_DIR",
    ];
    const FLUI_EXCEPT: [&str; 3] = [
        "FLUI_XTASK_LOCK_FILE",
        "FLUI_XTASK_NO_LOCK",
        "FLUI_XTASK_LOCK_HELD",
    ];
    match name {
        "RUSTFLAGS" | "RUSTDOCFLAGS" => true,
        _ if name.starts_with("CARGO_") || name == "CARGO" => {
            !CARGO_EXCEPT.contains(&name)
                && !name.starts_with("CARGO_PKG_")
                && !name.starts_with("CARGO_TERM_")
        }
        _ if name.starts_with("FLUI_") => !FLUI_EXCEPT.contains(&name),
        _ if name.starts_with("NEXTEST_") => name != "NEXTEST_TEST_THREADS",
        _ => false,
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
        self.dir.join(&key.0)
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
            .join(format!("{}.{}.tmp", key.0, std::process::id()));
        let text = format!("{}\n{}\n{}\n", run.pid, run.at, run.checkout);
        std::fs::write(&staged, text)
            .and_then(|()| std::fs::rename(&staged, self.marker(key)))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&staged);
            })?;
        self.prune();
        Ok(())
    }

    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut markers: Vec<(SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_none())
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        if markers.len() <= CAP {
            return;
        }
        markers.sort();
        for (_, path) in &markers[..markers.len() - CAP] {
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
        std::fs::write(&staged, format!("{checkout}\n{key}\n"))?;
        std::fs::rename(&staged, ticket.path("waiter"))?;
        Ok(ticket)
    }

    fn path(&self, extension: &str) -> PathBuf {
        self.dir.join(format!("{}.{extension}", stem(self.order)))
    }

    /// The newest live waiter from `checkout` registered after this one.
    fn newest_after(&self, checkout: &str) -> Option<Waiter> {
        live_waiters(&self.dir)
            .into_iter()
            .filter(|waiter| waiter.order > self.order && waiter.checkout == checkout)
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
                key: Key(lines.next()?.to_owned()),
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

impl<'a> Verifier<'a> {
    /// Records under `dir` (beside the host lock) for `checkout`.
    pub(crate) fn new(
        dir: Option<&Path>,
        checkout: &Path,
        use_cache: bool,
        out: &'a mut dyn Write,
    ) -> Self {
        let checkout = checkout
            .canonicalize()
            .unwrap_or_else(|_| checkout.to_path_buf());
        Self {
            dir: dir.map(Path::to_path_buf),
            checkout: checkout.display().to_string(),
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

    /// Computes the key with `key`, takes the lock with `lock` and runs the
    /// steps with `run`, skipping what a record makes unnecessary. `key` is
    /// asked before the lock, right after it, and after `run` passed: the
    /// marker is written only when the last two agree.
    pub(crate) fn verify<G>(
        mut self,
        mut key: impl FnMut() -> anyhow::Result<Key>,
        lock: impl FnOnce() -> G,
        run: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<Outcome> {
        let before = match key() {
            Ok(before) => before,
            Err(error) => {
                self.note(format_args!(
                    "cannot compute the run's key ({error:#}); running uncached"
                ));
                let _guard = lock();
                run()?;
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
        let now = match key() {
            Ok(now) => now,
            Err(error) => {
                drop(ticket);
                self.note(format_args!(
                    "cannot compute the run's key ({error:#}); running uncached"
                ));
                run()?;
                return Ok(Outcome::Passed);
            }
        };
        if let Some(found) = self.cached(&now) {
            return Ok(Outcome::AlreadyVerified(found));
        }
        let newer = ticket
            .as_ref()
            .and_then(|ticket| ticket.newest_after(&self.checkout));
        drop(ticket);
        if newer.is_some_and(|newer| newer.key != now) {
            self.note(
                "superseded by a newer check-changed of this checkout; this tree was not verified",
            );
            return Ok(Outcome::Superseded);
        }
        run()?;
        match key() {
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

    fn key(text: &str) -> Key {
        Key(text.to_owned())
    }

    fn inputs() -> RunInputs {
        RunInputs {
            checkout: PathBuf::from("checkout"),
            base: "origin/main".to_owned(),
            toolchain: "rustc 1.99.0".to_owned(),
            env: Vec::new(),
            steps: "$ cargo clippy -p a".to_owned(),
        }
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
            self.checkout
                .path()
                .canonicalize()
                .expect("canonical checkout")
                .display()
                .to_string()
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
            let asked = Cell::new(0);
            let ran = Cell::new(false);
            let mut out = Vec::new();
            let outcome = Verifier::new(
                Some(self.dir.path()),
                self.checkout.path(),
                use_cache,
                &mut out,
            )
            .verify(
                || {
                    let index = asked.get().min(keys.len() - 1);
                    asked.set(asked.get() + 1);
                    Ok(key(keys[index]))
                },
                during_lock,
                || {
                    ran.set(true);
                    result
                },
            );
            (
                outcome,
                ran.get(),
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
    /// another checkout, and one that died while waiting.
    fn other_newer_waiters_let_the_run_proceed() {
        type Newer = fn(&Case) -> Option<Ticket>;
        let rows: [(&str, Newer); 3] = [
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
        let base = inputs().key_with("tree", "base");
        type Row = (&'static str, fn() -> Key);
        let rows: [Row; 5] = [
            ("tree", || inputs().key_with("other", "base")),
            ("merge_base", || inputs().key_with("tree", "other")),
            ("toolchain", || {
                RunInputs {
                    toolchain: "rustc 1.100.0".to_owned(),
                    ..inputs()
                }
                .key_with("tree", "base")
            }),
            ("env", || {
                RunInputs {
                    env: vec![("RUSTFLAGS".to_owned(), "-Dwarnings".to_owned())],
                    ..inputs()
                }
                .key_with("tree", "base")
            }),
            ("steps", || {
                RunInputs {
                    steps: "$ cargo clippy -p a\ncheck-changed: skipped wasm32".to_owned(),
                    ..inputs()
                }
                .key_with("tree", "base")
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
            inputs().key_with("tree", "base"),
            base,
            "the key is deterministic"
        );
    }

    #[test]
    fn only_result_changing_variables_are_keyed() {
        let vars = [
            ("RUSTFLAGS", true),
            ("RUSTDOCFLAGS", true),
            ("CARGO_PROFILE_DEV_DEBUG", true),
            ("CARGO_ENCODED_RUSTFLAGS", true),
            ("FLUI_HEADLESS", true),
            ("NEXTEST_PROFILE", true),
            ("CARGO_BUILD_JOBS", false),
            ("NEXTEST_TEST_THREADS", false),
            ("CARGO_TARGET_DIR", false),
            ("CARGO_MANIFEST_DIR", false),
            ("CARGO_PKG_VERSION", false),
            ("CARGO_TERM_COLOR", false),
            ("FLUI_XTASK_NO_LOCK", false),
            ("PATH", false),
        ];
        let wrong: Vec<&str> = vars
            .iter()
            .filter(|(name, keyed)| changes_results(name) != *keyed)
            .map(|(name, _)| *name)
            .collect();
        assert!(wrong.is_empty(), "misclassified: {wrong:?}");
        let env = results_env([
            (OsString::from("rustflags"), OsString::from("-Dwarnings")),
            (OsString::from("PATH"), OsString::from("/bin")),
        ]);
        assert_eq!(env, [("RUSTFLAGS".to_owned(), "-Dwarnings".to_owned())]);
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
