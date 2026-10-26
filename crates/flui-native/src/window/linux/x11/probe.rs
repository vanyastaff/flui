//! The live half of the X11 window: [`X11WindowProbe`] reads the properties
//! [`mode`](super::mode) decides a window's mode from, holding two connections
//! to the X server `DISPLAY` names, apart from winit's own:
//!
//! - one **queries**, on whichever thread asks (`WM_STATE` and `_NET_WM_STATE`
//!   of a window), and sets the properties a client sets on a window that is
//!   not mapped yet;
//! - one **watches** on a thread of its own: it selects property changes on
//!   each window it is given and records which windows the window manager
//!   changed, then wakes the owner.
//!
//! They are two so a query's reply wait never takes an event the watching
//! thread, asleep in `poll`, would then not hear of.
//!
//! The watching thread runs no application code. It only marks windows; the
//! owner reads them in its next turn (`take_changed`) and asks each window to
//! sample itself again. The winit window reaches the probe as the window
//! model's [`WindowManagerView`], which the host wires.

use std::{
    collections::BTreeSet,
    os::fd::{AsFd, OwnedFd},
    sync::{Arc, Weak},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use flui_foundation::geometry::Size;
use flui_platform::WindowMode;
use parking_lot::Mutex;
use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, eventfd, poll},
    io::Errno,
};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _,
            EventMask, PropMode,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use super::mode::{NetWmState, NetWmStateAtoms, WmState, mode_flags};
use crate::owner::{panic_boundary::contain_owner_callback, signal::OwnerSignal};
use crate::window::state::mode_of;
use crate::window::{WindowManagerError, WindowManagerView};

/// `_NET_WM_STATE` is read this far (in 4-byte units).
const PROPERTY_READ_LENGTH: u32 = 64;

/// The atoms the probe reads and watches, as the server numbered them.
#[derive(Clone, Copy)]
struct Atoms {
    wm_state: xproto::Atom,
    net_wm_state: xproto::Atom,
    frame_extents: xproto::Atom,
    request_frame_extents: xproto::Atom,
    states: NetWmStateAtoms,
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, WindowManagerError> {
        let intern = |name: &[u8]| -> Result<xproto::Atom, WindowManagerError> {
            Ok(connection
                .intern_atom(false, name)
                .map_err(WindowManagerError::from_display)?
                .reply()
                .map_err(WindowManagerError::from_display)?
                .atom)
        };
        Ok(Self {
            wm_state: intern(b"WM_STATE")?,
            net_wm_state: intern(b"_NET_WM_STATE")?,
            frame_extents: intern(b"_NET_FRAME_EXTENTS")?,
            request_frame_extents: intern(b"_NET_REQUEST_FRAME_EXTENTS")?,
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

/// Query and watch the windows of one X server.
pub(crate) struct X11WindowProbe {
    atoms: Atoms,
    query: RustConnection,
    root: xproto::Window,
    /// Windows whose mode properties changed since the owner last asked.
    changed: Arc<Mutex<BTreeSet<u32>>>,
    watch: Arc<RustConnection>,
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl X11WindowProbe {
    /// Connect to the server `DISPLAY` names and start watching. `signal` is
    /// woken when a window's mode properties change; it is held weakly.
    ///
    /// # Errors
    /// [`WindowManagerError`] when a connection cannot be made or the thread cannot
    /// start.
    pub(crate) fn connect(signal: &Arc<OwnerSignal>) -> Result<Arc<Self>, WindowManagerError> {
        let (query, screen) =
            RustConnection::connect(None).map_err(WindowManagerError::from_display)?;
        let root = query
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| {
                WindowManagerError::from_display("the server lists no screen of the display number")
            })?
            .root;
        let atoms = Atoms::intern(&query)?;
        let watch = Arc::new(
            RustConnection::connect(None)
                .map_err(WindowManagerError::from_display)?
                .0,
        );
        let stop = Arc::new(
            eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                .map_err(WindowManagerError::from_display)?,
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
            .map_err(WindowManagerError::from_display)?;
        Ok(Arc::new(Self {
            atoms,
            query,
            root,
            changed,
            watch,
            stop,
            thread: Some(thread),
        }))
    }

    fn property(
        &self,
        window: u32,
        property: xproto::Atom,
        kind: xproto::Atom,
    ) -> Option<xproto::GetPropertyReply> {
        self.query
            .get_property(false, window, property, kind, 0, PROPERTY_READ_LENGTH)
            .ok()?
            .reply()
            .ok()
    }
}

impl WindowManagerView for X11WindowProbe {
    /// Start hearing of changes to `window`'s mode properties.
    ///
    /// The request is sent without waiting for its reply: this connection is
    /// the watching thread's to read, and a reply wait here would read events
    /// off the socket into the connection's queue, where the sleeping thread
    /// would not hear of them. A refusal (the window is gone) arrives as an
    /// error event the thread discards.
    fn watch(&self, window: u32) {
        let selected = self
            .watch
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map(x11rb::cookie::VoidCookie::ignore_error)
            .map_err(WindowManagerError::from_display)
            .and_then(|()| self.watch.flush().map_err(WindowManagerError::from_display));
        if let Err(error) = selected {
            tracing::debug!(%error, window, "cannot watch an X11 window's state");
        }
    }

    /// Set `mode` in `_NET_WM_STATE` of the not yet mapped `window`, replacing
    /// the maximized and fullscreen atoms it had and keeping any other, as EWMH
    /// has a client do for a window that is not managed yet: the window manager
    /// reads the property when it maps the window, and ignores a client message
    /// about a window it does not manage. [`WindowMode::Normal`] clears the
    /// mode, and so does [`WindowMode::Minimized`], which is no mode to be
    /// mapped in (the window is minimized after it is mapped).
    fn set_mode_hint(&self, window: u32, mode: WindowMode) {
        let states = &self.atoms.states;
        let wanted: &[u32] = match mode {
            WindowMode::Normal | WindowMode::Minimized => &[],
            WindowMode::Maximized => &[states.maximized_horz, states.maximized_vert],
            WindowMode::Fullscreen => &[states.fullscreen],
            _ => return,
        };
        let mode_atoms = [
            states.maximized_horz,
            states.maximized_vert,
            states.fullscreen,
        ];
        let current: Vec<u32> = self
            .property(window, self.atoms.net_wm_state, AtomEnum::ATOM.into())
            .and_then(|reply| Some(reply.value32()?.collect()))
            .unwrap_or_default();
        let mut atoms = current.clone();
        atoms.retain(|atom| !mode_atoms.contains(atom));
        atoms.extend_from_slice(wanted);
        // A write of what is there changes nothing and would still tell every
        // watcher of the window that something changed.
        if atoms == current {
            return;
        }
        let set = self
            .query
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms.net_wm_state,
                AtomEnum::ATOM,
                &atoms,
            )
            .map_err(WindowManagerError::from_display)
            .and_then(|cookie| cookie.check().map_err(WindowManagerError::from_display));
        if let Err(error) = set {
            tracing::debug!(%error, window, ?mode, "cannot set an X11 window's mode before it is mapped");
        }
    }
    /// The frame the window manager will put around the not yet mapped
    /// `window`, as the width and height it adds: asked for with
    /// `_NET_REQUEST_FRAME_EXTENTS` and read from `_NET_FRAME_EXTENTS`, waiting
    /// at most `wait` for the answer. `None` when the window manager does not
    /// answer (there is none, or it does not know the request).
    fn frame_extents(&self, window: u32, wait: Duration) -> Option<Size<i32>> {
        let request =
            ClientMessageEvent::new(32, window, self.atoms.request_frame_extents, [0_u32; 5]);
        self.query
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                request,
            )
            .ok()?
            .check()
            .ok()?;
        let deadline = Instant::now() + wait;
        loop {
            // left, right, top, bottom
            let extents: Option<[u32; 4]> = self
                .property(window, self.atoms.frame_extents, AtomEnum::CARDINAL.into())
                .and_then(|reply| reply.value32()?.collect::<Vec<_>>().try_into().ok());
            if let Some([left, right, top, bottom]) = extents {
                let sum = |a: u32, b: u32| i32::try_from(u64::from(a) + u64::from(b)).ok();
                return Some(Size::new(sum(left, right)?, sum(top, bottom)?));
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The mode the window manager gives `window`, `None` when the server
    /// cannot say (the window is gone, or the connection failed).
    fn mode(&self, window: u32) -> Option<WindowMode> {
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
        Some(mode_of(mode_flags(
            wm_state,
            NetWmState::from_atoms(&state_atoms, &self.atoms.states),
        )))
    }

    /// The windows whose mode properties changed since the last call.
    fn take_changed(&self) -> Vec<u32> {
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
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the X11 window thread ended in a panic");
        }
    }
}

/// Connects the probe to the server `DISPLAY` names as the window manager's
/// view the winit window reads; `signal` is woken when a watched window's
/// mode changes.
///
/// # Errors
/// What [`X11WindowProbe::connect`] reports.
pub(crate) fn connect_window_manager(
    signal: &Arc<OwnerSignal>,
) -> Result<Arc<dyn WindowManagerView>, WindowManagerError> {
    X11WindowProbe::connect(signal).map(|probe| probe as Arc<dyn WindowManagerView>)
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
