//! The Linux display source: the outputs of the compositor, kept current by a
//! thread of its own.
//!
//! [`LinuxDisplays`] is the shared half. It holds the registry that issues
//! display ids, the snapshot last published and the outputs last read, and it
//! answers the owner's `display_snapshot` and a window's "which display am I
//! on" from them without touching the connection. [`DisplayWatcher`] is the
//! other half: the connection and the thread that waits on it.
//!
//! # The thread
//!
//! The thread owns the connection and does nothing but `poll` it together with
//! a stop descriptor. When the compositor reports a change, the thread swaps
//! the new output list in, marks the snapshot stale and wakes the owner; the
//! owner's next `display_snapshot` resolves the list into a snapshot. No
//! application code runs on the thread: the wake posts to the owner and the
//! owner signal is held weakly, so even the last owner of the signal is never
//! released here.
//!
//! # Shutdown
//!
//! The caller closes the owner signal's admission first, so a wake that is
//! still on its way is inert. [`DisplayWatcher`]'s drop then writes the stop
//! descriptor and joins the thread; the thread closes the connection before it
//! ends, so the connection is released only after the thread has stopped using
//! it and the signal handle only after the last wake.
//!
//! # Failure
//!
//! A source that cannot connect, or loses its connection, answers every
//! snapshot with the error until a watcher is started again. It does not retry
//! on its own: a source that reconnected on every owner wake would turn a
//! missing display server into a busy loop.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;

use flui_platform_api::{DisplayId, DisplayInfo, DisplaySnapshot};
use parking_lot::Mutex;
use rustix::event::{EventfdFlags, eventfd};

use super::protocol::{Environment, Protocol};
use super::session::{DisplayError, Session, Step, connect};
use super::view::{FreshIds, NativeIds, OutputView};
use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;
use crate::shared::owner_signal::OwnerSignal;
use crate::shared::panic_boundary::contain_owner_callback;

/// The displays of one platform instance.
pub(crate) struct LinuxDisplays {
    source: DisplaySource,
    state: Mutex<State>,
}

struct State {
    /// The outputs last read; `None` until the first read.
    views: Option<Vec<OutputView>>,
    /// The protocol `views` came from.
    protocol: Option<Protocol>,
    /// The ids of outputs the protocol cannot key.
    fresh: FreshIds,
    /// Which native numbers belong to which display, as of the published
    /// snapshot.
    natives: Vec<NativeIds>,
    /// Why the source cannot answer, until a watcher starts again.
    unavailable: Option<String>,
}

impl LinuxDisplays {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            source: DisplaySource::new(),
            state: Mutex::new(State {
                views: None,
                protocol: None,
                fresh: FreshIds::default(),
                natives: Vec::new(),
                unavailable: None,
            }),
        })
    }

    /// The snapshot: the last published one while nothing has changed,
    /// otherwise the outputs resolved again.
    ///
    /// Before a watcher has started, the first call reads the compositor once
    /// on a connection of its own, so a platform that has not begun running
    /// still answers with the real displays.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when there is no display server to ask, the
    /// connection was lost, or the snapshot revision space is used up.
    pub(crate) fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with_aux(
            || self.sample(),
            |natives| self.state.lock().natives = natives,
        )
    }

    fn sample(&self) -> Result<(Vec<DisplayInfo>, Vec<NativeIds>), PlatformError> {
        let known = {
            let state = self.state.lock();
            if let Some(message) = &state.unavailable {
                return Err(PlatformError::Displays {
                    message: message.clone(),
                });
            }
            state.views.is_some()
        };
        if !known {
            // The compositor is read with no lock held.
            if let Err(error) = self.read_once() {
                let message = error.to_string();
                self.state.lock().unavailable = Some(message.clone());
                return Err(PlatformError::Displays { message });
            }
        }
        let mut state = self.state.lock();
        let Some(views) = state.views.clone() else {
            return Err(PlatformError::Displays {
                message: "the display list was lost while it was being read".into(),
            });
        };
        let resolved = state.fresh.resolve(&views, |key| self.source.mint(key));
        Ok((resolved.displays, resolved.natives))
    }

    /// Read the outputs once, on a connection that closes again.
    fn read_once(&self) -> Result<(), DisplayError> {
        let environment = Environment::current();
        let protocol = Protocol::from_environment(&environment).ok_or(DisplayError::NoServer)?;
        let session = connect(protocol, &environment)?;
        self.install(protocol, session.views());
        Ok(())
    }

    /// The display a native number names — a Wayland output's global name, an X11
    /// CRTC — in the last published snapshot. `None` when the protocol is not
    /// the one the outputs were read from, or when no display answers to the
    /// number; the latter also marks the snapshot stale once per revision, since
    /// a window can be on an output the snapshot does not hold yet.
    pub(crate) fn display_of_native(&self, protocol: Protocol, native: u32) -> Option<DisplayId> {
        let found = {
            let state = self.state.lock();
            if state.protocol != Some(protocol) {
                return None;
            }
            state
                .natives
                .iter()
                .find(|ids| ids.natives.contains(&native))
                .map(|ids| ids.id)
        };
        if found.is_none() {
            self.source.note_miss(u64::from(native));
        }
        found
    }

    /// Replace the outputs with a fresh read and mark the snapshot stale.
    fn install(&self, protocol: Protocol, views: Vec<OutputView>) {
        {
            let mut state = self.state.lock();
            if state.protocol != Some(protocol) {
                state.fresh = FreshIds::default();
                state.protocol = Some(protocol);
            }
            state.views = Some(views);
            state.unavailable = None;
        }
        self.source.invalidate();
    }

    /// Record that the source can no longer answer.
    fn lose(&self, message: String) {
        self.state.lock().unavailable = Some(message);
        self.source.invalidate();
    }

    /// Connect to the compositor `protocol` names and keep its outputs current
    /// until the returned watcher is dropped. `signal` is woken whenever they
    /// change; it is held weakly.
    ///
    /// The first read happens here, before the thread starts, so the snapshot
    /// is complete from the moment this returns.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the connection cannot be made; the
    /// source then answers with that error.
    pub(crate) fn watch(
        self: &Arc<Self>,
        protocol: Protocol,
        signal: &Arc<OwnerSignal>,
    ) -> Result<DisplayWatcher, PlatformError> {
        match connect(protocol, &Environment::current()) {
            Ok(session) => self.watch_session(protocol, signal, session),
            Err(error) => Err(self.fail(error.to_string())),
        }
    }

    /// The source cannot answer: record why, and report it.
    fn fail(&self, message: String) -> PlatformError {
        self.lose(message.clone());
        PlatformError::Displays { message }
    }

    /// [`watch`](Self::watch) over a session already connected, which is also
    /// how the tests give the thread a session with no compositor behind it.
    fn watch_session(
        self: &Arc<Self>,
        protocol: Protocol,
        signal: &Arc<OwnerSignal>,
        session: Box<dyn Session>,
    ) -> Result<DisplayWatcher, PlatformError> {
        let fail = |message: String| self.fail(message);
        let stop = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map_err(|error| {
            fail(format!(
                "cannot create the display thread's stop signal: {error}"
            ))
        })?;
        self.install(protocol, session.views());
        let stop = Arc::new(stop);
        let thread = std::thread::Builder::new()
            .name("flui-display-linux".into())
            .spawn({
                let displays = Arc::clone(self);
                let signal = Arc::downgrade(signal);
                let stop = Arc::clone(&stop);
                move || run(&displays, protocol, &signal, session, &stop)
            })
            .map_err(|error| fail(format!("cannot start the display thread: {error}")))?;
        Ok(DisplayWatcher {
            stop,
            thread: Some(thread),
        })
    }
}

/// The display thread's body.
fn run(
    displays: &Arc<LinuxDisplays>,
    protocol: Protocol,
    signal: &Weak<OwnerSignal>,
    mut session: Box<dyn Session>,
    stop: &OwnedFd,
) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop {
            match session.step(stop.as_fd()) {
                Ok(Step::Stopped) => return None,
                Ok(Step::Idle) => {}
                Ok(Step::Changed) => {
                    displays.install(protocol, session.views());
                    wake(signal);
                }
                Err(error) => return Some(error.to_string()),
            }
        }
    }));
    // The connection closes before the thread ends, so a join sees it closed.
    contain_owner_callback(|| drop(session));
    let failure = match outcome {
        Ok(failure) => failure,
        Err(payload) => {
            let message =
                crate::shared::panic_boundary::panic_payload_message(payload.as_ref()).to_owned();
            // The payload may own anything; it is kept rather than dropped
            // here, where a panicking `Drop` would end the thread twice over.
            std::mem::forget(payload);
            Some(format!("the display thread panicked: {message}"))
        }
    };
    if let Some(message) = failure {
        tracing::warn!(%message, "the Linux display source stopped");
        displays.lose(message);
        wake(signal);
    }
}

/// Wake the owner if it is still there. The upgraded handle can be the last
/// owner of the signal, and releasing it drops the owner-turn callback, which
/// is application code, so the whole body is a contained owner-callback.
fn wake(signal: &Weak<OwnerSignal>) {
    contain_owner_callback(|| {
        if let Some(signal) = signal.upgrade() {
            // A refused wake means the owner is closing; the stale flag stays
            // set for whatever samples next.
            let _ = signal.wake();
        }
    });
}

#[cfg(test)]
mod tests;

/// The connection and thread behind a [`LinuxDisplays`]. Dropping it stops the
/// thread and closes the connection.
pub(crate) struct DisplayWatcher {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for DisplayWatcher {
    fn drop(&mut self) {
        // The eventfd stays readable once written, so the thread sees it
        // whether it is polling or about to.
        if let Err(error) = rustix::io::write(self.stop.as_fd(), &1_u64.to_ne_bytes()) {
            tracing::error!(%error, "cannot signal the display thread to stop");
            // Joining a thread that was never told to stop would hang.
            return;
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the display thread ended in a panic that escaped its boundary");
        }
    }
}
