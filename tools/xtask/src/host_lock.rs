//! One heavy run at a time for the user on this machine: the commands that
//! build or test the workspace (`main`'s `Command::is_heavy`) queue behind an
//! exclusive file lock.
//!
//! Several checkouts and worktrees of this repository often share one
//! machine, and two workspace builds at once oversubscribe it until every run
//! looks hung. The lock file sits in a fixed per-user directory, so every
//! checkout the user has finds the same one: `%LOCALAPPDATA%\flui\` on
//! Windows, `$HOME/.cache/flui/` elsewhere (`XDG_RUNTIME_DIR` is not
//! consulted, so sessions with and without it pick the same file). Without
//! that variable, or with a relative value, it falls back to `flui-<user>`
//! in the temporary directory (the user id on Unix, the user name on
//! Windows), so accounts sharing that directory keep separate locks; if that
//! temporary directory is itself relative, the run warns and proceeds
//! unlocked. An absolute `FLUI_XTASK_LOCK_FILE` names another file (a
//! relative one is refused with a warning) and `FLUI_XTASK_NO_LOCK=1` skips
//! the lock.
//!
//! On a home directory shared over the network, runs on different machines
//! share one lock and queue behind one another: a slowdown, not a failure.
//! On Windows the user-name fallback applies only without `LOCALAPPDATA`
//! (rare); accounts sharing a user name may then share a lock.
//!
//! The lock is the operating system's advisory lock on an open file
//! ([`File::lock`]): it is released when the handle closes, and so when the
//! holding process exits or crashes. A dead run never leaves a stale lock
//! behind, and there is nothing to clean up.
//!
//! Composite commands (`ci` runs `gate` and `test`) call each other in this
//! process, under the one lock `main` took. A child process this one spawns
//! (through [`crate::tasks`]' `Cmd`) carries [`LOCK_HELD`] naming the lock
//! file, and a heavy command started with it does not lock that same file
//! again: its parent holds it and is waiting for the child. A marker naming
//! another file is ignored, and the child locks its own normally.
//!
//! A waiting run prints once which run it waits for, read from a record the
//! holder writes beside the lock. That line is best effort: the record can be
//! missing (the holder has not written it yet) or, if a removal failed, name
//! an earlier run. The lock itself never depends on it.
//!
//! The lock never fails a run: if the file cannot be opened or locked, a
//! warning says so and the command runs unlocked.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};

/// Names another lock file instead of the per-user one.
const LOCK_FILE: &str = "FLUI_XTASK_LOCK_FILE";
/// `1` skips the lock.
const NO_LOCK: &str = "FLUI_XTASK_NO_LOCK";
/// Set by a process holding the lock for the processes it spawns: the path
/// of the lock file it holds.
const LOCK_HELD: &str = "FLUI_XTASK_LOCK_HELD";

/// The lock file this process holds, while a guard is live. A path rather
/// than a flag: a spawned child is marked with it.
static HELD_HERE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Serializes the tests that take a guard, since [`HELD_HERE`] is
/// process-wide.
#[cfg(test)]
pub(crate) static TEST_GUARDS: Mutex<()> = Mutex::new(());

fn held_here() -> std::sync::MutexGuard<'static, Option<PathBuf>> {
    HELD_HERE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks `child` with the lock file this process holds, if it holds one, so
/// a heavy command the child runs does not wait for its own parent.
pub(crate) fn mark_child(child: &mut Command) {
    if let Some(path) = held_here().as_ref() {
        child.env(LOCK_HELD, path);
    }
}

/// Where the lock is and whether to take it, read from the environment.
#[derive(Debug, Clone)]
pub(crate) struct LockSettings {
    path: PathBuf,
    /// A relative `FLUI_XTASK_LOCK_FILE`, refused in favour of the default.
    rejected: Option<PathBuf>,
    /// The path falls back to a relative temporary directory, so no two
    /// runs could agree on it.
    unlocated: bool,
    opted_out: bool,
    held_by: Option<PathBuf>,
}

impl LockSettings {
    /// The settings this process's environment gives.
    pub(crate) fn from_env() -> Self {
        Self::from_vars(
            |key| std::env::var_os(key),
            current_user,
            std::env::temp_dir,
        )
    }

    /// `var` reads the environment; `user` names the current user and
    /// `temp` the temporary directory, both asked only when no per-user
    /// directory variable is set.
    fn from_vars(
        var: impl Fn(&str) -> Option<OsString>,
        user: impl FnOnce() -> Option<String>,
        temp: impl FnOnce() -> PathBuf,
    ) -> Self {
        Self::for_platform(cfg!(windows), var, user, temp)
    }

    /// [`Self::from_vars`] choosing Windows' or the other platforms' rules,
    /// so a test on one host covers both.
    fn for_platform(
        windows: bool,
        var: impl Fn(&str) -> Option<OsString>,
        user: impl FnOnce() -> Option<String>,
        temp: impl FnOnce() -> PathBuf,
    ) -> Self {
        let set = |key| var(key).filter(|value| !value.is_empty());
        // A relative value would name a different file from each directory a
        // run starts in, so it counts as unset. XDG_RUNTIME_DIR is not
        // consulted: a session with it and one without must agree.
        let user_dir = if windows {
            set("LOCALAPPDATA").map(PathBuf::from)
        } else {
            set("HOME").map(|home| PathBuf::from(home).join(".cache"))
        }
        .filter(|dir| dir.is_absolute());
        // Without one, the shared temporary directory, in a directory named
        // after the user so accounts on one machine keep separate locks.
        // A relative temporary directory would name a different file from
        // each directory a run starts in: no lock at all then.
        let mut unlocated = false;
        let dir = user_dir.map_or_else(
            || {
                let temp = temp();
                unlocated = !temp.is_absolute();
                temp.join(fallback_dir_name(user()))
            },
            |dir| dir.join("flui"),
        );
        // A relative override would name a different file from each
        // directory a run starts in, so it is refused, not resolved.
        let (path, rejected, unlocated) = match set(LOCK_FILE).map(PathBuf::from) {
            Some(path) if path.is_absolute() => (path, None, false),
            rejected => (dir.join("xtask-heavy.lock"), rejected, unlocated),
        };
        Self {
            unlocated,
            path,
            rejected,
            opted_out: set(NO_LOCK).is_some_and(|value| value == "1"),
            held_by: set(LOCK_HELD).map(PathBuf::from),
        }
    }

    /// Lock at `path`, with no opt-out and no parent holding it.
    #[cfg(test)]
    pub(crate) fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            rejected: None,
            unlocated: false,
            opted_out: false,
            held_by: None,
        }
    }

    /// Whether a parent process holds this very lock file.
    fn parent_holds(&self) -> bool {
        self.held_by
            .as_deref()
            .is_some_and(|held| same_file(held, &self.path))
    }
}

/// `flui-<user>-<hash>`: the user with any character outside
/// `[A-Za-z0-9._-]` replaced, so it stays one path component, then a hash
/// of the user as given, so two names that read the same once replaced
/// (`a b`, `a_b`) keep separate directories. Plain `flui` when the user
/// cannot be named.
fn fallback_dir_name(user: Option<String>) -> String {
    let Some(user) = user.filter(|user| !user.is_empty()) else {
        return "flui".to_owned();
    };
    let readable: String = user
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("flui-{readable}-{:08x}", fnv1a32(user.as_bytes()))
}

/// 32-bit FNV-1a: fixed by its definition, so every xtask build, whatever
/// its Rust version, names the same directory (std's `DefaultHasher`
/// promises no such stability).
fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The current user's numeric id: the owner of a file this process creates
/// is its effective user (no libc call needed). `USER`/`LOGNAME` if that
/// fails.
#[cfg(unix)]
fn current_user() -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    tempfile::tempfile()
        .and_then(|file| file.metadata())
        .map(|metadata| metadata.uid().to_string())
        .ok()
        .or_else(|| {
            ["USER", "LOGNAME"]
                .into_iter()
                .find_map(|key| std::env::var(key).ok())
        })
}

/// The current user's name.
#[cfg(not(unix))]
fn current_user() -> Option<String> {
    std::env::var("USERNAME").ok()
}

/// Whether `a` and `b` name the same file, however each is spelled.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The run that holds the lock, as a waiting run names it.
#[derive(Debug, Clone)]
pub(crate) struct Holder {
    /// The command line, `cargo xtask <args>`.
    pub(crate) command: String,
    /// The checkout it runs in.
    pub(crate) checkout: PathBuf,
}

impl Holder {
    /// This process: its own arguments and checkout.
    pub(crate) fn this_run() -> Self {
        Self::from_args(std::env::args_os().skip(1), crate::util::repo_root())
    }

    /// The holder running `cargo xtask <args>` in `checkout`. An argument
    /// that is not Unicode (clap accepts one as a path) is shown lossily.
    fn from_args(args: impl IntoIterator<Item = OsString>, checkout: PathBuf) -> Self {
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        Self {
            command: format!("cargo xtask {}", args.join(" ")),
            checkout,
        }
    }
}

/// Holds the lock until dropped (or until the process exits).
#[derive(Debug)]
pub(crate) struct HeavyRunLock {
    held: Option<(File, PathBuf)>,
}

impl HeavyRunLock {
    /// Takes the lock, waiting while another run holds it after printing to
    /// `out` which run that is (best effort, see the module docs).
    ///
    /// Returns without the lock when `settings` opt out, when a parent
    /// process holds this lock file, or when the lock file cannot be used
    /// (after a warning).
    pub(crate) fn acquire(settings: &LockSettings, me: &Holder, out: &mut dyn Write) -> Self {
        let unheld = Self { held: None };
        if settings.opted_out || settings.parent_holds() {
            return unheld;
        }
        let path = &settings.path;
        if settings.unlocated {
            let _ = writeln!(
                out,
                "xtask: warning: the run lock's path {} is not absolute (no per-user directory \
                 and a relative temporary directory); running without it",
                path.display()
            );
            return unheld;
        }
        if let Some(rejected) = &settings.rejected {
            let _ = writeln!(
                out,
                "xtask: warning: {LOCK_FILE}={} is not an absolute path; using {} instead",
                rejected.display(),
                path.display()
            );
        }
        let opened = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path)
            });
        let file = match opened {
            Ok(file) => file,
            Err(error) => {
                warn(out, path, &error);
                return unheld;
            }
        };
        let locked = match file.try_lock() {
            Ok(()) => Ok(()),
            Err(TryLockError::WouldBlock) => {
                let _ = writeln!(out, "xtask: waiting for {}", holder_line(path));
                file.lock()
            }
            Err(TryLockError::Error(error)) => Err(error),
        };
        if let Err(error) = locked {
            warn(out, path, &error);
            return unheld;
        }
        write_record(path, me);
        *held_here() = Some(path.clone());
        Self {
            held: Some((file, path.clone())),
        }
    }

    /// Whether this guard holds the lock.
    #[cfg(test)]
    fn holds(&self) -> bool {
        self.held.is_some()
    }
}

impl Drop for HeavyRunLock {
    fn drop(&mut self) {
        if let Some((file, path)) = self.held.take() {
            // Before unlocking, so the next holder's record is never the one
            // removed. If the removal fails, a waiter may name this finished
            // run until the next holder replaces the record.
            let _ = std::fs::remove_file(record(&path));
            *held_here() = None;
            let _ = file.unlock();
        }
    }
}

fn warn(out: &mut dyn Write, path: &Path, error: &std::io::Error) {
    let _ = writeln!(
        out,
        "xtask: warning: cannot use the run lock {}: {error}; running without it",
        path.display()
    );
}

/// The holder's record, beside the lock file: on Windows the lock keeps
/// other processes from reading the locked file itself.
fn record(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".holder");
    PathBuf::from(name)
}

/// Writes the record to a file of this process's own, then renames it into
/// place, so a waiter reads a whole record or none. Best effort.
fn write_record(path: &Path, me: &Holder) {
    let pid = std::process::id();
    let mut staged = record(path).into_os_string();
    staged.push(format!(".{pid}"));
    let staged = PathBuf::from(staged);
    let text = format!("{pid}\n{}\n{}\n", me.command, me.checkout.display());
    if std::fs::write(&staged, text)
        .and_then(|()| std::fs::rename(&staged, record(path)))
        .is_err()
    {
        let _ = std::fs::remove_file(&staged);
    }
}

/// "`cargo xtask test` (pid 42, D:\flui) to finish", or a generic line when
/// the holder left no readable record.
fn holder_line(path: &Path) -> String {
    let text = std::fs::read_to_string(record(path)).unwrap_or_default();
    let mut lines = text.lines();
    match (lines.next(), lines.next(), lines.next()) {
        (Some(pid), Some(command), Some(checkout)) => {
            format!("`{command}` (pid {pid}, {checkout}) to finish ...")
        }
        _ => "another heavy xtask run to finish ...".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::table_test::run_table;

    fn holder(command: &str) -> Holder {
        Holder {
            command: command.to_owned(),
            checkout: PathBuf::from("checkout-under-test"),
        }
    }

    fn lock_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("heavy.lock")
    }

    /// Takes the lock on another thread; the receiver yields what it printed
    /// once `acquire` returns, and the guard is dropped right after.
    fn acquire_elsewhere(settings: LockSettings) -> mpsc::Receiver<String> {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let guard = HeavyRunLock::acquire(&settings, &holder("cargo xtask test"), &mut out);
            drop(guard);
            let _ = sender.send(String::from_utf8_lossy(&out).into_owned());
        });
        receiver
    }

    /// Whether a fresh handle finds the file locked by someone else.
    fn locked(path: &Path) -> bool {
        let file = File::open(path).expect("the lock file exists");
        matches!(file.try_lock(), Err(TryLockError::WouldBlock))
    }

    fn second_acquirer_waits_for_the_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = lock_path(&dir);
        let mut out = Vec::new();
        let first = HeavyRunLock::acquire(
            &LockSettings::at(&path),
            &holder("cargo xtask check-changed"),
            &mut out,
        );
        assert!(
            first.holds() && locked(&path),
            "the first run holds the lock"
        );
        let second = acquire_elsewhere(LockSettings::at(&path));
        assert!(
            second.recv_timeout(Duration::from_millis(500)).is_err(),
            "the second run must wait while the first holds the lock"
        );
        drop(first);
        let printed = second
            .recv_timeout(Duration::from_secs(10))
            .expect("the second run proceeds once the first releases");
        let pid = std::process::id().to_string();
        assert!(
            printed.contains("waiting for `cargo xtask check-changed`")
                && printed.contains(&format!("pid {pid}"))
                && printed.contains("checkout-under-test"),
            "the waiter names the holder: {printed}"
        );
    }

    /// Set for a child process of [`reentrant_child_does_not_wait`]: take
    /// the lock as `main` would, then exit.
    const CHILD: &str = "FLUI_XTASK_LOCK_TEST_CHILD";

    /// Spawns this test binary as a heavy run marked by [`mark_child`] whose
    /// own lock file is `child_lock`; whether it exits successfully within
    /// `limit` (it is killed otherwise).
    fn child_finishes(child_lock: &Path, limit: Duration) -> bool {
        let mut child = Command::new(std::env::current_exe().expect("test executable"));
        child
            .args(["host_lock::tests::host_lock_contract", "--exact"])
            .env_remove(NO_LOCK)
            .env_remove(LOCK_HELD)
            .env(LOCK_FILE, child_lock)
            .env(CHILD, "1");
        mark_child(&mut child);
        let mut child = child.spawn().expect("spawn the child run");
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().expect("poll the child") {
                return status.success();
            }
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A real child process, marked by [`mark_child`] and reading its
    /// settings from its environment as `main` does, runs under the parent's
    /// lock without waiting for it.
    fn reentrant_child_does_not_wait() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = lock_path(&dir);
        let mut out = Vec::new();
        let parent = HeavyRunLock::acquire(
            &LockSettings::at(&path),
            &holder("cargo xtask ci"),
            &mut out,
        );
        assert!(
            child_finishes(&path, Duration::from_secs(20)),
            "the child must run without waiting for its parent"
        );
        assert!(
            parent.holds() && locked(&path),
            "the parent keeps the lock while its child runs"
        );
    }

    /// A marker naming another lock file does not excuse the child from its
    /// own: it waits while a third run holds that one.
    fn marker_for_another_file_still_locks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let parent_lock = lock_path(&dir);
        let child_lock = dir.path().join("other.lock");
        let mut out = Vec::new();
        let _parent = HeavyRunLock::acquire(
            &LockSettings::at(&parent_lock),
            &holder("cargo xtask ci"),
            &mut out,
        );
        let third = File::create(&child_lock).expect("create the other lock");
        third.lock().expect("a third run holds the other lock");
        assert!(
            !child_finishes(&child_lock, Duration::from_secs(2)),
            "the child must wait for the lock it was not handed"
        );
    }

    fn opted_out_run_does_not_wait() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = lock_path(&dir);
        let mut out = Vec::new();
        let holder_run = HeavyRunLock::acquire(
            &LockSettings::at(&path),
            &holder("cargo xtask ci"),
            &mut out,
        );
        let other = acquire_elsewhere(LockSettings {
            opted_out: true,
            ..LockSettings::at(&path)
        });
        other
            .recv_timeout(Duration::from_secs(5))
            .expect("the opted-out run proceeds without waiting");
        assert!(
            holder_run.holds() && locked(&path),
            "the holder keeps the lock while the other run proceeds"
        );
    }

    fn unopenable_lock_file_warns_and_proceeds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let not_a_dir = dir.path().join("plain-file");
        std::fs::write(&not_a_dir, "").expect("create a plain file");
        let path = not_a_dir.join("heavy.lock");
        let mut out = Vec::new();
        let guard = HeavyRunLock::acquire(
            &LockSettings::at(&path),
            &holder("cargo xtask gate"),
            &mut out,
        );
        let printed = String::from_utf8_lossy(&out);
        assert!(!guard.holds(), "no lock is held");
        assert!(
            printed.matches("warning").count() == 1 && printed.contains("plain-file"),
            "one warning naming the file: {printed}"
        );
    }

    /// A refused relative override is reported, and the default is locked.
    fn relative_override_warns_and_locks_the_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = lock_path(&dir);
        let settings = LockSettings {
            rejected: Some(PathBuf::from("relative.lock")),
            ..LockSettings::at(&path)
        };
        let mut out = Vec::new();
        let guard = HeavyRunLock::acquire(&settings, &holder("cargo xtask gate"), &mut out);
        let printed = String::from_utf8_lossy(&out);
        assert!(
            printed.contains("relative.lock is not an absolute path"),
            "the refusal is reported: {printed}"
        );
        assert!(guard.holds() && locked(&path), "the default is locked");
    }

    /// A lock path that falls back to a relative temporary directory is not
    /// used: one warning, and the run proceeds unlocked.
    fn relative_temp_dir_warns_and_runs_unlocked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = lock_path(&dir);
        let settings = LockSettings {
            unlocated: true,
            ..LockSettings::at(&path)
        };
        let mut out = Vec::new();
        let guard = HeavyRunLock::acquire(&settings, &holder("cargo xtask gate"), &mut out);
        let printed = String::from_utf8_lossy(&out);
        assert!(!guard.holds(), "no lock is held");
        assert!(
            printed.matches("warning").count() == 1 && printed.contains("not absolute"),
            "one warning: {printed}"
        );
        assert!(!path.exists(), "no lock file is created");
    }

    #[test]
    fn host_lock_contract() {
        if std::env::var_os(CHILD).is_some() {
            let guard = HeavyRunLock::acquire(
                &LockSettings::from_env(),
                &holder("cargo xtask gate"),
                &mut std::io::stderr(),
            );
            drop(guard);
            return;
        }
        let _serial = TEST_GUARDS.lock().unwrap_or_else(PoisonError::into_inner);
        run_table(
            "host_lock",
            &[
                (
                    "second_acquirer_waits_for_the_first",
                    second_acquirer_waits_for_the_first,
                ),
                (
                    "reentrant_child_does_not_wait",
                    reentrant_child_does_not_wait,
                ),
                (
                    "marker_for_another_file_still_locks",
                    marker_for_another_file_still_locks,
                ),
                ("opted_out_run_does_not_wait", opted_out_run_does_not_wait),
                (
                    "relative_override_warns_and_locks_the_default",
                    relative_override_warns_and_locks_the_default,
                ),
                (
                    "relative_temp_dir_warns_and_runs_unlocked",
                    relative_temp_dir_warns_and_runs_unlocked,
                ),
                (
                    "unopenable_lock_file_warns_and_proceeds",
                    unopenable_lock_file_warns_and_proceeds,
                ),
            ],
        );
    }

    #[test]
    fn settings_come_from_the_environment() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        let alice = || Some("alice".to_owned());
        let tmp = std::env::temp_dir();
        let lock = |dir: PathBuf| dir.join("xtask-heavy.lock");
        /// A row name, the environment, the user, and the expected path on
        /// Windows and elsewhere.
        type PathRow = (
            &'static str,
            &'static [(&'static str, &'static str)],
            Option<&'static str>,
            [PathBuf; 2],
        );
        let alice_tmp = lock(tmp.join("flui-alice-872213e7"));
        let per_user = [
            lock(Path::new(LOCAL).join("flui")),
            lock(Path::new(HOME).join(".cache").join("flui")),
        ];
        let paths: [PathRow; 9] = [
            (
                "per_user_directory_variable",
                &[("LOCALAPPDATA", LOCAL), ("HOME", HOME)],
                Some("alice"),
                per_user.clone(),
            ),
            (
                "same_path_with_or_without_xdg_runtime_dir",
                &[
                    ("LOCALAPPDATA", LOCAL),
                    ("HOME", HOME),
                    ("XDG_RUNTIME_DIR", RUNTIME),
                ],
                Some("alice"),
                per_user.clone(),
            ),
            (
                "relative_user_directory_counts_as_unset",
                &[("LOCALAPPDATA", "local"), ("HOME", "home")],
                Some("alice"),
                [alice_tmp.clone(), alice_tmp.clone()],
            ),
            (
                "home_is_not_read_on_windows",
                &[("HOME", HOME)],
                Some("alice"),
                [alice_tmp.clone(), per_user[1].clone()],
            ),
            (
                "temp_fallback_names_the_user",
                &[],
                Some("alice"),
                [alice_tmp.clone(), alice_tmp.clone()],
            ),
            (
                "unnamed_user_shares_the_plain_directory",
                &[],
                None,
                [lock(tmp.join("flui")), lock(tmp.join("flui"))],
            ),
            (
                "absolute_override_wins",
                &[
                    ("LOCALAPPDATA", LOCAL),
                    ("HOME", HOME),
                    (LOCK_FILE, ABSOLUTE),
                ],
                Some("alice"),
                [PathBuf::from(ABSOLUTE), PathBuf::from(ABSOLUTE)],
            ),
            (
                "relative_override_is_refused",
                &[
                    ("LOCALAPPDATA", LOCAL),
                    ("HOME", HOME),
                    (LOCK_FILE, "relative.lock"),
                ],
                Some("alice"),
                per_user,
            ),
            (
                "relative_override_without_a_user_directory",
                &[(LOCK_FILE, "relative.lock")],
                Some("alice"),
                [alice_tmp.clone(), alice_tmp],
            ),
        ];
        let wrong: Vec<String> = paths
            .iter()
            .filter_map(|(row, env, user, [windows, other])| {
                [(true, "windows", windows), (false, "other", other)]
                    .into_iter()
                    .filter_map(|(platform, name, want)| {
                        let user = user.map(str::to_owned);
                        let got = LockSettings::for_platform(
                            platform,
                            vars(env),
                            || user,
                            || tmp.clone(),
                        );
                        (got.path != *want || got.unlocated).then(|| {
                            format!(
                                "{row} ({name}): {} != {}",
                                got.path.display(),
                                want.display()
                            )
                        })
                    })
                    .reduce(|a, b| format!("{a}; {b}"))
            })
            .collect();
        assert!(wrong.is_empty(), "lock path selection: {wrong:#?}");
        /// A row name, the environment, and whether a relative temporary
        /// directory leaves the run unlocated.
        type TempRow = (&'static str, &'static [(&'static str, &'static str)], bool);
        let relative_temp: [TempRow; 3] = [
            ("relative_temp_without_a_user_directory", &[], true),
            (
                "relative_temp_under_an_absolute_override",
                &[(LOCK_FILE, ABSOLUTE)],
                false,
            ),
            (
                "relative_temp_unused_beside_a_user_directory",
                &[("LOCALAPPDATA", LOCAL), ("HOME", HOME)],
                false,
            ),
        ];
        let wrong: Vec<&str> = relative_temp
            .iter()
            .filter(|(_, env, unlocated)| {
                LockSettings::from_vars(vars(env), alice, || PathBuf::from("relative-tmp"))
                    .unlocated
                    != *unlocated
            })
            .map(|(row, _, _)| *row)
            .collect();
        assert!(wrong.is_empty(), "relative temporary directory: {wrong:?}");
        let user = LockSettings::from_vars(vars(&[]), alice, std::env::temp_dir);
        assert!(!user.opted_out && user.held_by.is_none() && user.rejected.is_none());
        let refused = LockSettings::from_vars(
            vars(&[(LOCK_FILE, "relative.lock")]),
            alice,
            std::env::temp_dir,
        );
        assert_eq!(refused.rejected, Some(PathBuf::from("relative.lock")));
        let set = LockSettings::from_vars(
            vars(&[(LOCK_FILE, ABSOLUTE), (NO_LOCK, "1"), (LOCK_HELD, ABSOLUTE)]),
            alice,
            std::env::temp_dir,
        );
        assert!(set.opted_out && set.parent_holds());
        let other = LockSettings::from_vars(
            vars(&[
                (LOCK_FILE, ABSOLUTE),
                (NO_LOCK, "0"),
                (LOCK_HELD, "another.lock"),
            ]),
            alice,
            std::env::temp_dir,
        );
        assert!(!other.opted_out && !other.parent_holds());
    }

    /// A `LOCALAPPDATA` and a `HOME` that are absolute on this host, so
    /// both platforms' rules can be checked here.
    const LOCAL: &str = if cfg!(windows) {
        r"C:\Users\alice\AppData\Local"
    } else {
        "/users/alice/local"
    };
    const RUNTIME: &str = if cfg!(windows) {
        r"C:\run\user\1000"
    } else {
        "/run/user/1000"
    };
    const HOME: &str = if cfg!(windows) {
        r"C:\home\alice"
    } else {
        "/home/alice"
    };

    /// An absolute lock path on this platform.
    const ABSOLUTE: &str = if cfg!(windows) {
        r"C:\locks\heavy.lock"
    } else {
        "/locks/heavy.lock"
    };

    /// The fallback directory is one path component, distinct for names that
    /// read the same once sanitized, and fixed by a hash every build agrees
    /// on.
    #[test]
    fn fallback_directory_names_each_user_apart() {
        let name = |user: &str| fallback_dir_name(Some(user.to_owned()));
        assert_eq!(fnv1a32(b""), 0x811c_9dc5, "FNV-1a offset basis");
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c, "FNV-1a test vector");
        let colliding = [("a b", "a_b"), ("x/y", "x_y"), ("é", "_")];
        let merged: Vec<_> = colliding
            .iter()
            .filter(|(one, other)| name(one) == name(other))
            .collect();
        assert!(
            merged.is_empty(),
            "names that share a directory: {merged:?}"
        );
        assert_eq!(name("a"), "flui-a-e40c292c");
        for user in ["../x y", r"a\b", "x/y"] {
            let dir = name(user);
            assert_eq!(
                Path::new(&dir).components().count(),
                1,
                "`{user}` stays one component: {dir}"
            );
            assert!(dir.starts_with("flui-"), "`{user}` cannot climb: {dir}");
        }
    }

    /// An argument that is not Unicode, which clap accepts as a path, is
    /// shown lossily instead of panicking.
    #[test]
    fn holder_shows_a_non_unicode_argument() {
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[u16::from(b'a'), 0xD800])
        };
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'a', 0xFF])
        };
        let holder = Holder::from_args([OsString::from("device"), odd], PathBuf::from("checkout"));
        assert_eq!(holder.command, "cargo xtask device a\u{FFFD}");
    }
}
