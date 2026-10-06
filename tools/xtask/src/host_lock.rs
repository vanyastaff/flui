//! One heavy run per host: `check-changed`, `test`, `ci`, `ci-full`, `gate`
//! and `gpu-test` queue behind a host-wide exclusive file lock.
//!
//! Several checkouts and worktrees of this repository often share one machine,
//! and two workspace builds at once oversubscribe it until every run looks
//! hung. The lock file lives in the system temporary directory, so every
//! checkout on the host finds the same one; `FLUI_XTASK_LOCK_FILE` names
//! another and `FLUI_XTASK_NO_LOCK=1` skips the lock.
//!
//! The lock is the operating system's advisory lock on an open file
//! ([`File::lock`]): it is released when the handle closes, and so when the
//! holding process exits or crashes. A dead run never leaves a stale lock
//! behind, and there is nothing to clean up.
//!
//! Composite commands (`ci` runs `gate` and `test`) call each other in this
//! process, under the one lock `main` took. A child process this one spawns
//! (through [`crate::tasks`]' `Cmd`) carries [`LOCK_HELD`] with this
//! process's id, and a heavy command started with it set does not lock again:
//! its parent already holds the lock and is waiting for it.
//!
//! The lock never fails a run: if the file cannot be opened or locked, a
//! warning says so and the command runs unlocked.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Names another lock file instead of the one in the temporary directory.
const LOCK_FILE: &str = "FLUI_XTASK_LOCK_FILE";
/// `1` skips the lock.
const NO_LOCK: &str = "FLUI_XTASK_NO_LOCK";
/// Set by a process holding the lock for the processes it spawns: the id of
/// the holder.
pub(crate) const LOCK_HELD: &str = "FLUI_XTASK_LOCK_HELD";

/// How often a waiting run checks the lock again.
const POLL: Duration = Duration::from_millis(100);
/// How often a waiting run repeats which run it waits for.
const REMIND: Duration = Duration::from_secs(60);

/// Guards this process holds; a spawned child is marked while one is live.
static HELD_HERE: AtomicUsize = AtomicUsize::new(0);

/// Marks `child` as spawned by the lock's holder when this process holds it,
/// so a heavy command the child runs does not wait for its own parent.
pub(crate) fn mark_child(child: &mut Command) {
    if HELD_HERE.load(Ordering::Acquire) > 0 {
        child.env(LOCK_HELD, std::process::id().to_string());
    }
}

/// Where the lock is and whether to take it, read from the environment.
#[derive(Debug, Clone)]
pub(crate) struct LockSettings {
    path: PathBuf,
    opted_out: bool,
    held_by: Option<OsString>,
}

impl LockSettings {
    /// The settings this process's environment gives.
    pub(crate) fn from_env() -> Self {
        Self::from_vars(|key| std::env::var_os(key))
    }

    fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |key| var(key).filter(|value| !value.is_empty());
        Self {
            path: set(LOCK_FILE).map_or_else(
                || std::env::temp_dir().join("flui-xtask-heavy.lock"),
                PathBuf::from,
            ),
            opted_out: set(NO_LOCK).is_some_and(|value| value == "1"),
            held_by: set(LOCK_HELD),
        }
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

/// Holds the host-wide lock until dropped (or until the process exits).
#[derive(Debug)]
pub(crate) struct HeavyRunLock {
    held: Option<(File, PathBuf)>,
}

impl HeavyRunLock {
    /// Takes the lock, waiting while another run holds it, and printing to
    /// `out` which run that is: once, then at most every minute.
    ///
    /// Returns without the lock when `settings` opt out, when a parent
    /// process already holds it, or when the lock file cannot be used (after
    /// a warning).
    pub(crate) fn acquire(settings: &LockSettings, me: &Holder, out: &mut dyn Write) -> Self {
        let unheld = Self { held: None };
        if settings.opted_out || settings.held_by.is_some() {
            return unheld;
        }
        let path = &settings.path;
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
        {
            Ok(file) => file,
            Err(error) => {
                warn(out, path, &error);
                return unheld;
            }
        };
        let mut reminded: Option<Instant> = None;
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    if reminded.is_none_or(|at| at.elapsed() >= REMIND) {
                        let _ = writeln!(out, "xtask: waiting for {}", holder_line(path));
                        reminded = Some(Instant::now());
                    }
                    std::thread::sleep(POLL);
                }
                Err(TryLockError::Error(error)) => {
                    warn(out, path, &error);
                    return unheld;
                }
            }
        }
        // Who holds it, for a run that starts waiting; best effort. Written
        // to a sidecar: on Windows the lock keeps others from reading the
        // locked file itself.
        let _ = std::fs::write(
            sidecar(path),
            format!(
                "{}\n{}\n{}\n",
                std::process::id(),
                me.command,
                me.checkout.display()
            ),
        );
        HELD_HERE.fetch_add(1, Ordering::AcqRel);
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
            // Before unlocking, so the next holder's record is never removed.
            let _ = std::fs::remove_file(sidecar(&path));
            HELD_HERE.fetch_sub(1, Ordering::AcqRel);
            let _ = file.unlock();
        }
    }
}

fn warn(out: &mut dyn Write, path: &Path, error: &std::io::Error) {
    let _ = writeln!(
        out,
        "xtask: warning: cannot use the host-wide run lock {}: {error}; running without it",
        path.display()
    );
}

fn sidecar(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".holder");
    PathBuf::from(name)
}

/// "`cargo xtask test` (pid 42, D:\flui) to finish", or a generic line when
/// the holder left no readable record.
fn holder_line(path: &Path) -> String {
    let record = std::fs::read_to_string(sidecar(path)).unwrap_or_default();
    let mut lines = record.lines();
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

    use super::*;
    use crate::table_test::run_table;

    fn settings(path: &Path) -> LockSettings {
        LockSettings {
            path: path.to_path_buf(),
            opted_out: false,
            held_by: None,
        }
    }

    fn holder(command: &str) -> Holder {
        Holder {
            command: command.to_owned(),
            checkout: PathBuf::from("checkout-under-test"),
        }
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
        let path = dir.path().join("heavy.lock");
        let mut out = Vec::new();
        let first = HeavyRunLock::acquire(
            &settings(&path),
            &holder("cargo xtask check-changed"),
            &mut out,
        );
        assert!(
            first.holds() && locked(&path),
            "the first run holds the lock"
        );
        let second = acquire_elsewhere(settings(&path));
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

    /// A run that does not lock returns at once, and leaves the holder's lock
    /// in place.
    fn passes_by_a_held_lock(skipping: impl FnOnce(&Path) -> LockSettings) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("heavy.lock");
        let mut out = Vec::new();
        let parent = HeavyRunLock::acquire(&settings(&path), &holder("cargo xtask ci"), &mut out);
        let child = acquire_elsewhere(skipping(&path));
        child
            .recv_timeout(Duration::from_secs(5))
            .expect("the run proceeds without waiting");
        assert!(
            parent.holds() && locked(&path),
            "the holder keeps the lock while the other run proceeds"
        );
    }

    /// Set for a child process of [`reentrant_child_does_not_wait`]: take
    /// the lock as `main` would, then exit.
    const CHILD: &str = "FLUI_XTASK_LOCK_TEST_CHILD";

    /// A real child process, marked by [`mark_child`] and reading its
    /// settings from its environment as `main` does, runs under the parent's
    /// lock without waiting for it.
    fn reentrant_child_does_not_wait() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("heavy.lock");
        let mut out = Vec::new();
        let parent = HeavyRunLock::acquire(&settings(&path), &holder("cargo xtask ci"), &mut out);
        let mut child = Command::new(std::env::current_exe().expect("test executable"));
        child
            .args(["host_lock::tests::host_lock_contract", "--exact"])
            .env_remove(NO_LOCK)
            .env_remove(LOCK_HELD)
            .env(LOCK_FILE, &path)
            .env(CHILD, "1");
        mark_child(&mut child);
        let mut child = child.spawn().expect("spawn the child run");
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll the child") {
                break Some(status);
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(POLL);
        };
        assert!(
            status.is_some_and(|status| status.success()),
            "the child must run without waiting for its parent ({status:?})"
        );
        assert!(
            parent.holds() && locked(&path),
            "the parent keeps the lock while its child runs"
        );
    }

    fn opted_out_run_does_not_wait() {
        passes_by_a_held_lock(|path| LockSettings {
            opted_out: true,
            ..settings(path)
        });
    }

    fn unopenable_lock_file_warns_and_proceeds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("no-such-dir").join("heavy.lock");
        let mut out = Vec::new();
        let guard = HeavyRunLock::acquire(&settings(&path), &holder("cargo xtask gate"), &mut out);
        let printed = String::from_utf8_lossy(&out);
        assert!(!guard.holds(), "no lock is held");
        assert!(
            printed.matches("warning").count() == 1 && printed.contains("no-such-dir"),
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
        let none = LockSettings::from_vars(vars(&[]));
        assert_eq!(
            none.path,
            std::env::temp_dir().join("flui-xtask-heavy.lock")
        );
        assert!(!none.opted_out && none.held_by.is_none());
        let set = LockSettings::from_vars(vars(&[
            (LOCK_FILE, "elsewhere.lock"),
            (NO_LOCK, "1"),
            (LOCK_HELD, "42"),
        ]));
        assert_eq!(set.path, PathBuf::from("elsewhere.lock"));
        assert!(set.opted_out && set.held_by.is_some());
        let empty = LockSettings::from_vars(vars(&[(NO_LOCK, "0"), (LOCK_HELD, "")]));
        assert!(!empty.opted_out && empty.held_by.is_none());
    }
}
