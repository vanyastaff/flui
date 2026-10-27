//! What the raster thread draws into (ADR-0254 §5).
//!
//! A [`PlatformWindow`](crate::PlatformWindow) stays on its owner thread. The
//! thread that presents frames gets a [`SurfaceTarget`] from
//! [`PlatformWindow::surface_target`](crate::PlatformWindow::surface_target)
//! instead: the window's raw handles and the pre-present notification, and
//! nothing else of the window.

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

/// The surface of one window, as the thread that presents its frames holds
/// it.
///
/// It is `Send + Sync + 'static` so a renderer can own it on any thread for
/// as long as it keeps a GPU surface, and it implements the two
/// `raw-window-handle` traits wgpu's safe surface creation reads.
///
/// # Once the window is gone
///
/// A target does not keep its window open. Once the native window is
/// destroyed (closed, or recycled by the OS) or its surface is gone, both
/// handle queries answer [`HandleError::Unavailable`], and keep answering it:
/// a renderer that re-queries the target on recovery never receives a handle
/// whose window no longer exists. The handle a query returned before stays
/// the caller's to stop using; the window's handler is told first, through
/// [`SurfaceChange::Release`](crate::SurfaceChange::Release), on every host
/// whose surface can go away while the window lives.
///
/// How each host knows its window is gone:
///
/// - **Win32**: the window's identity probe (its class and the address of its
///   context), so a destroyed `HWND`, or one the OS recycled for another
///   window, answers `Unavailable`;
/// - **AppKit**: a closed flag, set when the window closes;
/// - **Android**: only while the activity has no native window (after
///   `TerminateWindow`); a pause that keeps the window keeps the target;
/// - **winit, UIKit and the web**: a closed flag, set when the window closes;
/// - **headless**: always, since no native window exists.
pub trait SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {
    /// A frame is about to be presented into this surface.
    ///
    /// The thread that presents calls it once per presented frame,
    /// immediately before the present. On Wayland it arms the surface's frame
    /// callback, so the next redraw the window asks for waits for the
    /// compositor: one per display refresh while the surface is visible, none
    /// while it is hidden. A host whose window system has no such protocol
    /// returns at once, and says so where it implements this.
    fn pre_present_notify(&self);
}

/// Asks one window for a frame from any thread (ADR-0254 §5).
///
/// [`PlatformWindow::request_redraw`](crate::PlatformWindow::request_redraw)
/// is the owner thread's, like every method of the window. A thread that is
/// not the owner — an async task's waker, a signal written on a worker, an
/// accessibility adapter's thread — asks through the requester the window
/// handed out instead. It never blocks: the host carries the request to the
/// owner where its window system needs that (AppKit), and asks the window
/// system directly where any thread may (Win32, winit, Android). Once the
/// window is gone, a request reaches nothing.
///
/// Any `Fn() + Send + Sync + 'static` closure is a requester, which is what a
/// window built outside the hosts hands out.
pub trait RedrawRequester: Send + Sync + 'static {
    /// Ask the window for a frame, as
    /// [`request_redraw`](crate::PlatformWindow::request_redraw) does on the
    /// owner thread.
    fn request_redraw(&self);
}

impl<F: Fn() + Send + Sync + 'static> RedrawRequester for F {
    fn request_redraw(&self) {
        self();
    }
}

/// The target of a window that has no native surface, such as a window
/// built from this crate alone: both handle queries answer
/// [`HandleError::Unavailable`], and
/// [`pre_present_notify`](SurfaceTarget::pre_present_notify) returns at once,
/// since nothing presents into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoNativeSurface;

impl HasWindowHandle for NoNativeSurface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for NoNativeSurface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for NoNativeSurface {
    /// Nothing presents into a surface that does not exist.
    fn pre_present_notify(&self) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// What a renderer takes: an owned raw-handle source it may keep on any
    /// thread. `Arc<dyn SurfaceTarget>` is one on its own, with no other crate
    /// in the graph enabling `raw-window-handle/alloc`.
    const fn assert_bounds<T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static>() {}
    const _: () = assert_bounds::<Arc<dyn SurfaceTarget>>();
}
