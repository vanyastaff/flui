//! What the X server and the window manager say about a window, which winit
//! does not report: whether it is minimized, and when that or its mode
//! changes.
//!
//! The probe holds two connections to the X server `DISPLAY` names, apart from
//! winit's own:
//!
//! - one **queries** on the owner thread (`WM_STATE` and `_NET_WM_STATE` of a
//!   window, decided by
//!   [`lifecycle_model::x11_window_mode`](super::lifecycle_model::x11_window_mode));
//! - one **watches** on a thread of its own: it selects property changes on
//!   each window it is given and records which windows the window manager
//!   changed, then wakes the owner.
//!
//! They are two so a query's reply wait never takes an event the watching
//! thread, asleep in `poll`, would then not hear of.
//!
//! The watching thread runs no application code. It only marks windows; the
//! owner reads them in its next turn ([`X11WindowProbe::take_changed`]) and
//! asks each window to sample itself again.

use std::{
    collections::BTreeSet,
    os::fd::{AsFd, OwnedFd},
    sync::{Arc, Weak},
    thread::JoinHandle,
};

use parking_lot::Mutex;
use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, eventfd, poll},
    io::Errno,
};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask},
    },
    rust_connection::RustConnection,
};

use super::lifecycle_model::x11_window_mode::{
    NetWmState, NetWmStateAtoms, WmState, X11WindowMode, window_mode,
};
use crate::shared::{owner_signal::OwnerSignal, panic_boundary::contain_owner_callback};

/// `_NET_WM_STATE` is read this far (in 4-byte units).
const STATE_ATOMS_LENGTH: u32 = 64;

/// The atoms the probe reads and watches, as the server numbered them.
#[derive(Clone, Copy)]
struct Atoms {
    wm_state: xproto::Atom,
    net_wm_state: xproto::Atom,
    states: NetWmStateAtoms,
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, ProbeError> {
        let intern = |name: &[u8]| -> Result<xproto::Atom, ProbeError> {
            Ok(connection
                .intern_atom(false, name)
                .map_err(ProbeError::from_display)?
                .reply()
                .map_err(ProbeError::from_display)?
                .atom)
        };
        Ok(Self {
            wm_state: intern(b"WM_STATE")?,
            net_wm_state: intern(b"_NET_WM_STATE")?,
            states: NetWmStateAtoms {
                hidden: intern(b"_NET_WM_STATE_HIDDEN")?,
                maximized_horz: intern(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
                maximized_vert: intern(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
                fullscreen: intern(b"_NET_WM_STATE_FULLSCREEN")?,
            },
        })
    }

    /// Whether a change of property `atom` can change a window's mode.
    fn concerns_mode(&self, atom: xproto::Atom) -> bool {
        atom == self.wm_state || atom == self.net_wm_state
    }
}

/// The probe could not connect.
#[derive(Debug, thiserror::Error)]
#[error("X11 window probe: {0}")]
pub(crate) struct ProbeError(String);

impl ProbeError {
    fn from_display(error: impl std::fmt::Display) -> Self {
        Self(error.to_string())
    }
}

/// Query and watch the windows of one X server.
pub(crate) struct X11WindowProbe {
    atoms: Atoms,
    query: RustConnection,
    /// Windows whose mode properties changed since the owner last asked.
    changed: Arc<Mutex<BTreeSet<u32>>>,
    watch: Arc<RustConnection>,
    stop: Arc<OwnedFd>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl X11WindowProbe {
    /// Connect to the server `DISPLAY` names and start watching. `signal` is
    /// woken when a window's mode properties change; it is held weakly.
    ///
    /// # Errors
    /// [`ProbeError`] when a connection cannot be made or the thread cannot
    /// start.
    pub(crate) fn connect(signal: &Arc<OwnerSignal>) -> Result<Arc<Self>, ProbeError> {
        let query = RustConnection::connect(None)
            .map_err(ProbeError::from_display)?
            .0;
        let atoms = Atoms::intern(&query)?;
        let watch = Arc::new(
            RustConnection::connect(None)
                .map_err(ProbeError::from_display)?
                .0,
        );
        let stop = Arc::new(
            eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                .map_err(ProbeError::from_display)?,
        );
        let changed = Arc::new(Mutex::new(BTreeSet::new()));
        let thread = std::thread::Builder::new()
            .name("flui-x11-window".into())
            .spawn({
                let watch = Arc::clone(&watch);
                let stop = Arc::clone(&stop);
                let changed = Arc::clone(&changed);
                let signal = Arc::downgrade(signal);
                move || run(&watch, atoms, &stop, &changed, &signal)
            })
            .map_err(ProbeError::from_display)?;
        Ok(Arc::new(Self {
            atoms,
            query,
            changed,
            watch,
            stop,
            thread: Mutex::new(Some(thread)),
        }))
    }

    /// Start hearing of changes to `window`'s mode properties.
    pub(crate) fn watch(&self, window: u32) {
        let selected = self
            .watch
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(ProbeError::from_display)
            .and_then(|cookie| cookie.check().map_err(ProbeError::from_display))
            .and_then(|()| self.watch.flush().map_err(ProbeError::from_display));
        if let Err(error) = selected {
            tracing::debug!(%error, window, "cannot watch an X11 window's state");
        }
    }

    /// The mode the window manager gives `window`, `None` when the server
    /// cannot say (the window is gone, or the connection failed).
    pub(crate) fn mode(&self, window: u32) -> Option<X11WindowMode> {
        let state_atoms = self
            .property(window, self.atoms.net_wm_state, AtomEnum::ATOM.into())?
            .value32()
            .map(Iterator::collect::<Vec<_>>)
            .unwrap_or_default();
        let wm_state = self
            .property(window, self.atoms.wm_state, self.atoms.wm_state)?
            .value32()
            .map(Iterator::collect::<Vec<_>>)
            .and_then(|words| WmState::from_property(&words));
        Some(window_mode(
            wm_state,
            NetWmState::from_atoms(&state_atoms, &self.atoms.states),
        ))
    }

    fn property(
        &self,
        window: u32,
        property: xproto::Atom,
        kind: xproto::Atom,
    ) -> Option<xproto::GetPropertyReply> {
        self.query
            .get_property(false, window, property, kind, 0, STATE_ATOMS_LENGTH)
            .ok()?
            .reply()
            .ok()
    }

    /// The windows whose mode properties changed since the last call.
    pub(crate) fn take_changed(&self) -> Vec<u32> {
        std::mem::take(&mut *self.changed.lock())
            .into_iter()
            .collect()
    }
}

impl Drop for X11WindowProbe {
    fn drop(&mut self) {
        // The eventfd stays readable once written, so the thread sees it
        // whether it is polling or about to.
        if let Err(error) = rustix::io::write(self.stop.as_fd(), &1_u64.to_ne_bytes()) {
            tracing::error!(%error, "cannot signal the X11 window thread to stop");
            return;
        }
        if let Some(thread) = self.thread.lock().take()
            && thread.join().is_err()
        {
            tracing::error!("the X11 window thread ended in a panic");
        }
    }
}

/// The watching thread: wait for the server or the stop signal, record the
/// windows whose mode properties changed, wake the owner.
fn run(
    connection: &RustConnection,
    atoms: Atoms,
    stop: &OwnedFd,
    changed: &Mutex<BTreeSet<u32>>,
    signal: &Weak<OwnerSignal>,
) {
    loop {
        let mut any = false;
        loop {
            match connection.poll_for_event() {
                Ok(Some(Event::PropertyNotify(property))) if atoms.concerns_mode(property.atom) => {
                    changed.lock().insert(property.window);
                    any = true;
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    tracing::debug!(%error, "the X11 window connection ended; window state changes are no longer heard");
                    return;
                }
            }
        }
        if any {
            contain_owner_callback(|| {
                if let Some(signal) = signal.upgrade() {
                    // A refused wake means the owner is closing.
                    let _ = signal.wake();
                }
            });
        }
        if connection.flush().is_err() {
            return;
        }
        let mut fds = [
            PollFd::new(connection.stream(), PollFlags::IN),
            PollFd::new(stop, PollFlags::IN),
        ];
        match poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => {
                tracing::debug!(%error, "polling the X11 window connection failed");
                return;
            }
        }
        if fds[1]
            .revents()
            .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP)
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
