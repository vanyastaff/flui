//! Memory pressure from the kernel's pressure triggers.
//!
//! A thread holds one trigger per level open on `/proc/pressure/memory` and
//! sleeps in `poll` until the kernel marks one readable (`POLLPRI`), which it
//! does when the stall the trigger names has lasted its threshold within its
//! window. Which trigger the kernel accepts, and which level each stands for,
//! is a pure rule ([`triggers`]), which every host checks; the thread runs on
//! a Linux host.
//!
//! A kernel without pressure information, or one that refuses a process the
//! trigger, leaves the level without a source: it is logged once and the
//! other level, if it has one, runs on.

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::fd::{AsFd, OwnedFd},
    sync::Arc,
    thread::JoinHandle,
};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use flui_platform::SystemNotice;
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, eventfd, poll},
    io::Errno,
};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use super::{HostEvent, Inbox};
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use triggers::{Stall, Trigger, level_of, triggers_for};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const PRESSURE_FILE: &str = "/proc/pressure/memory";

/// The thread that waits for pressure. Dropping it stops the thread.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(super) struct PressureWatcher {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
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

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
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
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
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

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
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

#[cfg(all(test, target_os = "linux", feature = "winit-backend"))]
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

#[cfg(any(target_os = "linux", test))]
#[forbid(unsafe_code)]
pub(crate) mod triggers {
    //! Memory pressure from the kernel's pressure stall information (PSI).
    //!
    //! The kernel exposes memory pressure as `/proc/pressure/memory`, a file with a
    //! `some` line (the share of time at least one task stalled on memory) and a
    //! `full` line (every non-idle task did).
    //! Reading the averages would mean polling. Writing a *trigger*
    //! (`some 150000 1000000`: wake me when tasks stall 150 ms within any 1 s
    //! window) to the same file and polling the descriptor for `POLLPRI` wakes the
    //! reader only when pressure is real, so the watcher sleeps otherwise.
    //!
    //! Two triggers give the two levels: a `some` stall is
    //! [`MemoryPressureLevel::Moderate`] (free what is easy to rebuild), a `full` stall
    //! [`MemoryPressureLevel::Critical`] (free all but what the frame needs).
    //!
    //! # Windows the kernel accepts
    //!
    //! A window is 500 ms to 10 s and a threshold no longer than its window. A
    //! process without `CAP_SYS_RESOURCE` may only use a window that is a multiple
    //! of 2 s, so each level has a second trigger with a 2 s window and a
    //! proportionally longer threshold, tried when the kernel refuses the first.

    use std::time::Duration;

    use flui_foundation::memory::MemoryPressureLevel;

    /// How much of the system waits on memory.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Stall {
        /// At least one task waits.
        Partial,
        /// Every non-idle task waits.
        Full,
    }

    impl Stall {
        fn word(self) -> &'static str {
            match self {
                Self::Partial => "some",
                Self::Full => "full",
            }
        }

        /// The level this stall is reported as.
        pub(crate) fn level(self) -> MemoryPressureLevel {
            match self {
                Self::Partial => MemoryPressureLevel::Moderate,
                Self::Full => MemoryPressureLevel::Critical,
            }
        }
    }

    /// The heaviest level any of `fired` asks for, `None` for none.
    pub(crate) fn level_of(fired: impl IntoIterator<Item = Stall>) -> Option<MemoryPressureLevel> {
        fired.into_iter().map(Stall::level).max()
    }

    /// A trigger the kernel accepts: wake when `stall` lasts `threshold` within
    /// any `window`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Trigger {
        stall: Stall,
        threshold: Duration,
        window: Duration,
    }

    /// The kernel's bounds on a trigger's window.
    const WINDOW_MIN: Duration = Duration::from_millis(500);
    const WINDOW_MAX: Duration = Duration::from_secs(10);
    /// The window an unprivileged process may use is a multiple of this.
    const UNPRIVILEGED_WINDOW_STEP: Duration = Duration::from_secs(2);

    #[derive(Debug, thiserror::Error, PartialEq, Eq)]
    pub(crate) enum TriggerError {
        #[error("pressure window {0:?} is outside 500 ms to 10 s")]
        WindowOutOfRange(Duration),
        #[error("pressure threshold {threshold:?} is zero or longer than its window {window:?}")]
        ThresholdOutOfRange {
            threshold: Duration,
            window: Duration,
        },
    }

    impl Trigger {
        pub(crate) fn new(
            stall: Stall,
            threshold: Duration,
            window: Duration,
        ) -> Result<Self, TriggerError> {
            if !(WINDOW_MIN..=WINDOW_MAX).contains(&window) {
                return Err(TriggerError::WindowOutOfRange(window));
            }
            if threshold.is_zero() || threshold > window {
                return Err(TriggerError::ThresholdOutOfRange { threshold, window });
            }
            Ok(Self {
                stall,
                threshold,
                window,
            })
        }

        pub(crate) fn stall(&self) -> Stall {
            self.stall
        }

        /// Whether the kernel lets a process without `CAP_SYS_RESOURCE` set it.
        #[cfg(test)]
        pub(crate) fn is_unprivileged(&self) -> bool {
            self.window
                .as_micros()
                .is_multiple_of(UNPRIVILEGED_WINDOW_STEP.as_micros())
        }

        /// The text to write to `/proc/pressure/memory`, in microseconds:
        /// `some 150000 1000000`. The kernel takes one write as one request.
        pub(crate) fn request(&self) -> String {
            format!(
                "{} {} {}",
                self.stall.word(),
                self.threshold.as_micros(),
                self.window.as_micros()
            )
        }

        /// The bytes to write to `/proc/pressure/memory`: the request and a NUL.
        /// The kernel overwrites the last byte it is given with its own
        /// terminator, so a write without one loses the last digit of the window
        /// (`some 150000 1000000` arrives as `some 150000 100000`, a threshold
        /// longer than its window, and is refused).
        pub(crate) fn request_bytes(&self) -> Vec<u8> {
            let mut bytes = self.request().into_bytes();
            bytes.push(0);
            bytes
        }
    }

    /// The triggers to try for `stall`, in the order to try them: the one with a
    /// 1 s window, then the one an unprivileged process may set.
    pub(crate) fn triggers_for(stall: Stall) -> [Trigger; 2] {
        let threshold = match stall {
            Stall::Partial => Duration::from_millis(150),
            Stall::Full => Duration::from_millis(100),
        };
        let window = Duration::from_secs(1);
        let unprivileged_window = UNPRIVILEGED_WINDOW_STEP;
        // The same share of the window, so the fallback is no more eager.
        let unprivileged_threshold = threshold * 2;
        [
            Trigger::new(stall, threshold, window).expect("BUG: the built-in trigger is in range"),
            Trigger::new(stall, unprivileged_threshold, unprivileged_window)
                .expect("BUG: the built-in fallback trigger is in range"),
        ]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn psi_trigger_requests() {
            // (stall, level, [first request, fallback request])
            let table = [
                (
                    Stall::Partial,
                    MemoryPressureLevel::Moderate,
                    ["some 150000 1000000", "some 300000 2000000"],
                ),
                (
                    Stall::Full,
                    MemoryPressureLevel::Critical,
                    ["full 100000 1000000", "full 200000 2000000"],
                ),
            ];
            for (stall, level, expected) in table {
                let triggers = triggers_for(stall);
                let requests = triggers.map(|trigger| trigger.request());
                assert_eq!(requests, expected, "{stall:?}");
                for (trigger, text) in triggers.iter().zip(expected) {
                    let bytes = trigger.request_bytes();
                    assert_eq!(
                        bytes.last(),
                        Some(&0),
                        "{text}: the kernel eats a last byte"
                    );
                    assert_eq!(&bytes[..bytes.len() - 1], text.as_bytes());
                }
                assert!(
                    triggers
                        .iter()
                        .all(|trigger| trigger.stall() == stall && trigger.stall().level() == level)
                );
                assert!(
                    !triggers[0].is_unprivileged(),
                    "{stall:?}: the 1 s window needs a privilege"
                );
                assert!(
                    triggers[1].is_unprivileged(),
                    "{stall:?}: the fallback needs none"
                );
            }
        }

        #[test]
        fn psi_trigger_bounds() {
            let ms = Duration::from_millis;
            // (case, threshold, window, accepted)
            let table = [
                ("shortest window", ms(100), ms(500), true),
                ("longest window", ms(100), ms(10_000), true),
                ("threshold equals window", ms(1000), ms(1000), true),
                ("window under 500 ms", ms(100), ms(499), false),
                ("window over 10 s", ms(100), ms(10_001), false),
                ("zero window", ms(0), ms(0), false),
                ("zero threshold", ms(0), ms(1000), false),
                ("threshold over window", ms(1001), ms(1000), false),
            ];
            for (case, threshold, window, accepted) in table {
                assert_eq!(
                    Trigger::new(Stall::Partial, threshold, window).is_ok(),
                    accepted,
                    "{case}"
                );
            }
            assert_eq!(
                Trigger::new(Stall::Full, ms(100), ms(20_000)),
                Err(TriggerError::WindowOutOfRange(ms(20_000)))
            );
            assert_eq!(
                Trigger::new(Stall::Full, ms(0), ms(1000)),
                Err(TriggerError::ThresholdOutOfRange {
                    threshold: ms(0),
                    window: ms(1000)
                })
            );
        }

        #[test]
        fn psi_unprivileged_window_is_a_multiple_of_two_seconds() {
            let ms = Duration::from_millis;
            for (window, expected) in [
                (ms(1000), false),
                (ms(2000), true),
                (ms(4000), true),
                (ms(3000), false),
                (ms(2500), false),
            ] {
                let trigger = Trigger::new(Stall::Partial, ms(100), window).expect("in range");
                assert_eq!(trigger.is_unprivileged(), expected, "{window:?}");
            }
        }

        #[test]
        fn psi_fired_triggers_make_one_level() {
            use Stall::{Full, Partial};
            // (case, fired stalls, level)
            let table: &[(&str, &[Stall], Option<MemoryPressureLevel>)] = &[
                ("nothing fired", &[], None),
                (
                    "some alone",
                    &[Partial],
                    Some(MemoryPressureLevel::Moderate),
                ),
                ("full alone", &[Full], Some(MemoryPressureLevel::Critical)),
                (
                    "both in one poll",
                    &[Partial, Full],
                    Some(MemoryPressureLevel::Critical),
                ),
                (
                    "both, full first",
                    &[Full, Partial],
                    Some(MemoryPressureLevel::Critical),
                ),
                (
                    "the same one twice",
                    &[Partial, Partial],
                    Some(MemoryPressureLevel::Moderate),
                ),
            ];
            for (case, fired, expected) in table {
                assert_eq!(level_of(fired.iter().copied()), *expected, "{case}");
            }
        }
    }
}
