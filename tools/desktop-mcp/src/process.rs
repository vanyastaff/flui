//! Processes the session launched, and their cleanup.
//!
//! Every child is killed when the server exits. On Windows the server puts
//! itself in a kill-on-close job object, so every process it starts, and
//! everything those start, dies even when the server itself is killed
//! without running any cleanup.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{ToolError, ToolResult};

/// How to start a program.
#[derive(Debug, Clone, Default)]
pub struct LaunchSpec {
    /// Executable path or name on `PATH`.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
}

/// How a killed child ended.
#[derive(Debug, Clone, Serialize)]
pub struct Killed {
    /// The process id.
    pub pid: u32,
    /// Whether it had already exited before the kill.
    pub already_exited: bool,
    /// Its exit code, when the OS reports one.
    pub exit_code: Option<i32>,
}

/// How many exited children `kill` still recognizes.
const REMEMBERED_EXITS: usize = 256;

/// The launched children still tracked, and the recent ones that exited on
/// their own, so `kill` can say so instead of "not launched".
#[derive(Debug, Default)]
struct Tracked {
    running: HashMap<u32, Child>,
    /// `(pid, exit code)`, oldest first.
    exited: VecDeque<(u32, Option<i32>)>,
    /// Per pid, the launches that returned it and whose caller may hold it
    /// (an abandoned launch's caller never got the pid, so it is dropped):
    /// with two, a `kill` of the pid cannot tell which launch it means.
    returned: HashMap<u32, std::collections::HashSet<u64>>,
    /// Pids a `kill` is ending right now.
    ending: std::collections::HashSet<u32>,
    /// How many launches were recorded, and which one each pid's child in
    /// `running` came from: a pid a later launch reused names that one.
    launches: u64,
    launch_of: HashMap<u32, u64>,
    /// The start time read at each running child's spawn, through its
    /// handle.
    start_of: HashMap<u32, Option<u64>>,
    /// Launches between their spawn and their entry in `running`.
    launching: usize,
    /// Set by shutdown's `kill_all`: nothing is launched after it.
    closed: bool,
    /// Killed children not yet reaped (one stuck in uninterruptible IO
    /// outlives its kill): polled by `reap`, so none is left a zombie.
    unreaped: Vec<(u32, Child)>,
}

impl Tracked {
    /// Remembers how `pid` ended, dropping the oldest past the cap.
    fn remember(&mut self, pid: u32, code: Option<i32>) {
        self.exited.retain(|&(old, _)| old != pid);
        if self.exited.len() == REMEMBERED_EXITS {
            self.exited.pop_front();
        }
        self.exited.push_back((pid, code));
    }

    /// Moves the children that exited on their own to `exited`, so a long
    /// session of short-lived launches does not pile up handles or zombies.
    fn hold_unreaped(&mut self, pid: u32, mut child: Child) {
        if !matches!(child.try_wait(), Ok(Some(_))) {
            self.unreaped.push((pid, child));
        }
    }

    fn reap(&mut self) {
        self.unreaped
            .retain_mut(|(_, child)| !matches!(child.try_wait(), Ok(Some(_))));
        let mut done = Vec::new();
        self.running.retain(|&pid, child| match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                done.push((pid, status.code()));
                false
            }
            // Unknown is not exited: kept, so kill and shutdown can still
            // end it.
            Err(e) => {
                tracing::warn!("could not query whether process {pid} exited: {e}");
                true
            }
        });
        for (pid, code) in done {
            self.remember(pid, code);
        }
    }
}

/// How a launched process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exited {
    /// Its exit code, when the OS reports one.
    pub code: Option<i32>,
}

/// The launched children, by pid.
#[derive(Debug)]
pub struct Children {
    tracked: Mutex<Tracked>,
    /// Signalled when a launch in flight is recorded (or failed).
    settled: Condvar,
    /// The kill-on-exit job, or why there is none: then nothing is
    /// launched, since a hard kill of the server would leave it running.
    #[cfg(target_os = "windows")]
    job: Result<crate::os::KillOnExitJob, String>,
}

impl Children {
    /// An empty set; on Windows it also creates the job object.
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        let mut job = crate::os::KillOnExitJob::new().map_err(|e| {
            tracing::warn!("launch is disabled, there is no kill-on-exit job: {e}");
            e.to_string()
        });
        // Joining the job itself is what keeps grandchildren in: a child is
        // then in the job from its creation, before it can start anything.
        // Refused (a host job that forbids nesting), a child would run
        // outside the job until assigned, long enough to start one that
        // escapes, so launch is disabled instead.
        #[cfg(target_os = "windows")]
        if let Ok(own) = &mut job
            && let Err(e) = own.assign_self()
        {
            tracing::warn!("launch is disabled, the server cannot join its kill-on-exit job: {e}");
            job = Err(e.to_string());
        }
        Self {
            tracked: Mutex::new(Tracked::default()),
            settled: Condvar::new(),
            #[cfg(target_os = "windows")]
            job,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tracked> {
        // A poisoned set is still a correct set: every operation on it is a
        // single insert, remove or move.
        self.tracked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts the program with null stdio and records it.
    ///
    /// Returns the pid and the process's start time, read while this set
    /// still holds the child, so no later process can have taken the pid;
    /// and the launch's number, which [`Self::abandon`] takes.
    pub fn launch(&self, spec: &LaunchSpec) -> ToolResult<(u32, Option<u64>, u64)> {
        if spec.program.trim().is_empty() {
            return Err(ToolError::InvalidArgument("program is empty".into()));
        }
        // Reaped before the spawn: exited children still count against the
        // process limit until reaped, and a full limit would fail the spawn.
        {
            let mut tracked = self.lock();
            if tracked.closed {
                return Err(ToolError::ShuttingDown);
            }
            tracked.reap();
            tracked.launching += 1;
        }
        // Counted in flight until recorded or failed, so shutdown waits for
        // a spawn that is still running instead of missing its child.
        let _in_flight = InFlight(self);
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        #[cfg(target_os = "windows")]
        let job = self.job.as_ref().map_err(|e| {
            ToolError::NotSupported(format!(
                "launch is unavailable: the kill-on-exit job that ends launched processes if the server dies could not be created ({e})"
            ))
        })?;
        let child = command
            .spawn()
            .map_err(|e| ToolError::platform(format!("launching `{}`", spec.program), e))?;
        // On Windows the kill-on-exit job is what ends children when the
        // server dies hard; a child that cannot join it is ended at once.
        #[cfg(target_os = "windows")]
        if let Err(e) = job.assign(&child) {
            let pid = child.id();
            let mut child = child;
            let _ = end(pid, &mut child);
            self.lock().hold_unreaped(pid, child);
            return Err(ToolError::NotSupported(format!(
                "`{}` started but could not join the kill-on-exit job ({e}), so it was ended",
                spec.program
            )));
        }
        let pid = child.id();
        // Through the child's own handle: the launched process's start time
        // whatever the pid names by now.
        #[cfg(target_os = "windows")]
        let started = crate::os::child_started(&child);
        #[cfg(not(target_os = "windows"))]
        let started = crate::os::process_started(pid);
        let mut tracked = self.lock();
        // Shutdown gave up waiting and already ended the rest: this one is
        // ended here rather than left running.
        if tracked.closed {
            drop(tracked);
            let mut child = child;
            return Err(match end(pid, &mut child) {
                Ok(_) => {
                    self.lock().hold_unreaped(pid, child);
                    ToolError::ShuttingDown
                }
                // Kept, so shutdown's last pass tries it again.
                Err(e) => {
                    self.lock().running.insert(pid, child);
                    tracing::warn!(pid, "ending a process launched during shutdown failed: {e}");
                    ToolError::ShuttingDown
                }
            });
        }
        // A reused pid is a new process: its old exit no longer answers, and
        // a kill held for the old launch must not end this one.
        tracked.exited.retain(|&(old, _)| old != pid);
        tracked.running.insert(pid, child);
        tracked.launches += 1;
        let launch = tracked.launches;
        tracked.launch_of.insert(pid, launch);
        tracked.start_of.insert(pid, started);
        tracked.returned.entry(pid).or_default().insert(launch);
        Ok((pid, started, launch))
    }

    /// Kills a child this session launched; one that already exited on its
    /// own is reported as such.
    pub fn kill(&self, pid: u32) -> ToolResult<Killed> {
        self.end_tracked(pid, None)
    }

    /// Ends the process launch number `launch` started, for a launch whose
    /// caller will never be told its pid. Unlike [`Self::kill`] it holds for a
    /// pid two launches shared, since it names the launch: once a later
    /// launch took the pid over, this one's process is gone and the later
    /// one's is left alone.
    pub fn abandon(&self, pid: u32, launch: u64) -> ToolResult<Killed> {
        self.end_tracked(pid, Some(launch))
    }

    /// Ends the child under `pid`: for `kill` (`launch` `None`) unless two
    /// launches shared the pid, for `abandon` only if it is that launch's.
    /// Checked under the same lock that takes the child out, so a launch
    /// that reuses the pid in between is never the one ended.
    fn end_tracked(&self, pid: u32, launch: Option<u64>) -> ToolResult<Killed> {
        let mut tracked = self.lock();
        match launch {
            None if tracked.returned.get(&pid).is_some_and(|l| l.len() > 1) => {
                return Err(ToolError::InvalidArgument(format!(
                    "pid {pid} was returned by two launches of this session, so it cannot be told which one to end; the running one is ended when the server exits"
                )));
            }
            // Its caller never got the pid: it no longer counts as a launch
            // that returned it, whether or not a later launch has taken the
            // pid over since (whose own entry stays).
            Some(launch) => {
                if let Some(launches) = tracked.returned.get_mut(&pid) {
                    launches.remove(&launch);
                    if launches.is_empty() {
                        tracked.returned.remove(&pid);
                    }
                }
                // Its process is gone and the pid names a later launch's:
                // that one is left alone.
                if tracked.launch_of.get(&pid) != Some(&launch) {
                    return Ok(Killed {
                        pid,
                        already_exited: true,
                        exit_code: None,
                    });
                }
            }
            None => {}
        }
        if tracked.ending.contains(&pid) {
            return Err(ToolError::Busy(format!(
                "process {pid} is being ended by another kill"
            )));
        }
        if let Some(mut child) = tracked.running.remove(&pid) {
            tracked.ending.insert(pid);
            drop(tracked);
            let outcome = end(pid, &mut child);
            // The outcome is recorded under the same lock that clears
            // `ending`: a kill in between would otherwise find the pid
            // nowhere and call it never launched.
            let mut tracked = self.lock();
            tracked.ending.remove(&pid);
            self.settled.notify_all();
            return match outcome {
                // Remembered, so a second kill of the same pid says it has
                // exited rather than that it was never launched.
                Ok(killed) => {
                    tracked.remember(pid, killed.exit_code);
                    tracked.hold_unreaped(pid, child);
                    Ok(killed)
                }
                // Still running: keep it tracked, so it can be retried and
                // is ended again at shutdown.
                Err(e) => {
                    tracked.running.insert(pid, child);
                    Err(e)
                }
            };
        }
        if let Some(&(_, exit_code)) = tracked.exited.iter().rev().find(|&&(old, _)| old == pid) {
            return Ok(Killed {
                pid,
                already_exited: true,
                exit_code,
            });
        }
        // Launched and neither running nor ending: it exited, and its exit
        // record was dropped to make room for newer ones.
        if tracked.returned.contains_key(&pid) {
            return Ok(Killed {
                pid,
                already_exited: true,
                exit_code: None,
            });
        }
        // Never launched here: kill only ends processes started with launch.
        Err(ToolError::UnknownHandle {
            handle: pid.to_string(),
            kind: crate::error::HandleKind::Process,
        })
    }

    /// The start time read when this session launched the process now
    /// running under `pid`, if it did and one was read.
    pub fn launched_start(&self, pid: u32) -> Option<u64> {
        let tracked = self.lock();
        if !tracked.running.contains_key(&pid) {
            return None;
        }
        tracked.start_of.get(&pid).copied().flatten()
    }

    /// How a launched `pid` ended, once it has (`Some(exit code)`); `None`
    /// while it runs or when it is not this session's.
    pub fn exited(&self, pid: u32) -> Option<Exited> {
        let mut tracked = self.lock();
        tracked.reap();
        if tracked.running.contains_key(&pid) {
            return None;
        }
        tracked
            .exited
            .iter()
            .rev()
            .find(|&&(old, _)| old == pid)
            .map(|&(_, code)| Exited { code })
    }

    /// Whether `pid` was launched and has not been killed or reaped.
    #[cfg(test)]
    pub fn contains(&self, pid: u32) -> bool {
        self.lock().running.contains_key(&pid)
    }

    /// Kills every child. Idempotent.
    pub fn kill_all(&self) {
        let mut tracked = self.lock();
        tracked.closed = true;
        // A spawn still running would record its child after this pass, and
        // a kill in progress puts back one the OS refused to end: both are
        // waited for, bounded, since a spawn can hang on a network path.
        let until = Instant::now() + LAUNCH_SETTLE;
        while tracked.launching > 0 || !tracked.ending.is_empty() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                tracing::warn!(
                    "{} launches and {} kills still running at shutdown; a launch ends its process if it finishes",
                    tracked.launching,
                    tracked.ending.len()
                );
                break;
            }
            tracked = self
                .settled
                .wait_timeout(tracked, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        // Marked as being ended while out of `running`, so a `kill` in
        // between does not take them for exited.
        let mut drained: Vec<(u32, Child)> = tracked.running.drain().collect();
        tracked.ending.extend(drained.iter().map(|&(pid, _)| pid));
        drop(tracked);
        // Every kill first, then one shared wait: a child stuck in IO does
        // not hold up the others' kills, nor multiply the shutdown time.
        let mut failed = Vec::new();
        // One that already exited stays in the list, so the pass below
        // clears its `ending` mark like every other's.
        for (pid, child) in &mut drained {
            if matches!(child.try_wait(), Ok(Some(_))) {
                continue;
            }
            if let Err(e) = child.kill()
                && !matches!(child.try_wait(), Ok(Some(_)))
            {
                tracing::warn!("could not end launched child {pid} on shutdown: {e}");
                failed.push(*pid);
            }
        }
        let until = std::time::Instant::now() + REAP_WAIT;
        while std::time::Instant::now() < until
            && drained
                .iter_mut()
                .any(|(_, child)| !matches!(child.try_wait(), Ok(Some(_))))
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut tracked = self.lock();
        for (pid, child) in drained {
            tracked.ending.remove(&pid);
            if failed.contains(&pid) {
                // Kept, so the cleanup on drop tries it once more.
                tracked.running.insert(pid, child);
            } else {
                tracing::info!(pid, "ended launched child on shutdown");
                tracked.hold_unreaped(pid, child);
            }
        }
        self.settled.notify_all();
    }
}

/// How long shutdown waits for launches and kills in flight to settle.
const LAUNCH_SETTLE: Duration = Duration::from_secs(5);

/// One launch in flight; counted out when dropped, on every path.
struct InFlight<'a>(&'a Children);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.lock().launching -= 1;
        self.0.settled.notify_all();
    }
}

impl Drop for Children {
    /// The last attempt: what `kill_all` could not end is tried again, then
    /// reported.
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Ends `child`, waiting for it only once the kill went through: a kill
/// the OS refused (a helper that changed its credentials) leaves it
/// running, and waiting would block on it.
fn end(pid: u32, child: &mut Child) -> ToolResult<Killed> {
    let exited = |status: std::process::ExitStatus| Killed {
        pid,
        already_exited: true,
        exit_code: status.code(),
    };
    if let Ok(Some(status)) = child.try_wait() {
        return Ok(exited(status));
    }
    if let Err(e) = child.kill() {
        // It may have exited between the two calls.
        if let Ok(Some(status)) = child.try_wait() {
            return Ok(exited(status));
        }
        return Err(ToolError::platform(format!("ending process {pid}"), e));
    }
    // Polled, bounded: a killed process stuck in uninterruptible IO stays
    // until that IO returns, and waiting on it would hold a process slot,
    // or shutdown, for as long.
    let until = std::time::Instant::now() + REAP_WAIT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => break None,
        }
    };
    Ok(Killed {
        pid,
        already_exited: false,
        exit_code: status.and_then(|s| s.code()),
    })
}

/// How long `end` waits for a killed process to be reaped.
const REAP_WAIT: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_launched_after_shutdown() {
        let children = Children::new();
        children.kill_all();
        let spec = LaunchSpec {
            program: "definitely-not-a-program".into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
        };
        assert!(matches!(
            children.launch(&spec),
            Err(ToolError::ShuttingDown)
        ));
    }

    #[test]
    fn kill_refuses_foreign_pids() {
        let children = Children::new();
        let err = children
            .kill(std::process::id())
            .expect_err("BUG: the test process was not launched by the set");
        assert_eq!(err.code(), "unknown_handle", "{err}");
    }

    #[test]
    fn empty_program_is_rejected() {
        let children = Children::new();
        assert!(matches!(
            children.launch(&LaunchSpec::default()),
            Err(ToolError::InvalidArgument(_))
        ));
    }

    /// A child that exited on its own and was reaped by a later launch is
    /// reported as exited, not as a pid this session never launched.
    #[test]
    fn a_reaped_child_is_reported_as_exited() {
        let children = Children::new();
        let program = std::env::current_exe()
            .expect("BUG: the test binary has a path")
            .to_string_lossy()
            .into_owned();
        let spec = LaunchSpec {
            program,
            args: vec!["--list".into()],
            ..LaunchSpec::default()
        };
        let first = children
            .launch(&spec)
            .expect("BUG: relaunching the test binary works")
            .0;
        // Wait for `--list` to finish, then launch again: that reaps it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while children
            .lock()
            .running
            .get_mut(&first)
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
        {
            assert!(std::time::Instant::now() < deadline, "BUG: `--list` exits");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let second = children
            .launch(&spec)
            .expect("BUG: relaunching the test binary works")
            .0;
        assert!(!children.contains(first), "the exited child was reaped");
        assert_eq!(children.exited(first), Some(Exited { code: Some(0) }));
        let killed = children
            .kill(first)
            .expect("BUG: a reaped child is still known");
        assert!(killed.already_exited);
        assert_eq!(killed.exit_code, Some(0));
        let again = children
            .kill(first)
            .expect("BUG: a second kill says it exited");
        assert!(again.already_exited);
        let _ = children.kill(second);
    }

    /// Abandoning names the launch, not only the pid: a launch whose pid a
    /// later launch took over leaves that later process running.
    #[test]
    fn abandon_ends_only_its_own_launch() {
        let children = Children::new();
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".into(), "60".into(), "127.0.0.1".into()])
        } else {
            ("sleep", vec!["60".into()])
        };
        let (pid, _, launch) = children
            .launch(&LaunchSpec {
                program: program.into(),
                args,
                ..LaunchSpec::default()
            })
            .expect("BUG: a long-running system program starts");
        let other = children
            .abandon(pid, launch + 1)
            .expect("BUG: another launch's number is answered");
        assert!(other.already_exited, "another launch's process is gone");
        assert!(children.contains(pid), "and this one was left running");
        let own = children
            .abandon(pid, launch)
            .expect("BUG: its own launch is ended");
        assert!(!own.already_exited);
        assert!(!children.contains(pid));
    }

    /// A launch abandoned after a later launch took its pid over drops its
    /// own claim on the pid: the later launch is then the only one that
    /// returned it, and `kill` ends it rather than refusing it as shared.
    #[test]
    fn an_abandoned_launch_releases_a_reused_pid() {
        let children = Children::new();
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".into(), "60".into(), "127.0.0.1".into()])
        } else {
            ("sleep", vec!["60".into()])
        };
        let (pid, _, launch) = children
            .launch(&LaunchSpec {
                program: program.into(),
                args,
                ..LaunchSpec::default()
            })
            .expect("BUG: a long-running system program starts");
        // An earlier launch that returned the same pid, whose process exited
        // and was reaped before this one reused the number.
        let earlier = launch + 100;
        children
            .lock()
            .returned
            .entry(pid)
            .or_default()
            .insert(earlier);
        assert!(matches!(
            children.kill(pid),
            Err(ToolError::InvalidArgument(_))
        ));
        let abandoned = children
            .abandon(pid, earlier)
            .expect("BUG: an abandon is answered");
        assert!(abandoned.already_exited, "its own process is gone");
        assert!(children.contains(pid), "the later launch's is left running");
        let killed = children.kill(pid).expect("BUG: no longer shared");
        assert!(!killed.already_exited);
    }

    #[test]
    fn launch_then_kill_round_trip() {
        let children = Children::new();
        // A child that runs well past the kill, so the kill is what ends it.
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".into(), "60".into(), "127.0.0.1".into()])
        } else {
            ("sleep", vec!["60".into()])
        };
        let pid = children
            .launch(&LaunchSpec {
                program: program.into(),
                args,
                ..LaunchSpec::default()
            })
            .expect("BUG: a long-running system program starts")
            .0;
        assert!(children.contains(pid));
        let killed = children
            .kill(pid)
            .expect("BUG: a launched pid can be killed");
        assert_eq!(killed.pid, pid);
        assert!(!killed.already_exited, "the kill ended it");
        assert!(!children.contains(pid));
    }

    /// Shutdown ends every running child, within its bound, and a kill
    /// afterwards reports the process as ended rather than never launched.
    #[test]
    fn kill_all_ends_running_children() {
        let children = Children::new();
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".into(), "60".into(), "127.0.0.1".into()])
        } else {
            ("sleep", vec!["60".into()])
        };
        let pids: Vec<u32> = (0..3)
            .map(|_| {
                children
                    .launch(&LaunchSpec {
                        program: program.into(),
                        args: args.clone(),
                        ..LaunchSpec::default()
                    })
                    .expect("BUG: a long-running system program starts")
                    .0
            })
            .collect();
        let started = std::time::Instant::now();
        children.kill_all();
        assert!(
            started.elapsed() < LAUNCH_SETTLE + REAP_WAIT,
            "kill_all is bounded"
        );
        for pid in pids {
            assert!(!children.contains(pid), "{pid} was ended");
            let again = children.kill(pid).expect("BUG: a launched pid is known");
            assert!(again.already_exited);
        }
    }
}
