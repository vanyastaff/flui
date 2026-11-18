//! What an X11 window hands its renderer: the surface target (the
//! connection as its display and the window as its window, ADR-0254 §5), the
//! host's display connection (ADR-0305 §2) and the redraw requester any
//! thread may ring.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XcbDisplayHandle, XcbWindowHandle,
};

use crate::owner::linux::x11::{Doorbell, X11Connection};
use crate::owner::window_delivery::Retirement;

/// An X11 window's surface target (ADR-0254 §5): the platform's connection
/// as its display and the window as its window, until the window's
/// retirement.
pub(super) struct X11Surface {
    pub(super) connection: Arc<X11Connection>,
    pub(super) xid: NonZeroU32,
    pub(super) visual: Option<NonZeroU32>,
    pub(super) retirement: Retirement,
}

impl HasWindowHandle for X11Surface {
    #[expect(
        unsafe_code,
        reason = "a raw window handle is borrowed for as long as the target lives"
    )]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        if self.retirement.is_retired() {
            return Err(HandleError::Unavailable);
        }
        let mut handle = XcbWindowHandle::new(self.xid);
        handle.visual_id = self.visual;
        // SAFETY: the X window lives until the window's retirement has let go
        // of its handler (`X11Window::retire` destroys it in the retirement's
        // completion), and the handler owns the renderer that holds this
        // target and every surface made from the handle; once retirement has
        // started, no handle is handed out.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xcb(handle)) })
    }
}

impl HasDisplayHandle for X11Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        if self.retirement.is_retired() {
            return Err(HandleError::Unavailable);
        }
        xcb_display(&self.connection)
    }
}

/// The X11 host's display connection (ADR-0305 §2): the connection its windows
/// are made on, so its handle is every window target's.
pub(super) struct X11Display(pub(super) Arc<X11Connection>);

impl HasDisplayHandle for X11Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        xcb_display(&self.0)
    }
}

impl flui_platform::DisplayConnection for X11Display {}

/// The XCB display handle of `connection`, borrowed for as long as the
/// connection is.
#[expect(
    unsafe_code,
    reason = "a raw display handle is borrowed for as long as the connection lives"
)]
fn xcb_display(connection: &X11Connection) -> Result<DisplayHandle<'_>, HandleError> {
    let raw = std::ptr::NonNull::new(connection.xcb().get_raw_xcb_connection());
    let screen = i32::try_from(connection.screen_number()).map_err(|_| HandleError::Unavailable)?;
    // SAFETY: the libxcb connection the pointer names closes only when the
    // last `Arc<X11Connection>` drops, and the borrow cannot outlive the
    // `&X11Connection` it is made from.
    Ok(unsafe {
        DisplayHandle::borrow_raw(RawDisplayHandle::Xcb(XcbDisplayHandle::new(raw, screen)))
    })
}

impl flui_platform::SurfaceTarget for X11Surface {
    /// X11 has no frame callback to arm: a present is paced by the swap
    /// chain, and this returns at once. The swap chain's completion events go
    /// to the GPU driver's own connection, so the host learns nothing of when
    /// the frame is shown.
    fn pre_present_notify(
        &self,
        _: Option<flui_foundation::FrameStamp>,
        _: flui_platform::SwapChainStatistics,
    ) {
    }

    /// Records the keep and changes nothing else: a window still open keeps
    /// showing and taking input. Its retirement, whichever way it comes (a
    /// user's close, a command, the platform's release at quit), then unmaps
    /// the X window instead of destroying it (`X11Window::retire`), and this
    /// target, leaked, keeps the connection the window and the surface's
    /// display live on. A keep after the retirement destroyed the window, or
    /// after another client did, does nothing.
    fn keep_past_close(&self) {
        let _first = self.retirement.keep_past_close();
    }
}

/// An X11 window's redraw requester: any thread marks the window and rings
/// the loop's doorbell. It holds no window, so a requester a waker keeps
/// never keeps a closed window alive.
pub(super) struct X11Redraw {
    pub(super) redraw: Arc<AtomicBool>,
    pub(super) doorbell: Doorbell,
    pub(super) retirement: Retirement,
}

impl flui_platform::RedrawRequester for X11Redraw {
    fn request_redraw(&self) {
        if self.retirement.is_retired() {
            return;
        }
        self.redraw.store(true, Ordering::Release);
        if let Err(error) = self.doorbell.ring() {
            tracing::error!(%error, "a frame request could not wake the X11 loop");
        }
    }
}
