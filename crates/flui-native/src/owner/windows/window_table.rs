//! What one hook of FLUI's window class keeps of each window, and the scale
//! every hook reads a window at.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::USER_DEFAULT_SCREEN_DPI;

use super::{WindowIdentity, WindowRef};

/// A value one hook of FLUI's window class keeps for each window, by the
/// window's identity: put when the window is announced
/// ([`WindowMessages::created`](super::WindowMessages::created)), taken when
/// it is destroyed, or at its last message (`WM_NCDESTROY`) for a window whose
/// destruction was not announced.
///
/// No borrow is held while a value ends: [`Self::take`] and
/// [`Self::take_handle`] hand the value out, and the caller ends it once the
/// table is released, since ending it (a deactivation, a revocation) may reach
/// application code that reaches the table again. A read runs its closure with
/// the table borrowed, so the closure only copies out what it needs.
pub(crate) struct WindowTable<T> {
    windows: RefCell<HashMap<WindowIdentity, Kept<T>>>,
}

/// One window's value, with the reference the window was announced with.
struct Kept<T> {
    window: WindowRef,
    value: T,
}

impl<T> Default for WindowTable<T> {
    fn default() -> Self {
        Self {
            windows: RefCell::new(HashMap::new()),
        }
    }
}

impl<T> std::fmt::Debug for WindowTable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowTable")
            .field("windows", &self.windows.borrow().len())
            .finish()
    }
}

impl<T> WindowTable<T> {
    /// Keep `value` for `window`; answers a value kept for the same identity
    /// before (an identity is announced once, so none), handed out with the
    /// table released.
    pub(crate) fn put(&self, window: WindowRef, value: T) -> Option<T> {
        let replaced = self
            .windows
            .borrow_mut()
            .insert(window.identity(), Kept { window, value });
        replaced.map(|kept| kept.value)
    }

    /// What `read` makes of `window`'s value, while the table keeps one for
    /// the window the reference names (not another that took its handle).
    pub(crate) fn read<R>(&self, window: WindowRef, read: impl FnOnce(&T) -> R) -> Option<R> {
        self.windows
            .borrow()
            .get(&window.identity())
            .filter(|kept| kept.window == window)
            .map(|kept| read(&kept.value))
    }

    /// What `update` makes of `window`'s value, changing it in place.
    pub(crate) fn update<R>(
        &self,
        window: WindowRef,
        update: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.windows
            .borrow_mut()
            .get_mut(&window.identity())
            .filter(|kept| kept.window == window)
            .map(|kept| update(&mut kept.value))
    }

    /// The window `hwnd` names, while the table keeps a value for it.
    pub(crate) fn window_of(&self, hwnd: HWND) -> Option<WindowRef> {
        self.windows
            .borrow()
            .values()
            .find(|kept| kept.window.hwnd() == hwnd)
            .map(|kept| kept.window)
    }

    /// Take out the value kept for the window `identity` names, for the
    /// caller to end with the table released.
    pub(crate) fn take(&self, identity: WindowIdentity) -> Option<(WindowRef, T)> {
        let kept = self.windows.borrow_mut().remove(&identity)?;
        Some((kept.window, kept.value))
    }

    /// At a window's last message: take out what is left of the window `hwnd`
    /// names, for the caller to end with the table released.
    pub(crate) fn take_handle(&self, hwnd: HWND) -> Option<(WindowRef, T)> {
        let identity = self.window_of(hwnd)?.identity();
        self.take(identity)
    }
}

/// A window's DPI: one value per window, which the window's procedure sets
/// as the window takes a new DPI, before it tells its class's hooks
/// ([`WindowStep::Rescaled`](super::WindowStep::Rescaled)), and which the
/// window and every hook read. No hook keeps a copy that could lag behind it.
#[derive(Clone, Debug)]
pub(crate) struct WindowScale(Rc<Cell<u32>>);

impl WindowScale {
    /// A window at `dpi`.
    pub(crate) fn new(dpi: u32) -> Self {
        Self(Rc::new(Cell::new(dpi)))
    }

    /// The DPI a window opens at: the one the system reports for it, or, when
    /// the system reports none (0), the DPI of the scale it was placed at.
    pub(crate) fn at_open(reported_dpi: u32, placed_at: f64) -> Self {
        Self::new(opening_dpi(reported_dpi, placed_at))
    }

    /// Its DPI.
    pub(crate) fn dpi(&self) -> u32 {
        self.0.get()
    }

    /// Its scale: its DPI over the 96 of 100%.
    pub(crate) fn scale(&self) -> f64 {
        super::dpi_scale(self.dpi())
    }

    /// The window is now at `dpi`; answers the scale it was at.
    pub(crate) fn rescale(&self, dpi: u32) -> f64 {
        super::dpi_scale(self.0.replace(dpi))
    }
}

/// The DPI a window opens at, from the DPI the system reports for it (0 when
/// it reports none) and the scale it was placed at.
pub(super) fn opening_dpi(reported_dpi: u32, placed_at: f64) -> u32 {
    if reported_dpi != 0 {
        return reported_dpi;
    }
    let dpi = (placed_at * f64::from(USER_DEFAULT_SCREEN_DPI)).round();
    if dpi.is_finite() && dpi >= 1.0 && dpi <= f64::from(u16::MAX) {
        // In range by the check above.
        dpi as u32
    } else {
        USER_DEFAULT_SCREEN_DPI
    }
}
