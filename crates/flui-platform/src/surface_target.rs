//! What the raster thread draws into (ADR-0254 §5).
//!
//! A [`PlatformWindow`](crate::PlatformWindow) stays on its owner thread. The
//! thread that presents frames gets a [`SurfaceTarget`] from
//! [`PlatformWindow::surface_target`](crate::PlatformWindow::surface_target)
//! instead: the window's raw handles and the pre-present notification, and
//! nothing else of the window.

use flui_foundation::FrameStamp;
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
/// the main thread alone). Win32, the web and headless answer on any
/// thread.
///
/// # Once the window is gone
///
/// A target does not keep its window open. The Apple targets keep the
/// window's content view alive until the renderer lets go of the target; the
/// others hold no reference to the window. Once the native window is
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
/// - **X11, Wayland and the web**: the retirement alone;
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
    /// `frame` is the stamp of the frame this present shows, `None` for a
    /// present that shows no frame of the window (a scene lent outside its
    /// frames). A host whose window reports presentation carries it with the
    /// present to the platform's answer for that present, and the report
    /// names it ([`FramePresented::frame`](crate::FramePresented::frame)); a
    /// present without a frame is never reported. A host whose window reports
    /// none ignores it.
    ///
    /// `statistics` is what the presenter read from its swap chain about the
    /// frames it presented before this one, for a host whose platform reports
    /// presentation there and nowhere the host can reach on its own (DXGI on
    /// Windows): the host turns it into the window's
    /// [`FrameRequest::presented`](crate::FrameRequest::presented). A presenter
    /// that reads nothing passes [`SwapChainStatistics::None`].
    fn pre_present_notify(&self, frame: Option<FrameStamp>, statistics: SwapChainStatistics);

    /// A raster thread that did not answer may still present over a surface
    /// made from this target, so the native window it names must outlive the
    /// close (ADR-0305 §9).
    ///
    /// Called on the owner thread when the window's renderer lets go of its
    /// slot, which is during the window's close on every host (the close
    /// releases the window's handler, and the handler owns the renderer), and
    /// before the target is leaked for the life of the process; a later call
    /// does nothing. The close under way then hides the native window instead
    /// of destroying it, and keeps what its handles name. The window stays
    /// closed to the application: its handler is released, it is in no window
    /// list, it takes no input, and the handle queries keep answering
    /// [`HandleError::Unavailable`](raw_window_handle::HandleError::Unavailable).
    /// A host called after its native window is already gone (destroyed by
    /// the OS or another client), or whose surface names no native window
    /// (the web), does nothing, and says so where it implements this.
    fn keep_past_close(&self);
}

/// The platform's connection to its window system, which the one GPU instance
/// of an application is made on (ADR-0305 §2).
///
/// wgpu reads an instance's display when the instance is made and again at
/// every surface it makes, and refuses a surface whose display differs. A
/// window's [`SurfaceTarget`] cannot be that display: once its window closes
/// it answers [`HandleError::Unavailable`](raw_window_handle::HandleError::Unavailable),
/// and the next window's surface would then find no display. The host hands
/// out its own connection instead, which lives as long as anything holds it:
///
/// - **X11** and **Wayland**: the connection the host's windows are made on,
///   so it answers the same display handle as every target of the host;
/// - **Win32**, **AppKit**, **UIKit**, **Android** and **the web**: their
///   display handle, which carries no pointer;
/// - **headless**: no display; it answers
///   [`HandleError::NotSupported`](raw_window_handle::HandleError::NotSupported),
///   and an instance made over it makes no window surface.
///
/// A connection that answered a handle once answers the same one for as long
/// as it is held, on any thread. Not implemented for a window's target, so
/// passing one where the connection belongs does not compile.
pub trait DisplayConnection: HasDisplayHandle + Send + Sync + 'static {}

/// What a presenter read from its swap chain before a present: the platform's
/// own record of when its earlier frames reached the screen, in the platform's
/// units, which the window's host rebases onto the event timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwapChainStatistics {
    /// The presenter read nothing.
    #[default]
    None,
    /// A DXGI swap chain's counters.
    Dxgi(DxgiStatistics),
}

/// What a presenter read from a DXGI swap chain before a present: how many
/// presents the swap chain made before this one, and what its frame
/// statistics name shown.
///
/// The two share one numbering, the swap chain's presents counted from its
/// creation: the present about to be made is number
/// [`last_present_count`](Self::last_present_count) + 1, and a shown present's
/// [`present_count`](DxgiFrameStatistics::present_count) is the number it was
/// made with. So the host keeps the frame of each numbered present and reports
/// a shown present as the frame it carried. A count lower than the one before
/// is a new swap chain, whose numbers name none of the old one's presents.
///
/// ```
/// use flui_platform::{DxgiFrameStatistics, DxgiShown, DxgiStatistics};
///
/// let read = DxgiStatistics::new(41, DxgiShown::Present(DxgiFrameStatistics::new(39, 1_201, 9_000_000)));
/// assert_eq!(read.last_present_count, 41);
/// assert!(matches!(read.shown, DxgiShown::Present(shown) if shown.present_count == 39));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DxgiStatistics {
    /// `GetLastPresentCount`: how many presents the swap chain made before the
    /// one about to be.
    pub last_present_count: u32,
    /// What `GetFrameStatistics` answered.
    pub shown: DxgiShown,
}

impl DxgiStatistics {
    /// The counters a presenter read, field by field in declaration order.
    #[must_use]
    pub const fn new(last_present_count: u32, shown: DxgiShown) -> Self {
        Self {
            last_present_count,
            shown,
        }
    }
}

/// What a DXGI swap chain's `GetFrameStatistics` answered before a present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DxgiShown {
    /// No statistics: nothing shown yet, or the read failed.
    Nothing,
    /// The newest present the display showed.
    Present(DxgiFrameStatistics),
    /// The statistics broke off (`DXGI_ERROR_FRAME_STATISTICS_DISJOINT`: a
    /// mode change, a move to another output): the next statistics do not
    /// compare with the earlier ones.
    Disjoint,
}

/// A DXGI swap chain's frame statistics, as `IDXGISwapChain::GetFrameStatistics`
/// fills `DXGI_FRAME_STATISTICS`, read before a present.
///
/// The statistics name the newest present the display has shown:
/// [`present_count`](Self::present_count) is that present's number, and it was
/// shown at the refresh [`sync_refresh_count`](Self::sync_refresh_count), which
/// began at [`sync_qpc_time`](Self::sync_qpc_time) on the performance counter.
/// A host reports a present once: a read naming the present or the refresh it
/// last reported is a repeat.
///
/// ```
/// use flui_platform::DxgiFrameStatistics;
///
/// let read = DxgiFrameStatistics::new(40, 1_201, 9_000_000);
/// assert_eq!(read.present_count, 40);
/// assert_eq!(read.sync_qpc_time, 9_000_000);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DxgiFrameStatistics {
    /// `PresentCount`: the number of the newest present the display showed.
    pub present_count: u32,
    /// `SyncRefreshCount`: the refresh at which it was shown.
    pub sync_refresh_count: u32,
    /// `SyncQPCTime`: when that refresh began, in `QueryPerformanceCounter`
    /// ticks.
    pub sync_qpc_time: i64,
}

impl DxgiFrameStatistics {
    /// The statistics a presenter read, field by field in declaration order.
    #[must_use]
    pub const fn new(present_count: u32, sync_refresh_count: u32, sync_qpc_time: i64) -> Self {
        Self {
            present_count,
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
/// system directly where any thread may (Win32, Android). Once the
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
