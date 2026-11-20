//! A launched app's process on Linux: it leads a process group of its own, which the session ends
//! whole, and it dies with the driver (`PR_SET_PDEATHSIG`), however the driver goes.
//!
//! The parent-death signal fires when the *thread* that started the child ends, not its process:
//! a driver launched on one thread and moved to another (a [`Driver`](crate::Driver) is `Send`)
//! would see its app killed when the first thread returned. So the child is started from a thread
//! of the session's own ([`Starter`]), which lives until the session ends.

use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use rustix::process::{Pid, Signal};

/// The thread a child was started from, kept until the session ends: see the
/// [module docs](self).
pub(crate) struct Starter {
    hold: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Starter {
    fn drop(&mut self) {
        drop(self.hold.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Start `command` in a process group of its own, killed when its starter thread ends, from a
/// thread kept by the [`Starter`] returned with it.
///
/// # Errors
///
/// When the thread cannot start, or the command cannot.
pub(crate) fn spawn(mut command: Command) -> io::Result<(Child, Starter)> {
    let parent = rustix::process::getpid();
    command.process_group(0);
    #[expect(
        unsafe_code,
        reason = "two async-signal-safe calls between fork and exec: prctl and getppid"
    )]
    // SAFETY: the closure runs in the child between fork and exec and makes only async-signal-safe
    // system calls; it allocates nothing and takes no lock.
    unsafe {
        command.pre_exec(move || {
            rustix::process::set_parent_process_death_signal(Some(Signal::KILL))?;
            // The driver may have gone before the signal was asked for.
            if rustix::process::getppid() != Some(parent) {
                return Err(io::Error::other("the driver ended while the app started"));
            }
            Ok(())
        });
    }
    let (started, child) = mpsc::channel();
    let (hold, held) = mpsc::channel::<()>();
    let thread = thread::Builder::new()
        .name("flui-driver-starter".to_owned())
        .spawn(move || {
            let spawned = command.spawn();
            let running = spawned.is_ok();
            let _ = started.send(spawned);
            if running {
                // Until the session drops its end.
                let _ = held.recv();
            }
        })?;
    let starter = Starter {
        hold: Some(hold),
        thread: Some(thread),
    };
    let child = child
        .recv()
        .map_err(|_| io::Error::other("the starter thread ended before the app started"))??;
    Ok((child, starter))
}

/// Kill the process group `child` leads: the app and whatever it started.
pub(crate) fn kill_group(child: &Child) {
    let _ = rustix::process::kill_process_group(Pid::from_child(child), Signal::KILL);
}
