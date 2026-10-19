//! Memory pressure from the kernel's pressure triggers.
//!
//! A thread holds one trigger per level open on `/proc/pressure/memory` and
//! sleeps in `poll` until the kernel marks one readable (`POLLPRI`), which it
//! does when the stall the trigger names has lasted its threshold within its
//! window. Which trigger the kernel accepts, and which level each stands for,
//! is the model's
//! ([`lifecycle_model::memory_pressure`](crate::platforms::linux::lifecycle_model::memory_pressure)).
//!
//! A kernel without pressure information, or one that refuses a process the
//! trigger, leaves the level without a source: it is logged once and the
//! other level, if it has one, runs on.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::fd::{AsFd, OwnedFd},
    sync::Arc,
    thread::JoinHandle,
};

use flui_platform_api::SystemNotice;
use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, eventfd, poll},
    io::Errno,
};

use super::{HostEvent, Inbox};
use crate::platforms::linux::lifecycle_model::memory_pressure::{
    Stall, Trigger, level_of, triggers_for,
};

const PRESSURE_FILE: &str = "/proc/pressure/memory";

/// The thread that waits for pressure. Dropping it stops the thread.
pub(super) struct PressureWatcher {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl PressureWatcher {
    /// Set the triggers and start waiting; `None` when no trigger could be set
    /// or the thread could not start.
    pub(super) fn start(inbox: &Arc<Inbox>) -> Option<Self> {
        let triggers: Vec<(Trigger, File)> = [Stall::Partial, Stall::Full]
            .into_iter()
            .filter_map(set_trigger)
            .collect();
        if triggers.is_empty() {
            tracing::info!(
                "no memory pressure trigger could be set; memory pressure notices are off"
            );
            return None;
        }
        let stop = match eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK) {
            Ok(stop) => Arc::new(stop),
            Err(error) => {
                tracing::warn!(%error, "cannot create the memory pressure thread's stop signal");
                return None;
            }
        };
        let thread = std::thread::Builder::new()
            .name("flui-memory-pressure".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let inbox = Arc::clone(inbox);
                move || run(triggers, &stop, &inbox)
            });
        match thread {
            Ok(thread) => Some(Self {
                stop,
                thread: Some(thread),
            }),
            Err(error) => {
                tracing::warn!(%error, "cannot start the memory pressure thread");
                None
            }
        }
    }
}

impl Drop for PressureWatcher {
    fn drop(&mut self) {
        // The eventfd stays readable once written, so the thread sees it
        // whether it is polling or about to.
        if let Err(error) = rustix::io::write(self.stop.as_fd(), &1_u64.to_ne_bytes()) {
            tracing::error!(%error, "cannot signal the memory pressure thread to stop");
            // Joining a thread that was never told to stop would hang.
            return;
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the memory pressure thread ended in a panic");
        }
    }
}

/// Open the pressure file and set the first trigger for `level` the kernel
/// accepts. Each trigger needs a file of its own: the kernel allows one per
/// descriptor.
fn set_trigger(stall: Stall) -> Option<(Trigger, File)> {
    for trigger in triggers_for(stall) {
        let mut file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(PRESSURE_FILE)
        {
            Ok(file) => file,
            Err(error) => {
                tracing::debug!(%error, ?stall, "cannot open {PRESSURE_FILE}");
                return None;
            }
        };
        match file.write_all(trigger.request().as_bytes()) {
            Ok(()) => return Some((trigger, file)),
            // The kernel refuses a window an unprivileged process may not use
            // with `EPERM`, and a window or threshold out of range with
            // `EINVAL`: the next trigger is the one that process may set.
            Err(error) => {
                tracing::debug!(%error, request = trigger.request(), "the kernel refused a pressure trigger");
            }
        }
    }
    tracing::info!(
        ?stall,
        "the kernel accepts no pressure trigger for this level"
    );
    None
}

fn run(mut triggers: Vec<(Trigger, File)>, stop: &OwnedFd, inbox: &Inbox) {
    loop {
        let mut fds = Vec::with_capacity(triggers.len() + 1);
        fds.push(PollFd::new(stop, PollFlags::IN));
        fds.extend(
            triggers
                .iter()
                .map(|(_, file)| PollFd::new(file, PollFlags::PRI)),
        );
        match poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => {
                tracing::warn!(%error, "memory pressure polling failed; memory pressure notices are off");
                return;
            }
        }
        if fds[0]
            .revents()
            .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP)
        {
            return;
        }
        let mut fired = Vec::new();
        let mut gone = Vec::new();
        for (index, fd) in fds[1..].iter().enumerate() {
            let revents = fd.revents();
            if revents.contains(PollFlags::PRI) {
                fired.push(triggers[index].0.stall());
            }
            // The kernel destroys a trigger it can no longer serve and reports
            // it as an error.
            if revents.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                gone.push(index);
            }
        }
        drop(fds);
        for index in gone.into_iter().rev() {
            let (trigger, _) = triggers.remove(index);
            tracing::warn!(
                request = trigger.request(),
                "the kernel ended a pressure trigger"
            );
        }
        if let Some(level) = level_of(fired) {
            inbox.post(
                HostEvent::Notice(SystemNotice::MemoryPressure(level)),
                false,
            );
        }
        if triggers.is_empty() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading the real file needs a kernel with pressure information, which a
    /// CI runner may lack; the test then has nothing to check.
    #[test]
    fn the_kernels_pressure_file_parses_when_there_is_one() {
        let Ok(text) = std::fs::read_to_string(PRESSURE_FILE) else {
            return;
        };
        let report =
            crate::platforms::linux::lifecycle_model::memory_pressure::PressureReport::parse(&text)
                .expect("the kernel's memory pressure file has a some and a full line");
        assert_eq!(report.some.stall, Stall::Partial);
        assert_eq!(report.full.stall, Stall::Full);
    }
}
