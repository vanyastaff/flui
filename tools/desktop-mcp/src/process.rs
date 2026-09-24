//! Processes the session launched, and their cleanup.
//!
//! Every child is killed when the server exits. On Windows each child is also
//! placed in a kill-on-close job object, so the children die even when the
//! server itself is killed without running any cleanup.

use std::collections::HashMap;
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

/// The launched children, by pid.
#[derive(Debug)]
pub struct Children {
    running: Mutex<HashMap<u32, Child>>,
    #[cfg(target_os = "windows")]
    job: Option<crate::os::KillOnExitJob>,
}

impl Default for Children {
    fn default() -> Self {
        Self::new()
    }
}

impl Children {
    /// An empty set; on Windows it also creates the job object.
    pub fn new() -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
            #[cfg(target_os = "windows")]
            job: crate::os::KillOnExitJob::new()
                .inspect_err(|e| tracing::warn!("children will not be tied to a job: {e}"))
                .ok(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Child>> {
        // A poisoned map is still a correct map: every operation on it is a
        // single insert or remove.
        self.running
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
        let child = command
            .spawn()
            .map_err(|e| ToolError::platform(format!("launching `{}`", spec.program), e))?;
        // On Windows the kill-on-exit job is what ends children when the
        // server dies hard; a child that cannot join it is ended at once.
        #[cfg(target_os = "windows")]
        if let Some(job) = &self.job
            && let Err(e) = job.assign(&child)
        {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::NotSupported(format!(
                "`{}` started but could not join the kill-on-exit job ({e}), so it was ended",
                spec.program
            )));
        }
        let pid = child.id();
        let mut children = self.lock();
        // Reap the children that exited on their own, so a long session of
        // short-lived launches does not pile up handles or zombies.
        children.retain(|_, child| matches!(child.try_wait(), Ok(None)));
        children.insert(pid, child);
        Ok(pid)
    }

    /// Kills a child this session launched.
    pub fn kill(&self, pid: u32) -> ToolResult<Killed> {
        let mut child = self.lock().remove(&pid).ok_or_else(|| {
            ToolError::InvalidArgument(format!(
                "pid {pid} was not launched by this session; kill only ends processes started with launch"
            ))
        })?;
        Ok(end(pid, &mut child))
    }

    /// Whether `pid` was launched and has not been killed.
    #[cfg(test)]
    pub fn contains(&self, pid: u32) -> bool {
        self.lock().contains_key(&pid)
    }

    /// Kills every child. Idempotent.
    pub fn kill_all(&self) {
        let drained: Vec<(u32, Child)> = self.lock().drain().collect();
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
