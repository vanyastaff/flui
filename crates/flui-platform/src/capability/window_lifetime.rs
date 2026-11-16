//! Whether a provider's window is still open, and the notice when it closes.

use std::{cell::Cell, fmt, rc::Rc};

use flui_foundation::{Notifier, Subscription};

/// What a window's two halves share.
struct Lifetime {
    closed: Cell<bool>,
    closing: Notifier<()>,
}

/// Whether the window a [`WindowRef`](super::WindowRef) names is still open,
/// and the notice when it closes.
///
/// A UI runtime outlives each of its windows but one, so a provider that
/// answers a window later (a dialog modal to it, a sheet attached to it)
/// learns here that nothing is left to answer: it ends what it holds for the
/// window and stops asking the system about it.
///
/// Owner-thread (`!Send`), cheap to clone; clones observe one window.
#[derive(Clone)]
pub struct WindowLifetime(Rc<Lifetime>);

impl WindowLifetime {
    /// Whether the window has closed. It never opens again: a window that
    /// opens later is another window, with a lifetime of its own.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.0.closed.get()
    }

    /// Call `listener` once, on the owner thread, when the window closes,
    /// for as long as the returned watch is held.
    ///
    /// It runs inside the window's close, before the window's state is
    /// dropped, with no borrow of that state held; it must not block. A
    /// listener that panics does not keep the others from running, and the
    /// window still closes. On a window that has closed already the watch is
    /// inert and `listener` never runs: ask
    /// [`is_closed`](Self::is_closed) first.
    #[must_use = "the listener runs only while the watch is held"]
    pub fn on_close(&self, listener: impl Fn() + 'static) -> WindowCloseWatch {
        if self.is_closed() {
            return WindowCloseWatch(Subscription::inert());
        }
        WindowCloseWatch(self.0.closing.listen(Rc::new(listener)))
    }
}

impl fmt::Debug for WindowLifetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowLifetime")
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

/// A listener [`WindowLifetime::on_close`] runs while this is held;
/// dropping it detaches the listener.
#[must_use = "the listener runs only while the watch is held"]
pub struct WindowCloseWatch(
    #[expect(dead_code, reason = "held for its drop, which detaches the listener")] Subscription,
);

impl fmt::Debug for WindowCloseWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCloseWatch").finish_non_exhaustive()
    }
}

/// The window owner's half of a [`WindowLifetime`]: the UI runtime keeps one
/// per window and closes it when the window closes.
///
/// Not `Clone`: one owner closes a window. Dropping it closes the window
/// too, so a window whose owner went without closing it never reads as
/// open.
pub struct WindowCloser(Rc<Lifetime>);

impl WindowCloser {
    /// An open window's closer and the lifetime its providers observe.
    #[must_use]
    pub fn new() -> (Self, WindowLifetime) {
        let lifetime = Rc::new(Lifetime {
            closed: Cell::new(false),
            closing: Notifier::new(),
        });
        (Self(Rc::clone(&lifetime)), WindowLifetime(lifetime))
    }

    /// The window closed: mark it closed and run every watching listener,
    /// once. Later calls do nothing.
    ///
    /// # Panics
    ///
    /// With the first panic of a listener, once every listener has run; the
    /// window reads as closed either way.
    pub fn close(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        // Marked before the listeners run, so one that asks reads closed
        // and one that subscribes gets an inert watch.
        self.0.closing.notify(&());
    }
}

impl Drop for WindowCloser {
    fn drop(&mut self) {
        self.close();
    }
}

impl fmt::Debug for WindowCloser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCloser")
            .field("closed", &self.0.closed.get())
            .finish_non_exhaustive()
    }
}
