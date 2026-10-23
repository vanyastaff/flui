//! The Windows host's text input: the Text Services Framework reads the
//! focused field's store (ADR-0135).
//!
//! - `geometry`: the pure half, a text range's logical rect as the physical
//!   screen rect an input method places its windows against, checked on every
//!   host;
//! - `tsf`: each window's TSF connection, its documents and their store, the
//!   window's `TextStoreHost`;
//! - [`Tsf`]: the text services' hook on FLUI's window class and the table's
//!   text position, which activates each new window's connection and keeps it
//!   by the window's identity until the window closes.
//!
//! The window's steps reach the connection through the hook, at the points
//! their order needs: a DPI change gives it the new scale as it is set and
//! tells it the field's screen rectangles moved once the rectangle that scale
//! asks for is applied, and the close step deactivates it between the close
//! callbacks and the context's retirement (ADR-0135 §3). The window hands out
//! its connection through the hook, which the class keeps for it
//! (`owner::windows::window_texts`).

pub(crate) mod geometry {
    //! A text range's logical rect as the physical screen rect an input method
    //! places its windows against (TSF `ITextStoreACP::GetTextExt`), and a
    //! screen point as the logical point it asks about (`GetACPFromPoint`).
    //!
    //! Free of `windows` types and built for tests on every host, so its table
    //! runs there; the Win32 text services are its one consumer. The store
    //! answers in window-root logical pixels
    //! (ADR-0030 §7); the screen wants whole physical pixels. Three rules decide
    //! the conversion:
    //!
    //! - the scale is the one the caller passes in (the window's own scale
    //!   factor, the same source pointer input uses), never a monitor query;
    //! - the result covers the logical rect: the left and top edges take the
    //!   floor and the right and bottom edges the ceiling, so a candidate window
    //!   placed against the rect never overlaps a partly covered pixel of the
    //!   text. `floor`, not truncation, so a negative coordinate (a field
    //!   scrolled past the client origin, a window on a monitor left of the
    //!   primary) moves outwards too;
    //! - an edge closer than [`SNAP_TOLERANCE`] device pixels to a whole pixel is
    //!   that pixel, so float noise from the multiplication (`0.1 × 3.0`) does
    //!   not grow the rect by one.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::{Bounds, DevicePixelRatio, DevicePoint, Point};

    /// How far, in device pixels, an edge may sit from a whole pixel and still be
    /// taken as that pixel.
    pub const SNAP_TOLERANCE: f64 = 1e-6;

    /// A rectangle in physical screen pixels, edges as Win32's `RECT` holds them:
    /// `right` and `bottom` are exclusive.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ScreenRect {
        /// The left edge.
        pub left: i32,
        /// The top edge.
        pub top: i32,
        /// The right edge, exclusive.
        pub right: i32,
        /// The bottom edge, exclusive.
        pub bottom: i32,
    }

    /// Why a logical rect has no screen rect. The text services answer it as
    /// "no layout" (`TS_E_NOLAYOUT`), so an input method waits for the next
    /// layout change instead of placing its window at a wrong position.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
    pub enum ScreenRectError {
        /// A coordinate or extent is NaN or infinite.
        #[error("the text rect is not finite")]
        NotFinite,
        /// An edge lies outside the `i32` range screen coordinates use.
        #[error("the text rect lies outside the screen coordinate range")]
        OutOfRange,
    }

    /// The physical screen rect covering `rect` (window-root logical pixels) in
    /// a window whose client area starts at `client_origin` (physical screen
    /// pixels) and whose scale is `ratio`.
    ///
    /// A rect of negative size is taken by its edges, so the answer is never
    /// inverted.
    ///
    /// # Errors
    ///
    /// [`ScreenRectError::NotFinite`] for a NaN or infinite input, and
    /// [`ScreenRectError::OutOfRange`] when an edge does not fit an `i32`.
    pub fn range_rect_to_screen(
        rect: Bounds<f64>,
        client_origin: DevicePoint,
        ratio: DevicePixelRatio,
    ) -> Result<ScreenRect, ScreenRectError> {
        let edges = [
            rect.origin.x,
            rect.origin.y,
            rect.origin.x + rect.size.width,
            rect.origin.y + rect.size.height,
        ];
        if edges.iter().any(|edge| !edge.is_finite()) {
            return Err(ScreenRectError::NotFinite);
        }
        let [x0, y0, x1, y1] = edges.map(|edge| ratio.to_device(edge));
        let left = outward(x0.min(x1), f64::floor) + f64::from(client_origin.x);
        let top = outward(y0.min(y1), f64::floor) + f64::from(client_origin.y);
        let right = outward(x0.max(x1), f64::ceil) + f64::from(client_origin.x);
        let bottom = outward(y0.max(y1), f64::ceil) + f64::from(client_origin.y);
        Ok(ScreenRect {
            left: whole_i32(left)?,
            top: whole_i32(top)?,
            right: whole_i32(right)?,
            bottom: whole_i32(bottom)?,
        })
    }

    /// The window-root logical point at `screen` (physical screen pixels) in a
    /// window whose client area starts at `client_origin` and whose scale is
    /// `ratio`: the inverse of [`range_rect_to_screen`]'s offset and scale, for
    /// the point an input method asks about (TSF `GetACPFromPoint`).
    ///
    /// The difference of the two `i32` coordinates is taken in `f64`, where it
    /// is exact, since it need not fit an `i32`. Dividing it by the ratio can
    /// still overflow: a ratio below one grows it, and [`DevicePixelRatio`]
    /// admits any finite positive ratio. A point whose logical coordinates are
    /// not finite has no position in the document, nearest or exact, so the
    /// answer is `None`, which the caller reports as a point outside it (TSF
    /// `TS_E_INVALIDPOINT`).
    #[must_use]
    pub fn screen_point_to_client(
        screen: DevicePoint,
        client_origin: DevicePoint,
        ratio: DevicePixelRatio,
    ) -> Option<Point<f64>> {
        let point = Point::new(
            ratio.to_logical(f64::from(screen.x) - f64::from(client_origin.x)),
            ratio.to_logical(f64::from(screen.y) - f64::from(client_origin.y)),
        );
        (point.x.is_finite() && point.y.is_finite()).then_some(point)
    }

    /// `device` rounded outwards by `round`, unless it is within
    /// [`SNAP_TOLERANCE`] of a whole pixel, which it then is.
    fn outward(device: f64, round: fn(f64) -> f64) -> f64 {
        let nearest = device.round();
        if (device - nearest).abs() < SNAP_TOLERANCE {
            nearest
        } else {
            round(device)
        }
    }

    /// `value`, already whole, as an `i32`; `OutOfRange` outside it (and for an
    /// infinity a huge scale produced).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the value is whole and checked against the i32 range first"
    )]
    fn whole_i32(value: f64) -> Result<i32, ScreenRectError> {
        if !value.is_finite() {
            return Err(ScreenRectError::OutOfRange);
        }
        if value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
            return Err(ScreenRectError::OutOfRange);
        }
        Ok(value as i32)
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(windows)]
mod tsf;

#[cfg(windows)]
pub(crate) use tsf::TextServices;

#[cfg(windows)]
use std::cell::RefCell;
#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::rc::Rc;

#[cfg(windows)]
use flui_platform::text_store::TextStoreHost;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{WM_EXITSIZEMOVE, WM_MOVE, WM_NCDESTROY};

#[cfg(windows)]
use crate::owner::windows::{
    CreatedWindow, WindowIdentity, WindowMessages, WindowRef, WindowStep, WindowTexts,
};

/// The text services' hook on FLUI's window class, and the Windows table's
/// text position: each window the class creates gets its TSF connection,
/// which the hook keeps by the window's identity from the window's creation
/// until its close callbacks ran (ADR-0135 §3), or its `WM_NCDESTROY` for a
/// window whose close was not announced. The class hands the connection out
/// for the window (`owner::windows::window_texts`).
///
/// Public where it is defined, as the host's table names it as its text
/// position, and nameable only inside the crate.
#[cfg(windows)]
#[derive(Default)]
pub struct Tsf {
    windows: RefCell<HashMap<WindowIdentity, TsfWindow>>,
}

/// What the hook keeps of one window: the window, and its connection once
/// TSF activated for it.
#[cfg(windows)]
struct TsfWindow {
    window: WindowRef,
    services: Option<Rc<TextServices>>,
}

#[cfg(windows)]
impl std::fmt::Debug for Tsf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tsf")
            .field("windows", &self.windows.borrow().len())
            .finish()
    }
}

#[cfg(windows)]
impl Tsf {
    /// The hook of a class on an owner thread, with no window yet.
    pub(crate) fn new() -> Rc<Self> {
        let tsf = Rc::new(Self::default());
        #[cfg(test)]
        testing::made(&tsf);
        tsf
    }

    /// The connection of `window`, while it is held; cloned, so no borrow is
    /// held while TSF and the field's store run.
    fn services(&self, window: WindowRef) -> Option<Rc<TextServices>> {
        self.windows
            .borrow()
            .get(&window.identity())
            .filter(|kept| kept.window == window)
            .and_then(|kept| kept.services.clone())
    }

    /// The connection of the window `hwnd` names.
    fn services_by_handle(&self, hwnd: HWND) -> Option<Rc<TextServices>> {
        self.windows
            .borrow()
            .values()
            .find(|kept| kept.window.hwnd() == hwnd)
            .and_then(|kept| kept.services.clone())
    }

    /// Forget the window `identity` names and deactivate its connection,
    /// with no borrow held: the shutdown reaches application code.
    fn retire(&self, identity: WindowIdentity) {
        let removed = self.windows.borrow_mut().remove(&identity);
        if let Some(services) = removed.and_then(|kept| kept.services) {
            TextServices::retire_with_window(services);
        }
    }
}

/// The text position of the Windows table: each window holds a handle to its
/// TSF connection, which serves the focused field's store (the pull model,
/// ADR-0135), on the window's own thread only.
#[cfg(windows)]
impl crate::host::Text<WindowRef> for Tsf {
    fn for_window(window: &WindowRef) -> std::sync::Arc<dyn crate::text::TextInput> {
        std::sync::Arc::new(TsfHandle(*window))
    }
}

/// The class hands out the connection the hook keeps for a window.
#[cfg(windows)]
impl WindowTexts for Tsf {
    fn store_host(&self, window: WindowRef) -> Option<Rc<dyn TextStoreHost>> {
        self.services(window)
            .map(|services| services as Rc<dyn TextStoreHost>)
    }
}

/// One window's handle to its TSF connection.
#[cfg(windows)]
struct TsfHandle(WindowRef);

#[cfg(windows)]
impl crate::text::TextInput for TsfHandle {
    /// The window's text-store host: its TSF connection, read on the thread
    /// that created the window (any other gets `None`); `None` when TSF did
    /// not activate for it, or once it closed.
    fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
        let host = crate::owner::windows::window_texts()?.store_host(self.0)?;
        Some(crate::text::WindowTextInput::Store(host))
    }

    /// Windows' text size is linear: there is nothing to capture.
    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    /// The hook deactivates TSF at the window's close step, after its close
    /// callbacks (ADR-0135 §3): nothing is left to detach.
    fn detach(&self) {}
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the window class's hook trait declares its message entry `unsafe`"
)]
impl WindowMessages for Tsf {
    /// Activate TSF for the window just installed, on its owner thread, and
    /// keep the connection: from here on the window offers its text-store
    /// host. A failure leaves the window without a host, and text input takes
    /// the `WM_CHAR` path, as it does with no apartment (the platform's is
    /// reachable on its owner thread only, where windows are opened). A
    /// message the activation dispatched may have closed the window; its
    /// connection is then deactivated at once, and the open fails.
    fn created(&self, window: &CreatedWindow) {
        let hwnd = window.hwnd;
        let Some(apartment) = window.apartment.clone() else {
            tracing::warn!(
                ?hwnd,
                "no COM apartment for TSF; text input falls back to WM_CHAR"
            );
            return;
        };
        let this = window.window();
        // Kept before TSF activates: a close the activation dispatches finds
        // the window and forgets it.
        let replaced = self.windows.borrow_mut().insert(
            window.identity,
            TsfWindow {
                window: this,
                services: None,
            },
        );
        drop(replaced);
        #[cfg(test)]
        if let Some(hook) = BEFORE_TSF_ACTIVATION.get() {
            hook(hwnd);
        }
        match TextServices::activate(this, apartment) {
            Ok(services) => {
                let kept = self
                    .windows
                    .borrow_mut()
                    .get_mut(&window.identity)
                    .map(|kept| kept.services = Some(Rc::clone(&services)))
                    .is_some();
                if !kept {
                    // The window is already closed (by a message the
                    // activation dispatched): deactivate at once.
                    TextServices::retire_with_window(services);
                }
            }
            Err(error) => {
                let failed = self.windows.borrow_mut().remove(&window.identity);
                drop(failed);
                tracing::warn!(
                    ?hwnd,
                    ?error,
                    "TSF did not activate for this window; text input falls back to WM_CHAR"
                );
            }
        }
    }

    /// The scale TSF answers an input method's layout queries at follows the
    /// window's as it is set; once the rectangle it asks for is applied, the
    /// focused field's text sits somewhere else on the screen, and TSF is
    /// told so. The window's close callbacks having run, its connection is
    /// deactivated while its handle is still valid.
    fn step(&self, window: WindowRef, step: WindowStep) {
        match step {
            WindowStep::Rescaled { dpi } => {
                if let Some(services) = self.services(window) {
                    services.rescale(dpi);
                }
            }
            WindowStep::ScaleApplied => {
                if let Some(services) = self.services(window) {
                    services.window_moved();
                }
            }
            WindowStep::Closed => self.retire(window.identity()),
            WindowStep::Resized(_)
            | WindowStep::Minimized
            | WindowStep::FocusLost
            | WindowStep::Painting
            | WindowStep::SizeMoveStarted
            | WindowStep::SizeMoveEnded
            | WindowStep::SettingChanged
            | WindowStep::SessionLocked(_)
            | WindowStep::KeyLetThrough { .. } => {}
        }
    }

    /// The window moved, or a size-and-move loop ended: the focused field's
    /// text sits somewhere else on the screen, and TSF is told so. The
    /// window's procedure still answers the message, after this hook: TSF
    /// reads the window's position from the system and its scale from the
    /// connection, neither of which the procedure's own handling of these
    /// messages changes. A window's last message forgets what is left of a
    /// window whose close was not announced. Both heard, not answered.
    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            WM_MOVE | WM_EXITSIZEMOVE => {
                if let Some(services) = self.services_by_handle(hwnd) {
                    services.window_moved();
                }
            }
            WM_NCDESTROY => {
                let identity = self
                    .windows
                    .borrow()
                    .iter()
                    .find(|(_, kept)| kept.window.hwnd() == hwnd)
                    .map(|(identity, _)| *identity);
                if let Some(identity) = identity {
                    self.retire(identity);
                }
            }
            _ => {}
        }
        None
    }
}

#[cfg(all(windows, test))]
thread_local! {
    /// Runs as a window's text services are about to activate: lets a test
    /// stand in for a message the activation dispatches.
    static BEFORE_TSF_ACTIVATION: std::cell::Cell<Option<fn(HWND)>> =
        const { std::cell::Cell::new(None) };
}

/// The hooks made on this thread, for the tests that reach a window's
/// connection as its class keeps it.
#[cfg(all(windows, test))]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::rc::{Rc, Weak};

    use windows::Win32::Foundation::HWND;

    use super::{TextServices, Tsf};

    thread_local! {
        /// The text services' hook of every class made on this thread.
        static MADE: RefCell<Vec<Weak<Tsf>>> = const { RefCell::new(Vec::new()) };
    }

    /// Record a class's hook, made on this thread.
    pub(super) fn made(tsf: &Rc<Tsf>) {
        MADE.with(|made| made.borrow_mut().push(Rc::downgrade(tsf)));
    }

    /// The connection of the window `hwnd` names, from any class made on
    /// this thread.
    pub(crate) fn services_of(hwnd: HWND) -> Option<Rc<TextServices>> {
        MADE.with(|made| {
            made.borrow()
                .iter()
                .filter_map(Weak::upgrade)
                .find_map(|tsf| tsf.services_by_handle(hwnd))
        })
    }
}
