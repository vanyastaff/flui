//! Processes the session launched, and their cleanup.
//!
//! Every child is killed when the server exits. On Windows the server puts
//! itself in a kill-on-close job object, so every process it starts, and
//! everything those start, dies even when the server itself is killed
//! without running any cleanup.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

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
    fn reap(&mut self) {
        let mut done = Vec::new();
        self.running.retain(|&pid, child| match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                done.push((pid, status.code()));
                false
            }
            Err(_) => {
                done.push((pid, None));
                false
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
    /// still holds the child, so no later process can have taken the pid.
    pub fn launch(&self, spec: &LaunchSpec) -> ToolResult<(u32, Option<u64>)> {
        if spec.program.trim().is_empty() {
            return Err(ToolError::InvalidArgument("program is empty".into()));
        }
        // Reaped before the spawn: exited children still count against the
        // process limit until reaped, and a full limit would fail the spawn.
        self.lock().reap();
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
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::NotSupported(format!(
                "`{}` started but could not join the kill-on-exit job ({e}), so it was ended",
                spec.program
            )));
        }
        let pid = child.id();
        let started = crate::os::process_started(pid);
        let mut tracked = self.lock();
        // A reused pid is a new process: its old exit no longer answers.
        tracked.exited.retain(|&(old, _)| old != pid);
        tracked.running.insert(pid, child);
        Ok((pid, started))
    }

    /// Kills a child this session launched; one that already exited on its
    /// own is reported as such.
    pub fn kill(&self, pid: u32) -> ToolResult<Killed> {
        let mut tracked = self.lock();
        if let Some(mut child) = tracked.running.remove(&pid) {
            drop(tracked);
            return match end(pid, &mut child) {
                // Remembered, so a second kill of the same pid says it has
                // exited rather than that it was never launched.
                Ok(killed) => {
                    self.lock().remember(pid, killed.exit_code);
                    Ok(killed)
                }
                // Still running: keep it tracked, so it can be retried and
                // is ended again at shutdown.
                Err(e) => {
                    self.lock().running.insert(pid, child);
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
        Err(ToolError::InvalidArgument(format!(
            "pid {pid} was not launched by this session; kill only ends processes started with launch"
        )))
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
        let drained: Vec<(u32, Child)> = self.lock().running.drain().collect();
        for (pid, mut child) in drained {
            match end(pid, &mut child) {
                Ok(killed) => tracing::info!(?killed, "ended launched child on shutdown"),
                Err(e) => tracing::warn!("could not end launched child on shutdown: {e}"),
            }
        }
    }
}

impl Drop for Children {
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
    let status = child.wait().ok();
    Ok(Killed {
        pid,
        already_exited: false,
        exit_code: status.and_then(|s| s.code()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_refuses_foreign_pids() {
        let children = Children::new();
        let err = children
            .kill(std::process::id())
            .expect_err("BUG: the test process was not launched by the set");
        assert!(err.to_string().contains("not launched"), "{err}");
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
}
