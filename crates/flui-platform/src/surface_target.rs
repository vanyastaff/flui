//! What the raster thread draws into (ADR-0254 §5).
//!
//! A [`PlatformWindow`](crate::PlatformWindow) stays on its owner thread. The
//! thread that presents frames gets a [`SurfaceTarget`] from
//! [`PlatformWindow::surface_target`](crate::PlatformWindow::surface_target)
//! instead: the window's raw handles and the pre-present notification, and
//! nothing else of the window.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// The surface of one window, as the thread that presents its frames holds
/// it.
///
/// It is `Send + Sync + 'static` so a renderer can hold it, wherever it runs,
/// for as long as it keeps a GPU surface, and it implements the two
/// `raw-window-handle` traits wgpu's safe surface creation reads.
///
/// # Where a surface is made
///
/// Holding a target is free on any thread; making a surface from it is not.
/// The Apple targets hand out their view on the main thread only and the
/// Android target its native window on the looper thread only, and answer
/// [`HandleError::Unavailable`](raw_window_handle::HandleError::Unavailable) elsewhere: a renderer creates and recreates
/// its surface there (on Apple, `raw-window-metal` makes the Metal layer on
/// the main thread alone). Win32, winit, the web and headless answer on any
/// thread.
///
/// # Once the window is gone
///
/// A target does not keep its window open. The winit target holds the winit
/// window itself (winit's window is `Send + Sync` and must outlive a surface
/// made from it), so that window is let go when the renderer lets go of the
/// target; the Apple targets keep the window's content view alive the same
/// way; the others hold no reference to the window. Once the native window is
/// destroyed (closed, or recycled by the OS) or its surface is gone, both
/// handle queries answer [`HandleError::Unavailable`](raw_window_handle::HandleError::Unavailable), and keep answering it:
/// a renderer that re-queries the target on recovery never receives a handle
/// for a window that no longer exists. The handle a query returned before
/// stays the caller's to stop using; the window's handler is told first,
/// through [`SurfaceChange::Release`](crate::SurfaceChange::Release), on every
/// host whose surface can go away while the window lives.
///
/// How each host knows its window is gone: every host marks the window
/// retired where it retires its native window (its delivery's retirement),
/// and beyond that
///
/// - **Win32**: the window's identity probe (its class and the address of its
///   context), so a destroyed `HWND`, or one the OS recycled for another
///   window, answers `Unavailable`;
/// - **AppKit and UIKit**: the window's closed flag, set when the window
///   closes or its last handle drops; UIKit also answers `Unavailable` for a
///   window detached from its scene;
/// - **Android**: while the activity has no native window (after
///   `TerminateWindow`); a pause that keeps the window keeps the target;
/// - **winit and the web**: the retirement alone;
/// - **headless**: always, since no native window exists.
pub trait SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {
    /// A frame is about to be presented into this surface.
    ///
    /// The thread that presents calls it once per presented frame,
    /// immediately before the present. On Wayland it arms the surface's frame
    /// callback, so the next redraw the window asks for waits for the
    /// compositor: one per display refresh while the surface is visible, none
    /// while it is hidden, and asks the compositor when the frame is shown. A
    /// host whose window system has no such protocol returns at once, and says
    /// so where it implements this.
    ///
    /// `statistics` is what the presenter read from its swap chain about the
    /// frames it presented before this one, for a host whose platform reports
    /// presentation there and nowhere the host can reach on its own (DXGI on
    /// Windows): the host turns it into the window's
    /// [`FrameRequest::presented`](crate::FrameRequest::presented). A presenter
    /// that reads nothing passes [`SwapChainStatistics::None`].
    fn pre_present_notify(&self, statistics: SwapChainStatistics);
}

/// What a presenter read from its swap chain before a present: the platform's
/// own record of when its earlier frames reached the screen, in the platform's
/// units, which the window's host rebases onto the event timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwapChainStatistics {
    /// The presenter read nothing.
    #[default]
    None,
    /// A DXGI swap chain's `GetFrameStatistics` and `GetLastPresentCount`.
    Dxgi(DxgiFrameStatistics),
}

/// A DXGI swap chain's frame statistics, as `IDXGISwapChain::GetFrameStatistics`
/// fills `DXGI_FRAME_STATISTICS` and `GetLastPresentCount` answers, read before
/// a present.
///
/// The statistics name the newest present the display has shown:
/// [`present_count`](Self::present_count) is that present's number, and it was
/// shown at the refresh [`sync_refresh_count`](Self::sync_refresh_count), which
/// began at [`sync_qpc_time`](Self::sync_qpc_time) on the performance counter.
///
/// ```
/// use flui_platform::DxgiFrameStatistics;
///
/// let read = DxgiFrameStatistics::new(41, 40, 1_200, 1_201, 9_000_000);
/// assert_eq!(read.present_count, 40);
/// assert_eq!(read.sync_qpc_time, 9_000_000);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DxgiFrameStatistics {
    /// `GetLastPresentCount`: the number of the swap chain's last present
    /// before the one about to happen.
    pub last_present_count: u32,
    /// `PresentCount`: the number of the newest present the display showed.
    pub present_count: u32,
    /// `PresentRefreshCount`: the refresh at which that present was queued.
    pub present_refresh_count: u32,
    /// `SyncRefreshCount`: the refresh at which it was shown.
    pub sync_refresh_count: u32,
    /// `SyncQPCTime`: when that refresh began, in `QueryPerformanceCounter`
    /// ticks.
    pub sync_qpc_time: i64,
}

impl DxgiFrameStatistics {
    /// The statistics a presenter read, field by field in declaration order.
    #[must_use]
    pub const fn new(
        last_present_count: u32,
        present_count: u32,
        present_refresh_count: u32,
        sync_refresh_count: u32,
        sync_qpc_time: i64,
    ) -> Self {
        Self {
            last_present_count,
            present_count,
            present_refresh_count,
            sync_refresh_count,
            sync_qpc_time,
        }
    }
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
