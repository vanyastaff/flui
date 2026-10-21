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

/// Open the pressure file and set the first trigger for `stall` the kernel
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
        match file.write_all(&trigger.request_bytes()) {
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

fn run<F: AsFd>(mut triggers: Vec<(Trigger, F)>, stop: &OwnedFd, inbox: &Inbox) {
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
            // The kernel destroys a trigger it can no longer serve and reports
            // it as an error, together with `POLLPRI`: that is the end of the
            // trigger, not pressure.
            if revents.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                gone.push(index);
            } else if revents.contains(PollFlags::PRI) {
                fired.push(triggers[index].0.stall());
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
            inbox.post(HostEvent::Notice(SystemNotice::MemoryPressure(level)));
        }
        if triggers.is_empty() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::memory::MemoryPressureLevel;

    use super::*;

    /// A pair of sockets whose server end can raise `POLLPRI` on the client
    /// end by sending out-of-band data: the kernel raises it the same way for
    /// a pressure trigger.
    fn raising_pair() -> (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ) {
        std::os::unix::net::UnixStream::pair().expect("a socket pair")
    }

    fn raise(server: &std::os::unix::net::UnixStream) {
        rustix::net::send(server, b"!", rustix::net::SendFlags::OOB).expect("out-of-band data");
    }

    fn inbox() -> Arc<Inbox> {
        Arc::new(Inbox {
            queue: parking_lot::Mutex::new(std::collections::VecDeque::new()),
            signal: std::sync::Weak::new(),
        })
    }

    fn levels(inbox: &Inbox) -> Vec<MemoryPressureLevel> {
        inbox
            .queue
            .lock()
            .drain(..)
            .map(|delivery| match delivery.event {
                HostEvent::Notice(SystemNotice::MemoryPressure(level)) => level,
                other => panic!("only pressure notices come from here, got {other:?}"),
            })
            .collect()
    }

    fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !check() {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {what}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Take the out-of-band byte, as the kernel clears a trigger's event once
    /// it has been polled: the stand-in socket stays readable until then.
    fn consume(client: &std::os::unix::net::UnixStream) {
        let mut byte = [0_u8; 1];
        rustix::net::recv(client, &mut byte, rustix::net::RecvFlags::OOB)
            .expect("the out-of-band byte");
    }

    /// The watcher thread's loop on sockets that stand in for the kernel's
    /// trigger files. A trigger raised alone posts its own level; triggers
    /// raised together post the heaviest; a trigger that ends is dropped and
    /// the thread ends with the last one. The sockets stay raised until read,
    /// where the kernel clears a trigger's event once it is polled, so the
    /// test reads the byte when it has heard enough.
    #[test]
    fn pressure_loop_posts_the_level_of_the_trigger_raised() {
        // (case, which are raised before the loop starts, the first level)
        let table = [
            ("some alone", [true, false], MemoryPressureLevel::Moderate),
            ("full alone", [false, true], MemoryPressureLevel::Critical),
            ("both at once", [true, true], MemoryPressureLevel::Critical),
        ];
        for (case, raised, expected) in table {
            let inbox = inbox();
            let stop =
                eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).expect("an eventfd");
            let (some_server, some_client) = raising_pair();
            let (full_server, full_client) = raising_pair();
            let some_kept = some_client.try_clone().expect("a duplicate");
            let full_kept = full_client.try_clone().expect("a duplicate");
            if raised[0] {
                raise(&some_server);
            }
            if raised[1] {
                raise(&full_server);
            }
            std::thread::scope(|scope| {
                let triggers = vec![
                    (triggers_for(Stall::Partial)[0], some_client),
                    (triggers_for(Stall::Full)[0], full_client),
                ];
                let thread = scope.spawn(|| run(triggers, &stop, &inbox));
                wait_for(case, || !inbox.queue.lock().is_empty());
                if raised[0] {
                    consume(&some_kept);
                }
                if raised[1] {
                    consume(&full_kept);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
                let heard = levels(&inbox);
                assert_eq!(heard.first(), Some(&expected), "{case}: {heard:?}");

                // Quiet once read; both ending leaves nothing to wait for.
                assert_eq!(levels(&inbox), [], "{case}: no pressure while quiet");
                drop(some_server);
                drop(full_server);
                wait_for("the thread to end", || thread.is_finished());
            });
            assert_eq!(
                levels(&inbox),
                [],
                "{case}: ending a trigger is not pressure"
            );
        }
    }

    /// A trigger the kernel destroys is reported as an error together with
    /// `POLLPRI`; it ends the trigger and is no pressure. Here the server end
    /// closes with out-of-band data pending, which the client end reports the
    /// same way.
    #[test]
    fn pressure_loop_treats_a_destroyed_trigger_as_the_end_not_as_pressure() {
        let inbox = inbox();
        let stop = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).expect("an eventfd");
        let (server, client) = raising_pair();
        raise(&server);
        drop(server);
        std::thread::scope(|scope| {
            let thread =
                scope.spawn(|| run(vec![(triggers_for(Stall::Full)[0], client)], &stop, &inbox));
            wait_for("the thread to end", || thread.is_finished());
        });
        assert_eq!(levels(&inbox), []);
    }

    #[test]
    fn pressure_loop_ends_when_asked_to_stop() {
        let inbox = inbox();
        let stop = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).expect("an eventfd");
        let (server, client) = raising_pair();
        std::thread::scope(|scope| {
            let thread =
                scope.spawn(|| run(vec![(triggers_for(Stall::Full)[0], client)], &stop, &inbox));
            rustix::io::write(&stop, &1_u64.to_ne_bytes()).expect("the stop signal");
            wait_for("the thread to stop", || thread.is_finished());
        });
        drop(server);
    }

    /// The real thing, where the kernel offers it: a process that may write the
    /// pressure file and has the privilege for a 1 s window gets its triggers
    /// (the kernel drops the last byte of a write, so an unterminated request
    /// would be refused), and stopping the watcher returns.
    #[test]
    fn the_watcher_starts_and_stops_on_the_kernels_triggers() {
        let writable = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PRESSURE_FILE)
            .is_ok();
        let inbox = inbox();
        let watcher = PressureWatcher::start(&inbox);
        if writable && rustix::process::geteuid().is_root() {
            assert!(watcher.is_some(), "root sets pressure triggers");
        }
        drop(watcher);
        assert_eq!(levels(&inbox), []);
    }
}
