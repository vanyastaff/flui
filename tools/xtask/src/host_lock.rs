//! One heavy run at a time for the user on this machine: the commands that
//! build or test the workspace (`main`'s `Command::is_heavy`) queue behind an
//! exclusive file lock.
//!
//! Several checkouts and worktrees of this repository often share one
//! machine, and two workspace builds at once oversubscribe it until every run
//! looks hung. The lock file sits in a fixed per-user directory, so every
//! checkout the user has finds the same one: `%LOCALAPPDATA%\flui\` on
//! Windows; elsewhere `$XDG_RUNTIME_DIR/flui/`, or `~/.cache/flui/` without
//! it. `FLUI_XTASK_LOCK_FILE` names another file and `FLUI_XTASK_NO_LOCK=1`
//! skips the lock.
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
    opted_out: bool,
    held_by: Option<PathBuf>,
}

impl LockSettings {
    /// The settings this process's environment gives.
    pub(crate) fn from_env() -> Self {
        Self::from_vars(|key| std::env::var_os(key))
    }

    fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |key| var(key).filter(|value| !value.is_empty());
        let user_dir = if cfg!(windows) {
            set("LOCALAPPDATA").map(PathBuf::from)
        } else {
            set("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".cache")))
        };
        Self {
            path: set(LOCK_FILE).map_or_else(
                || {
                    user_dir
                        .unwrap_or_else(std::env::temp_dir)
                        .join("flui")
                        .join("xtask-heavy.lock")
                },
                PathBuf::from,
            ),
            opted_out: set(NO_LOCK).is_some_and(|value| value == "1"),
            held_by: set(LOCK_HELD).map(PathBuf::from),
        }
    }

    /// Lock at `path`, with no opt-out and no parent holding it.
    #[cfg(test)]
    pub(crate) fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
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
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self {
            command: format!("cargo xtask {}", args.join(" ")),
            checkout: crate::util::repo_root(),
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
        let user = LockSettings::from_vars(vars(&[
            ("LOCALAPPDATA", "local"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("HOME", "home"),
        ]));
        let user_dir = if cfg!(windows) { "local" } else { "runtime" };
        assert_eq!(
            user.path,
            Path::new(user_dir).join("flui").join("xtask-heavy.lock")
        );
        assert!(!user.opted_out && user.held_by.is_none());
        if !cfg!(windows) {
            let home = LockSettings::from_vars(vars(&[("HOME", "home")]));
            assert_eq!(home.path, Path::new("home/.cache/flui/xtask-heavy.lock"));
        }
        let set = LockSettings::from_vars(vars(&[
            ("LOCALAPPDATA", "local"),
            ("XDG_RUNTIME_DIR", "runtime"),
            (LOCK_FILE, "elsewhere.lock"),
            (NO_LOCK, "1"),
            (LOCK_HELD, "elsewhere.lock"),
        ]));
        assert_eq!(set.path, PathBuf::from("elsewhere.lock"));
        assert!(set.opted_out && set.parent_holds());
        let other = LockSettings::from_vars(vars(&[
            (LOCK_FILE, "elsewhere.lock"),
            (NO_LOCK, "0"),
            (LOCK_HELD, "another.lock"),
        ]));
        assert!(!other.opted_out && !other.parent_holds());
    }
}
