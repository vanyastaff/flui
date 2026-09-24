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
        for exit in done {
            if self.exited.len() == REMEMBERED_EXITS {
                self.exited.pop_front();
            }
            self.exited.push_back(exit);
        }
    }
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

impl Default for Children {
    fn default() -> Self {
        Self::new()
    }
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
        // Refused (a host job that forbids nesting), each child still joins
        // right after its spawn.
        #[cfg(target_os = "windows")]
        if let Ok(job) = &mut job
            && let Err(e) = job.assign_self()
        {
            tracing::warn!("processes the children start may outlive a hard kill: {e}");
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
    pub fn launch(&self, spec: &LaunchSpec) -> ToolResult<u32> {
        if spec.program.trim().is_empty() {
            return Err(ToolError::InvalidArgument("program is empty".into()));
        }
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
        let mut tracked = self.lock();
        tracked.reap();
        // A reused pid is a new process: its old exit no longer answers.
        tracked.exited.retain(|&(old, _)| old != pid);
        tracked.running.insert(pid, child);
        Ok(pid)
    }

    /// Kills a child this session launched; one that already exited on its
    /// own is reported as such.
    pub fn kill(&self, pid: u32) -> ToolResult<Killed> {
        let mut tracked = self.lock();
        if let Some(mut child) = tracked.running.remove(&pid) {
            drop(tracked);
            return Ok(end(pid, &mut child));
        }
        let exited = tracked.exited.iter().position(|&(old, _)| old == pid);
        if let Some(at) = exited
            && let Some((_, exit_code)) = tracked.exited.remove(at)
        {
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

    /// Whether `pid` was launched and has not been killed or reaped.
    #[cfg(test)]
    pub fn contains(&self, pid: u32) -> bool {
        self.lock().running.contains_key(&pid)
    }

    /// Kills every child. Idempotent.
    pub fn kill_all(&self) {
        let drained: Vec<(u32, Child)> = self.lock().running.drain().collect();
        for (pid, mut child) in drained {
            let killed = end(pid, &mut child);
            tracing::info!(?killed, "ended launched child on shutdown");
        }
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        self.kill_all();
    }
}

fn end(pid: u32, child: &mut Child) -> Killed {
    if let Ok(Some(status)) = child.try_wait() {
        return Killed {
            pid,
            already_exited: true,
            exit_code: status.code(),
        };
    }
    let _ = child.kill();
    let status = child.wait().ok();
    Killed {
        pid,
        already_exited: false,
        exit_code: status.and_then(|s| s.code()),
    }
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
            .expect("BUG: relaunching the test binary works");
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
            .expect("BUG: relaunching the test binary works");
        assert!(!children.contains(first), "the exited child was reaped");
        let killed = children
            .kill(first)
            .expect("BUG: a reaped child is still known");
        assert!(killed.already_exited);
        assert_eq!(killed.exit_code, Some(0));
        let _ = children.kill(second);
    }

    #[test]
    fn launch_then_kill_round_trip() {
        let children = Children::new();
        let program = std::env::current_exe().expect("BUG: the test binary has a path");
        // The test binary itself; `--list` prints the test names and exits.
        let pid = children
            .launch(&LaunchSpec {
                program: program.to_string_lossy().into_owned(),
                args: vec!["--list".into()],
                ..LaunchSpec::default()
            })
            .expect("BUG: relaunching the test binary works");
        assert!(children.contains(pid));
        let killed = children
            .kill(pid)
            .expect("BUG: a launched pid can be killed");
        assert_eq!(killed.pid, pid);
        assert!(!children.contains(pid));
    }
}
